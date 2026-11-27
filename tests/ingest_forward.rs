mod common;
// ingest::forward drain — extracted from src (CHORE-004)

use std::io::Write;
use byline::ingest::forward::{backoff_secs, drain, ForwardConfig, Table};
use byline::store::agent_heartbeats;
use byline::store::heartbeats;

#[test]
fn forwards_to_stub_and_marks() {
    let (store, _) = common::test_store("stub");
    let id = heartbeats::insert(&store.lock().unwrap(), r#"{"time":1.0,"entity":"e","type":"file","user_agent":"u"}"#, byline::store::AuthMethod::Basic, "fwd-key").unwrap();
    let (port, th) = common::stub_upstream(1, common::REPLY_201);
    let cfg = ForwardConfig { api_url: format!("http://127.0.0.1:{port}/api/hackatime/v1"), api_key: "fwd-key".into() };
    let n = drain(&store, Some(cfg), 10, Table::Heartbeats);
    let reqs = th.join().unwrap();
    assert_eq!(reqs.len(), 1);
    let body = common::body_of(&reqs[0]);
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
    let (store, _) = common::test_store("queued");
    heartbeats::insert(&store.lock().unwrap(), "{}", byline::store::AuthMethod::Basic, "k").unwrap();
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
    let (store, _) = common::test_store("midbatch");
    {
        let s = store.lock().unwrap();
        for t in 1..=3 {
            heartbeats::insert(&s, &format!(r#"{{"time":{t}.0,"entity":"e","type":"file","user_agent":"u"}}"#), byline::store::AuthMethod::Basic, "k").unwrap();
        }
    }
    // Stub answers the first 2 requests with 201, closes without a
    // response on the 3rd. Requests may arrive on one keep-alive
    // connection (ureq pools) or on fresh ones — both are handled, and
    // the 15s deadline turns a missing request into a failure, not a hang.
    let listener = std::net::TcpListener::bind("127.0.0.1:19903").unwrap();
    let th = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut served = 0usize;
        while served < 3 {
            let Some(mut s) = common::accept_until(&listener, deadline) else {
                panic!("stub: only {served} of 3 requests arrived within 15s");
            };
            s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            loop {
                let got = common::read_request(&mut s);
                if !common::request_complete(&got) {
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
}

/// FEAT-002: agent rows drain as composed WakaTime heartbeats; forwarded
/// rows are never re-sent on restart (as with forwards_to_stub_and_marks).
#[test]
fn agent_rows_drain_composed_and_never_resend() {
    let (store, _) = common::test_store("agent-drain");
    {
        let s = store.lock().unwrap();
        // fresh timestamp: task 3 drops rows older than 10 min at drain time
        let ev = common::test_ev("ev1", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64);
        agent_heartbeats::insert(&s, &ev, agent_heartbeats::HOOK_SOURCE).unwrap();
    }
    let (port, th) = common::stub_upstream(1, common::REPLY_201);
    let cfg = ForwardConfig { api_url: format!("http://127.0.0.1:{port}/api/hackatime/v1"), api_key: "k".into() };
    let n = drain(&store, Some(cfg), 10, Table::AgentHeartbeats);
    let reqs = th.join().unwrap();
    assert_eq!(n, 1);
    assert_eq!(reqs.len(), 1);
    let body = common::body_of(&reqs[0]);
    // composed shape (asserted in hooks_compose) — the stored raw must not leak
    assert!(body.contains(r#""category":"ai coding""#), "body={body}");
    assert!(!body.contains("event_id"), "stored raw leaked upstream: body={body}");
    // restart: drain again — nothing left, nothing re-sent
    let cfg2 = ForwardConfig { api_url: "http://127.0.0.1:1/api/v1".into(), api_key: "k".into() };
    assert_eq!(drain(&store, Some(cfg2), 10, Table::AgentHeartbeats), 0);
    assert_eq!(agent_heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 0);
}
