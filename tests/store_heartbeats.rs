mod common;
// store insert/queue ops — extracted from src (CHORE-004)

use byline::store::{agent_heartbeats, heartbeats, AuthMethod};

// ---- heartbeats ----

#[test]
fn insert_roundtrips_raw() {
    let (store, _) = common::tmp_path("roundtrip");
    let raw = r#"{"time":1764163200.5,"entity":"C:\\proj\\src\\main.rs","type":"file","user_agent":"wakatime/1.0"}"#;
    let id = heartbeats::insert(&store, raw, AuthMethod::Basic, "key-1").unwrap();
    assert!(id > 0);
    let rows = heartbeats::unforwarded(&store, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].raw, raw); // byte-identical
    // provenance column still written (SEC-001); read directly:
    let am: String = store.conn.query_row("SELECT auth_method FROM heartbeats WHERE id = ?", [id], |r| r.get(0)).unwrap();
    assert_eq!(am, "basic");
    assert_eq!(rows[0].id, id);
}

// Current behaviour on purpose: no insert-side dedupe (contract §7). The
// dedupe that keeps retried rows from inflating hours lives in derive (FIX-003).
#[test]
fn no_dedupe_two_identical_inserts() {
    let (store, _) = common::tmp_path("nodedupe");
    let raw = r#"{"time":1,"entity":"a","type":"file","user_agent":"x"}"#;
    heartbeats::insert(&store, raw, AuthMethod::Basic, "k").unwrap();
    heartbeats::insert(&store, raw, AuthMethod::Basic, "k").unwrap();
    assert_eq!(heartbeats::unforwarded(&store, 10).unwrap().len(), 2);
}

#[test]
fn mark_forwarded_removes_from_queue() {
    let (store, _) = common::tmp_path("mark");
    let id = heartbeats::insert(&store, "{}", AuthMethod::Query, "k").unwrap();
    assert_eq!(heartbeats::unforwarded(&store, 10).unwrap().len(), 1);
    heartbeats::mark_forwarded(&store, id).unwrap();
    assert_eq!(heartbeats::unforwarded(&store, 10).unwrap().len(), 0);
}

// ---- agent_heartbeats ----

#[test]
fn insert_composes_sanitized_raw() {
    let store = common::tmp("compose");
    let id = agent_heartbeats::insert(&store, &common::test_ev("abc", 1_764_163_200), agent_heartbeats::HOOK_SOURCE).unwrap();
    let raw: String = store
        .conn
        .query_row("SELECT raw FROM agent_heartbeats WHERE id = ?", [id], |r| r.get(0))
        .unwrap();

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["event_id"], serde_json::json!("abc"));
    assert_eq!(v["tool_name"], serde_json::json!("Edit"));
    assert_eq!(v["ai_input_tokens"], serde_json::json!(10));
    // hook_source column asserted end-to-end in hooks_claude's endpoint test
}

#[test]
fn insert_idempotent_on_event_id() {
    let store = common::tmp("idem");
    let a = agent_heartbeats::insert(&store, &common::test_ev("abc", 1_764_163_200), agent_heartbeats::HOOK_SOURCE).unwrap();
    let b = agent_heartbeats::insert(&store, &common::test_ev("abc", 1_764_163_200), agent_heartbeats::HOOK_SOURCE).unwrap();
    assert_eq!(a, b);
    let n: i64 = store.conn.query_row("SELECT count(*) FROM agent_heartbeats", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    // a different event still inserts
    let c = agent_heartbeats::insert(&store, &common::test_ev("def", 1_764_163_200), agent_heartbeats::HOOK_SOURCE).unwrap();
    assert_ne!(a, c);
}
