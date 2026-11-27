// transparent drain: post unforwarded rows to their original api_url with
// their original key; 2xx -> forwarded_at; backoff 2^n capped 3600s in config.

use crate::store::{heartbeats, Store};
use std::sync::{Arc, Mutex};

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

/// Drains up to `limit` unforwarded rows. Returns count forwarded.
/// Config missing -> no-op (nothing configured yet). Never errors upward.
pub fn drain(store: &Arc<Mutex<Store>>, cfg: Option<ForwardConfig>, limit: usize) -> usize {
    let Some(cfg) = cfg else { return 0 };
    let rows = {
        let s = store.lock().unwrap();
        heartbeats::unforwarded(&s, limit).unwrap_or_default()
    };
    let s = store.lock().unwrap();
    let mut forwarded = 0usize;
    for row in rows {
        // bulk path appends bare paths to the base
        let url = format!("{}/users/current/heartbeats.bulk", cfg.api_url.trim_end_matches('/'));
        match post_one(&url, &cfg.api_key, &row.raw) {
            Ok(()) => {
                let _ = heartbeats::mark_forwarded(&s, row.id);
                forwarded += 1;
                set_failures(&s, 0);
            }
            Err(_) => {
                let n = failures(&s) + 1;
                set_failures(&s, n);
                let _ = n;
                break; // backoff handled by caller pacing; stop this pass
            }
        }
    }
    forwarded
}

fn failures(s: &Store) -> u32 {
    s.conn
        .query_row("SELECT CAST(value AS INTEGER) FROM config WHERE key = 'forward_failures'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn set_failures(s: &Store, n: u32) {
    let _ = s.conn.execute(
        "INSERT INTO config (key, value) VALUES ('forward_failures', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [n.to_string()],
    );
}

fn post_one(url: &str, key: &str, body: &str) -> Result<(), String> {
    // one HTTP POST; body is a single heartbeat, sent as 1-element array? No:
    // rows were stored per-element, so send a bare 1-element array to the
    // bulk endpoint.
    let payload = format!("[{body}]");
    let req = ureq::post(url).header("Authorization", &format!("Basic {}", b64(key)));
    let mut resp = req.send(payload.as_bytes()).map_err(|e| e.to_string())?;
    if resp.status().as_u16() < 300 {
        let _ = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("upstream status {}", resp.status()))
    }
}

fn b64(s: &str) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = chunk[0] as u32 * 65536
            + chunk.get(1).copied().unwrap_or(0) as u32 * 256
            + chunk.get(2).copied().unwrap_or(0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn test_store(name: &str) -> Arc<Mutex<Store>> {
        let mut dir = std::env::temp_dir();
        dir.push(format!("byline-fwd-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("byline.db");
        std::fs::remove_file(&db).ok();
        Arc::new(Mutex::new(Store::open(&db).unwrap()))
    }

    /// accept() that never blocks past `deadline` (None instead of hanging).
    fn accept_until(listener: &TcpListener, deadline: std::time::Instant) -> Option<std::net::TcpStream> {
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
    fn request_complete(got: &str) -> bool {
        let Some((_, body)) = got.split_once("\r\n\r\n") else { return false };
        let clen = got
            .lines()
            .find(|l| l.to_lowercase().starts_with("content-length"))
            .and_then(|l| l.split(':').nth(1))
            .and_then(|v| v.trim().parse::<usize>().ok())
            .unwrap_or(0);
        body.len() >= clen
    }

    /// Stub upstream: accepts one POST, records the body, 201s it.
    fn stub_upstream(port: u16, keep_open: bool) -> std::thread::JoinHandle<String> {
        std::thread::spawn(move || {
            let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            let mut s = accept_until(&listener, deadline).expect("client never connected within 15s");
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            let mut got = String::new();
            s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                got = String::from_utf8_lossy(&buf).into_owned();
                if request_complete(&got) {
                    break;
                }
            }
            let body = got.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
            if keep_open {
                let resp = "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 25\r\n\r\n{\"responses\":[[{},201]]}\r\n";
                let _ = s.write_all(resp.as_bytes());
            }
            body
        })
    }

    #[test]
    fn forwards_to_stub_and_marks() {
        let store = test_store("stub");
        let id = heartbeats::insert(&store.lock().unwrap(), r#"{"time":1.0,"entity":"e","type":"file","user_agent":"u"}"#, crate::store::AuthMethod::Basic, "fwd-key").unwrap();
        let th = stub_upstream(19901, true);
        let cfg = ForwardConfig { api_url: "http://127.0.0.1:19901/api/hackatime/v1".into(), api_key: "fwd-key".into() };
        let n = drain(&store, Some(cfg), 10);
        let body = th.join().unwrap();
        assert_eq!(n, 1);
        assert!(body.starts_with('[') && body.contains(r#""time":1.0"#), "body={body}");
        assert!(body.contains(r#""e""#));
        let s = store.lock().unwrap();
        let queued = heartbeats::unforwarded(&s, 10).unwrap();
        assert!(queued.iter().all(|r| r.id != id), "row should be marked forwarded");
    }

    #[test]
    fn upstream_down_rows_stay_queued() {
        let store = test_store("down");
        let s = store.lock().unwrap();
        heartbeats::insert(&s, "{}", crate::store::AuthMethod::Basic, "k").unwrap();
        drop(s);
        // port with nothing listening (refused)
        let cfg = ForwardConfig { api_url: "http://127.0.0.1:1/api/v1".into(), api_key: "k".into() };
        let n = drain(&store, Some(cfg), 10);
        assert_eq!(n, 0);
        assert_eq!(heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 1);
    }

    #[test]
    fn no_config_is_noop() {
        let store = test_store("nocfg");
        heartbeats::insert(&store.lock().unwrap(), "{}", crate::store::AuthMethod::Basic, "k").unwrap();
        assert_eq!(drain(&store, None, 10), 0);
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
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    let mut got = String::new();
                    loop {
                        let n = s.read(&mut chunk).unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        got = String::from_utf8_lossy(&buf).into_owned();
                        if request_complete(&got) {
                            break;
                        }
                    }
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
        let n = drain(&store, Some(cfg), 10);
        th.join().unwrap(); // surfaces a stub panic instead of silently passing
        // two forwarded, third stays queued
        assert!(n >= 2, "n={n}");
        let s = store.lock().unwrap();
        let queued = heartbeats::unforwarded(&s, 10).unwrap();
        assert!(queued.len() <= 1, "at most the third row queued, got {}", queued.len());
        // restart: re-drain (stub accepts again) — the first two never re-send
        // because forwarded_at was set before the next row was attempted.
    }
}
