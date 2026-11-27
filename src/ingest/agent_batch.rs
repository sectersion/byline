// agent-row drain path (FEAT-002 task 3): batches of <=100 per POST, at most
// one agent POST per 30s (config `agent_last_post`). Editor rows never come here.

use crate::ingest::forward::{failures, post_one, set_failures, ForwardConfig};
use crate::hooks::compose;
use crate::store::{agent_heartbeats, Store};

const AGENT_BATCH: usize = 100; // max heartbeats per upstream POST
const POST_INTERVAL: i64 = 30; // min seconds between agent POSTs
const GATE_KEY: &str = "agent_last_post";

pub fn drain_agent(s: &Store, cfg: &ForwardConfig, limit: usize) -> usize {
    if !gate_open(s) { return 0; }
    let rows = agent_heartbeats::unforwarded(s, limit.min(AGENT_BATCH)).unwrap_or_default();
    let (mut bodies, mut ids) = (Vec::new(), Vec::new());
    for r in rows {
        if compose::is_stale(r.received_at) {
            // stale replay: drop from the queue, never composed
            let _ = agent_heartbeats::mark_forwarded(s, r.id);
        } else {
            match compose::to_heartbeat(s, &r.raw, r.received_at) {
                Ok(body) => {
                    bodies.push(body);
                    ids.push(r.id);
                }
                Err(_) => {
                    // malformed stored row: drop it, so it cannot block the queue
                    let _ = agent_heartbeats::mark_forwarded(s, r.id);
                    agent_heartbeats::bump_hook_errors(s);
                }
            }
        }
    }
    if bodies.is_empty() { return 0; }
    let url = format!("{}/users/current/heartbeats.bulk", cfg.api_url.trim_end_matches('/'));
    match post_one(&url, &cfg.api_key, &bodies.join(",")) { // one JSON array
        Ok(()) => {
            for id in &ids {
                let _ = agent_heartbeats::mark_forwarded(s, *id);
            }
            set_gate(s);
            set_failures(s, 0);
            ids.len()
        }
        Err(_) => {
            set_failures(s, failures(s) + 1);
            0
        }
    }
}

fn gate_open(s: &Store) -> bool {
    let last: Option<i64> = s.conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM config WHERE key = ?", [GATE_KEY], |r| r.get(0),
    ).ok();
    last.map_or(true, |t| now_secs() - t >= POST_INTERVAL)
}

fn set_gate(s: &Store) {
    let _ = s.conn.execute(
        "INSERT INTO config (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![GATE_KEY, now_secs().to_string()],
    );
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::forward::tests::{accept_until, read_request, request_complete, test_store};
    use crate::ingest::forward::{drain, Table};
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};


    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn insert_n(store: &Arc<Mutex<Store>>, n: usize, ts_offset: i64) {
        let s = store.lock().unwrap();
        for _ in 0..n {
            let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let ev = agent_heartbeats::test_ev(&format!("ev{id}"), now_secs() + ts_offset);
            agent_heartbeats::insert(&s, &ev, agent_heartbeats::HOOK_SOURCE).unwrap();
        }
    }

    /// Stub answering up to `n` POSTs with 201, recording each body.
    fn stub_n(n: usize) -> (u16, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let th = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut bodies = Vec::new();
            for _ in 0..n {
                let Some(mut s) = accept_until(&listener, deadline) else { break };
                s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let got = read_request(&mut s);
                if !request_complete(&got) { break; }
                bodies.push(got.split("\r\n\r\n").nth(1).unwrap_or("").to_string());
                let _ = s.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
                let _ = s.flush();
            }
            bodies
        });
        (port, th)
    }

    fn cfg_for(port: u16) -> ForwardConfig {
        ForwardConfig { api_url: format!("http://127.0.0.1:{port}/api/hackatime/v1"), api_key: "k".into() }
    }
    /// (a) 250 queued agent rows -> 3 POSTs of <=100 (one POST per drain pass;
    /// the 30s gate is backdated between passes).
    #[test]
    fn batch_250_rows_three_posts_of_100() {
        let store = test_store("batch");
        insert_n(&store, 250, 0);
        let (port, th) = stub_n(3); let cfg = cfg_for(port);
        for _ in 0..3 {
            drain(&store, Some(cfg.clone()), 250, Table::AgentHeartbeats);
            store.lock().unwrap().conn.execute("UPDATE config SET value = CAST(CAST(value AS INTEGER) - 60 AS TEXT) WHERE key = 'agent_last_post'", []).unwrap();
        }
        let bodies = th.join().unwrap();
        assert_eq!(bodies.len(), 3, "expected 3 POSTs");
        let total: usize = bodies.iter().map(|b| {
            let n = serde_json::from_str::<serde_json::Value>(b).unwrap().as_array().unwrap().len();
            assert!(n <= 100, "POST batch larger than 100: {n}");
            n
        }).sum();
        assert_eq!(total, 250, "all rows sent across the batches");
        assert_eq!(agent_heartbeats::unforwarded(&store.lock().unwrap(), 300).unwrap().len(), 0);
    }

    /// (b)+(c): a stale row (2h old) is dropped, never composed; and a drain
    /// 5s after a POST defers (one POST only; the fresh row stays queued).
    #[test]
    fn gate_defers_and_stale_is_dropped() {
        let store = test_store("gate-stale");
        insert_n(&store, 1, 0); // fresh
        insert_n(&store, 1, -7200); // stale replay
        let (port, th) = stub_n(1); let cfg = cfg_for(port);
        assert_eq!(drain(&store, Some(cfg.clone()), 10, Table::AgentHeartbeats), 1);
        // only the fresh row forwarded; the stale one was dropped, not queued
        assert_eq!(agent_heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 0);
        insert_n(&store, 1, 0); // fresh row, but the gate still holds
        std::thread::sleep(Duration::from_secs(5));
        assert_eq!(drain(&store, Some(cfg.clone()), 10, Table::AgentHeartbeats), 0);
        drop(cfg);
        assert_eq!(th.join().unwrap().len(), 1, "second drain must not POST");
        assert_eq!(agent_heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 1);
    }
}
