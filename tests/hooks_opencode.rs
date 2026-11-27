mod common;
// hooks::opencode — hook parse + endpoint — FEAT-008

use byline::hooks::opencode::parse;
use byline::store::agent_heartbeats;
use byline::store::Store;

const FIXTURE: &str = include_str!("../docs/fixtures/opencode/tool-execute-after.json");
const SECRET: &str = "SECRET-OLD-STRING-CONTENTS";

/// Fixture payload with a secret planted in args, ai_* fields added.
fn enriched() -> String {
    let mut v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    v["args"]["old_string"] = serde_json::json!(SECRET);
    v["ai_model"] = serde_json::json!("openai/gpt-4o");
    v["ai_input_tokens"] = serde_json::json!(1234);
    v["ai_output_tokens"] = serde_json::json!(567);
    v["ai_line_changes"] = serde_json::json!(3);
    v["timestamp"] = serde_json::json!(1_764_163_200);
    v.to_string()
}

fn post(port: u16, body: &str) -> (u16, String) {
    let raw = format!(
        "POST /api/v1/hooks/opencode HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    common::send(port, &raw)
}

fn hook_errors(store: &Store) -> i64 {
    store
        .conn
        .query_row("SELECT CAST(value AS INTEGER) FROM config WHERE key = 'hook_errors'", [], |r| r.get(0))
        .unwrap_or(0)
}

fn all_rows(store: &Store) -> Vec<(i64, String)> {
    let mut stmt = store.conn.prepare("SELECT id, raw FROM agent_heartbeats ORDER BY id").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|x| x.unwrap()).collect()
}

// (a) parse extracts counts/model, drops contents
#[test]
fn parse_extracts_counts_and_model_drops_contents() {
    let ev = parse(&enriched()).unwrap();
    assert_eq!(ev.tool_name, "edit");
    assert_eq!(ev.file_path.as_deref(), Some("/home/u/byline/src/main.rs"));
    assert_eq!(ev.ai_model.as_deref(), Some("openai/gpt-4o"));
    assert_eq!(ev.ai_input_tokens, Some(1234));
    assert_eq!(ev.ai_output_tokens, Some(567));
    assert_eq!(ev.ai_line_changes, Some(3));
    assert_eq!(ev.timestamp, Some(1_764_163_200));
    // the normalized shape holds no text field: the secret is gone
    assert!(!format!("{ev:?}").contains(SECRET), "secret leaked into HookEvent");
}

#[test]
fn parse_absent_optionals_are_none() {
    let ev = parse(FIXTURE).unwrap();
    assert_eq!(ev.ai_model, None);
    assert_eq!(ev.ai_input_tokens, None);
    // event_id derived from content when the plugin sends none
    assert_eq!(ev.event_id.len(), 16, "derived fnv id is 16 hex chars");
}

#[test]
fn parse_missing_tool_name_is_err() {
    let mut v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    v.as_object_mut().unwrap().remove("tool");
    assert!(parse(&v.to_string()).is_err());
    assert!(parse("not{!").is_err());
}

// (d) privacy: the secret lives in args; the stored row must not.
#[test]
fn stored_row_never_contains_contents() {
    let (store, _db) = common::test_store("secret");
    let ev = parse(&enriched()).unwrap();
    agent_heartbeats::insert(&store.lock().unwrap(), &ev, "opencode").unwrap();
    let rows = all_rows(&store.lock().unwrap());
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].1.contains(SECRET), "secret reached a stored row");
    assert!(rows[0].1.contains("openai/gpt-4o"));
}

// (c) malformed event -> 202 + hook_errors counter, never a crash
#[test]
fn malformed_event_202_and_counted() {
    let (store, db) = common::test_store("malformed");
    common::start(19882, store.clone(), None);
    let (code, resp) = post(19882, "not{!");
    assert_eq!(code, 202, "hook failures never block the agent");
    assert_eq!(resp, r#"{"ok":true}"#);
    let s = Store::open(&db).unwrap();
    assert_eq!(hook_errors(&s), 1, "malformed event counted");

    assert_eq!(all_rows(&s).len(), 0);
    // a good event right after still stores with hook_source in its column
    let (code, _) = post(19882, &enriched());
    assert_eq!(code, 202);
    let s = Store::open(&db).unwrap();
    let rows = all_rows(&s);
    assert_eq!(rows.len(), 1);
    let src: String = s
        .conn
        .query_row("SELECT hook_source FROM agent_heartbeats WHERE id = ?", [rows[0].0], |r| r.get(0))
        .unwrap();
    assert_eq!(src, "opencode");
    assert_eq!(hook_errors(&s), 1, "the good event must not bump the counter");
}