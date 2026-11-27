mod common;
// agent-row drain path (batching + 30s gate) — extracted from src (CHORE-004)

use byline::ingest::forward::{drain, ForwardConfig, Table};
use byline::store::agent_heartbeats;
use std::sync::{Arc, Mutex};
use std::time::Duration;

static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn insert_n(store: &Arc<Mutex<byline::store::Store>>, n: usize, ts_offset: i64) {
    let s = store.lock().unwrap();
    for _ in 0..n {
        let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let ev = common::test_ev(&format!("ev{id}"), now + ts_offset);
        agent_heartbeats::insert(&s, &ev, agent_heartbeats::HOOK_SOURCE).unwrap();
    }
}

fn cfg_for(port: u16) -> ForwardConfig {
    ForwardConfig { api_url: format!("http://127.0.0.1:{port}/api/hackatime/v1"), api_key: "k".into() }
}

/// (a) 250 queued agent rows -> 3 POSTs of <=100 (one POST per drain pass;
/// the 30s gate is backdated between passes).
#[test]
fn batch_250_rows_three_posts_of_100() {
    let (store, _) = common::test_store("batch");
    insert_n(&store, 250, 0);
    let (port, th) = common::stub_upstream(3, "HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n"); let cfg = cfg_for(port);
    for _ in 0..3 {
        drain(&store, Some(cfg.clone()), 250, Table::AgentHeartbeats);
        store.lock().unwrap().conn.execute("UPDATE config SET value = CAST(CAST(value AS INTEGER) - 60 AS TEXT) WHERE key = 'agent_last_post'", []).unwrap();
    }
    let reqs = th.join().unwrap();
    assert_eq!(reqs.len(), 3, "expected 3 POSTs");
    let total: usize = reqs.iter().map(|r| {
        let b = common::body_of(r);
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
    let (store, _) = common::test_store("gate-stale");
    insert_n(&store, 1, 0); // fresh
    insert_n(&store, 1, -7200); // stale replay
    let (port, th) = common::stub_upstream(1, "HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n"); let cfg = cfg_for(port);
    assert_eq!(drain(&store, Some(cfg.clone()), 10, Table::AgentHeartbeats), 1);
    // only the fresh row forwarded; the stale one was dropped, not queued
    assert_eq!(agent_heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 0);
    insert_n(&store, 1, 0); // fresh row, but the gate still holds
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(drain(&store, Some(cfg.clone()), 10, Table::AgentHeartbeats), 0);
    assert_eq!(th.join().unwrap().len(), 1, "second drain must not POST");
    assert_eq!(agent_heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 1);
}
