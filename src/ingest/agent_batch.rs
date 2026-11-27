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

