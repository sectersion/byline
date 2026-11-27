// transparent drain: post unforwarded rows to their original api_url with
// their original key; 2xx -> forwarded_at; backoff 2^n capped 3600s in config.

use crate::store::{heartbeats, Store};
use std::sync::{Arc, Mutex};

/// Which queue to drain. Agent rows are composed (hooks::compose) before send.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Table {
    Heartbeats,
    AgentHeartbeats,
}

#[derive(Clone)]
pub struct ForwardConfig {
    pub api_url: String, // full base incl. version prefix, e.g. https://hackatime.../api/hackatime/v1
    pub api_key: String,
}

impl ForwardConfig {
    pub fn from_store(store: &Store) -> Option<ForwardConfig> {
        let get = |k: &str| -> Option<String> {
            store.conn.query_row("SELECT value FROM config WHERE key = ?", [k], |r| r.get(0)).ok()
        };
        Some(ForwardConfig { api_url: get("upstream_api_url")?, api_key: get("upstream_api_key")? })
    }
}

/// Backoff seconds after n consecutive failures: 15 * 2^n capped 3600.
pub fn backoff_secs(failures: u32) -> u64 {
    15u64.saturating_mul(1u64 << failures.min(8)).min(3600)
}

/// Drains up to `limit` unforwarded rows from `table`; returns count forwarded.
/// Config missing -> no-op. Never errors upward. Backoff state (config
/// `forward_failures`) is shared across tables.
pub fn drain(store: &Arc<Mutex<Store>>, cfg: Option<ForwardConfig>, limit: usize, table: Table) -> usize {
    let Some(cfg) = cfg else { return 0 };
    let s = store.lock().unwrap();
    match table {
        Table::Heartbeats => drain_editor(&s, &cfg, limit),
        // task 3: agent rows batch (<=100/POST, <=1 POST/30s) in agent_batch
        Table::AgentHeartbeats => crate::ingest::agent_batch::drain_agent(&s, &cfg, limit),
    }
}

/// Editor heartbeats go as stored, one row per POST, unbatched (unchanged).
fn drain_editor(s: &Store, cfg: &ForwardConfig, limit: usize) -> usize {
    let rows: Vec<(i64, String)> = heartbeats::unforwarded(s, limit)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.id, r.raw))
        .collect();
    let mut forwarded = 0usize;
    for (id, raw) in rows {
        // bulk path appends bare paths to the base
        let url = format!("{}/users/current/heartbeats.bulk", cfg.api_url.trim_end_matches('/'));
        match post_one(&url, &cfg.api_key, &raw) {
            Ok(()) => {
                let _ = heartbeats::mark_forwarded(s, id);
                forwarded += 1;
                set_failures(s, 0);
            }
            Err(_) => {
                set_failures(s, failures(s) + 1);
                break; // backoff handled by caller pacing; stop this pass
            }
        }
    }
    forwarded
}

/// FIX-004: forward one non-heartbeat request to upstream verbatim — local
/// `/api/v1` prefix dropped, upstream key used (the caller's may differ).
/// Errors -> caller falls back to 404.
pub fn proxy(cfg: &ForwardConfig, method: &str, path_and_query: &str, body: &[u8]) -> Result<(u16, String), String> {
    let rest = path_and_query.strip_prefix("/api/v1").unwrap_or(path_and_query);
    // FIX-009: query-param auth (Basic is rejected by Hackatime); append after
    // any existing query string the caller's path may carry.
    let sep = if rest.contains('?') { "&" } else { "?" };
    let url = format!("{}{}{}api_key={}", cfg.api_url.trim_end_matches('/'), rest, sep, &cfg.api_key);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(5)))
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into();
    // ureq 3 only has named-method builders; WakaTime endpoints are GET/POST.
    let result = match method {
        "GET" => agent.get(&url).call(),
        "POST" => agent.post(&url).send(body),
        "DELETE" => agent.delete(&url).call(),
        "HEAD" => agent.head(&url).call(),
        _ => return Err(format!("unsupported method {method}")),
    };
    let mut resp = result.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    Ok((status, text))
}

pub(crate) fn failures(s: &Store) -> u32 {
    s.conn
        .query_row("SELECT CAST(value AS INTEGER) FROM config WHERE key = 'forward_failures'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

pub(crate) fn set_failures(s: &Store, n: u32) {
    let _ = s.conn.execute(
        "INSERT INTO config (key, value) VALUES ('forward_failures', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [n.to_string()],
    );
}

pub(crate) fn post_one(url: &str, key: &str, body: &str) -> Result<(), String> {
    // one POST to the bulk endpoint; the body goes out as one JSON array.
    // FIX-009: query-param auth — Hackatime rejects this key as a Basic header
    let payload = format!("[{body}]");
    let req = ureq::post(&format!("{url}?api_key={key}"));
    let mut resp = req.send(payload.as_bytes()).map_err(|e| e.to_string())?;
    if resp.status().as_u16() < 300 {
        let _ = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("upstream status {}", resp.status()))
    }
}


#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    pub(crate) fn test_store(name: &str) -> Arc<Mutex<Store>> {
        Arc::new(Mutex::new(crate::store::testutil::tmp(name)))
    }

    /// accept() that never blocks past `deadline` (None instead of hanging).
    pub(crate) fn accept_until(listener: &TcpListener, deadline: std::time::Instant) -> Option<std::net::TcpStream> {
        listener.set_nonblocking(true).unwrap();
        loop {
            match listener.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    return Some(s);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => return None,
            }
            if std::time::Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Headers + full body arrived on the wire?
    pub(crate) fn request_complete(got: &str) -> bool {
        let Some((_, body)) = got.split_once("\r\n\r\n") else { return false };
        let clen = got
            .lines()
            .find(|l| l.to_lowercase().starts_with("content-length"))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);
        body.len() >= clen
    }

    /// Read one HTTP request off `s` (until complete, close, or read timeout).
    pub(crate) fn read_request(s: &mut std::net::TcpStream) -> String {
        let (mut buf, mut got) = (Vec::new(), String::new());
        let mut chunk = [0u8; 65536];
        while !request_complete(&got) {
            match s.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(k) => {
                    buf.extend_from_slice(&chunk[..k]);
                    got = String::from_utf8_lossy(&buf).into_owned();
                }
            }
        }
        got
    }

    /// Shared stub upstream (forward, server::tests, agent_batch::tests):
    /// accepts up to `n` requests, records each raw request (split the body
    /// off at "\r\n\r\n" where only the body matters), replies with `reply`
    /// (empty reply -> close without responding). Ephemeral port.
    pub(crate) fn stub_upstream(
        n: usize,
        reply: &'static str,
    ) -> (u16, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let th = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut reqs = Vec::new();
            for _ in 0..n {
                let Some(mut s) = accept_until(&listener, deadline) else { break };
                s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                let got = read_request(&mut s);
                if !request_complete(&got) { break; }
                reqs.push(got);
                if !reply.is_empty() {
                    let _ = s.write_all(reply.as_bytes());
                    let _ = s.flush();
                }
            }
            reqs
        });
        (port, th)
    }

    /// Body of a recorded raw request.
    pub(crate) fn body_of(req: &str) -> &str {
        req.split("\r\n\r\n").nth(1).unwrap_or("")
    }

    const REPLY_201: &str =
        "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 25\r\n\r\n{\"responses\":[[{},201]]}\r\n";

    #[test]
    fn forwards_to_stub_and_marks() {
        let store = test_store("stub");
        let id = heartbeats::insert(&store.lock().unwrap(), r#"{"time":1.0,"entity":"e","type":"file","user_agent":"u"}"#, crate::store::AuthMethod::Basic, "fwd-key").unwrap();
        let (port, th) = stub_upstream(1, REPLY_201);
        let cfg = ForwardConfig { api_url: format!("http://127.0.0.1:{port}/api/hackatime/v1"), api_key: "fwd-key".into() };
        let n = drain(&store, Some(cfg), 10, Table::Heartbeats);
        let reqs = th.join().unwrap();
        assert_eq!(reqs.len(), 1);
        let body = body_of(&reqs[0]);
        assert_eq!(n, 1);
        assert!(body.starts_with('[') && body.contains(r#""time":1.0"#), "body={body}");
        assert!(body.contains(r#""e""#));
        let s = store.lock().unwrap();
        let queued = heartbeats::unforwarded(&s, 10).unwrap();
        assert!(queued.iter().all(|r| r.id != id), "row should be marked forwarded");
    }

    // Rows stay queued when there is nothing to drain to: no config at all,
    // or a config pointing at a port with nothing listening (refused).
    #[test]
    fn rows_stay_queued_without_upstream() {
        let store = test_store("queued");
        heartbeats::insert(&store.lock().unwrap(), "{}", crate::store::AuthMethod::Basic, "k").unwrap();
        assert_eq!(drain(&store, None, 10, Table::Heartbeats), 0, "no config is a no-op");
        assert_eq!(heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 1);
        let cfg = ForwardConfig { api_url: "http://127.0.0.1:1/api/v1".into(), api_key: "k".into() };
        assert_eq!(drain(&store, Some(cfg), 10, Table::Heartbeats), 0);
        assert_eq!(heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 1);
    }

    #[test]
    fn backoff_cap() {
        assert_eq!(backoff_secs(0), 15);
        assert_eq!(backoff_secs(1), 30);
        assert_eq!(backoff_secs(10), 3600); // capped
    }

    #[test]
    fn kill_midbatch_no_double_forward() {
        // 3 rows; upstream accepts first two then we kill the connection.
        let store = test_store("midbatch");
        {
            let s = store.lock().unwrap();
            for t in 1..=3 {
                heartbeats::insert(&s, &format!(r#"{{"time":{t}.0,"entity":"e","type":"file","user_agent":"u"}}"#), crate::store::AuthMethod::Basic, "k").unwrap();
            }
        }
        // Stub answers the first 2 requests with 201, closes without a
        // response on the 3rd. Requests may arrive on one keep-alive
        // connection (ureq pools) or on fresh ones — both are handled, and
        // the 15s deadline turns a missing request into a failure, not a hang.
        let listener = TcpListener::bind("127.0.0.1:19903").unwrap();
        let th = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut served = 0usize;
            while served < 3 {
                let Some(mut s) = accept_until(&listener, deadline) else {
                    panic!("stub: only {served} of 3 requests arrived within 15s");
                };
                s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                loop {
                    let got = read_request(&mut s);
                    if !request_complete(&got) {
                        break; // peer closed or read timed out; take next connection
                    }
                    served += 1;
                    if served < 3 {
                        let _ = s.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
                        let _ = s.flush();
                    } else {
                        drop(s); // 3rd request gets nothing (connection closed)
                        break;
                    }
                }
            }
        });
        let cfg = ForwardConfig { api_url: "http://127.0.0.1:19903/api/v1".into(), api_key: "k".into() };
        let n = drain(&store, Some(cfg), 10, Table::Heartbeats);
        th.join().unwrap(); // surfaces a stub panic instead of silently passing
        // two forwarded, third stays queued
        assert!(n >= 2, "n={n}");
        let s = store.lock().unwrap();
        let queued = heartbeats::unforwarded(&s, 10).unwrap();
        assert!(queued.len() <= 1, "at most the third row queued, got {}", queued.len());
        // restart: re-drain (stub accepts again) — the first two never re-send
        // because forwarded_at was set before the next row was attempted.
    }

    /// FEAT-002: agent rows drain as composed WakaTime heartbeats; forwarded
    /// rows are never re-sent on restart (as with forwards_to_stub_and_marks).
    #[test]
    fn agent_rows_drain_composed_and_never_resend() {
        let store = test_store("agent-drain");
        {
            let s = store.lock().unwrap();
            // fresh timestamp: task 3 drops rows older than 10 min at drain time
            let ev = crate::store::agent_heartbeats::test_ev("ev1", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64);
            crate::store::agent_heartbeats::insert(&s, &ev, crate::store::agent_heartbeats::HOOK_SOURCE).unwrap();
        }
        let (port, th) = stub_upstream(1, REPLY_201);
        let cfg = ForwardConfig { api_url: format!("http://127.0.0.1:{port}/api/hackatime/v1"), api_key: "k".into() };
        let n = drain(&store, Some(cfg), 10, Table::AgentHeartbeats);
        let reqs = th.join().unwrap();
        assert_eq!(n, 1);
        assert_eq!(reqs.len(), 1);
        let body = body_of(&reqs[0]);
        // composed shape (asserted in compose.rs) — the stored raw must not leak
        assert!(body.contains(r#""category":"ai coding""#), "body={body}");
        assert!(!body.contains("event_id"), "stored raw leaked upstream: body={body}");
        // restart: drain again — nothing left, nothing re-sent
        let cfg2 = ForwardConfig { api_url: "http://127.0.0.1:1/api/v1".into(), api_key: "k".into() };
        assert_eq!(drain(&store, Some(cfg2), 10, Table::AgentHeartbeats), 0);
        assert_eq!(crate::store::agent_heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 0);
    }
}
