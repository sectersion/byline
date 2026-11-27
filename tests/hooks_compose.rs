mod common;
// hooks::compose — agent row -> WakaTime heartbeat — extracted from src (CHORE-004)

use byline::hooks::compose::{parent_dir, to_heartbeat, CATEGORY};
use byline::store::{agent_heartbeats, heartbeats, AuthMethod};

/// Insert a stored agent row and hand back its raw + received_at.
fn stored_raw(store: &byline::store::Store, ev: &agent_heartbeats::HookEvent) -> (String, i64) {
    let id = agent_heartbeats::insert(store, ev, agent_heartbeats::HOOK_SOURCE).unwrap();
    store
        .conn
        .query_row("SELECT raw, received_at FROM agent_heartbeats WHERE id = ?", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
}

/// Store an editor heartbeat and pin its received_at (insert stamps 'now').
fn put_editor(store: &byline::store::Store, raw: &str, received_at: i64) {
    let id = heartbeats::insert(store, raw, AuthMethod::Basic, "k").unwrap();
    store.conn.execute("UPDATE heartbeats SET received_at = ? WHERE id = ?", rusqlite::params![received_at, id]).unwrap();
}

fn composed(store: &byline::store::Store, raw: &str, received_at: i64) -> serde_json::Value {
    serde_json::from_str(&to_heartbeat(store, raw, received_at).unwrap()).unwrap()
}

// (a) category/time/user_agent/ai_* and no prompt contents
#[test]
fn composed_shape() {
    let store = common::tmp("shape");
    let (raw, received_at) = stored_raw(&store, &common::test_ev("e1", 1_764_163_200));
    let hb = composed(&store, &raw, received_at);
    assert_eq!(hb["category"], serde_json::json!(CATEGORY));
    assert_eq!(hb["time"].as_f64(), Some(1_764_163_200.0), "time is the event's own second");
    assert!(
        hb["user_agent"].as_str().unwrap().starts_with("byline/")
            && hb["user_agent"].as_str().unwrap().contains("(claude-code)")
    );
    assert_eq!(hb["ai_model"], serde_json::json!("claude-sonnet-4-5"));
    assert_eq!(hb["ai_input_tokens"], serde_json::json!(10));
    assert_eq!(hb["ai_output_tokens"], serde_json::json!(20));
    assert_eq!(hb["ai_line_changes"], serde_json::json!(3));
    // no prompt/contents: the shape only carries names and counts
    let s = serde_json::to_string(&hb).unwrap();
    assert!(!s.contains("old_string") && !s.contains("SECRET"));
}

// (b) editor heartbeat within ±30 min supplies entity/project
#[test]
fn matching_editor_supplies_entity_project() {
    let store = common::tmp("editor");
    let (raw, received_at) = stored_raw(&store, &common::test_ev("e1", 1_764_163_200));
    put_editor(&store, r#"{"time":1764163300.0,"entity":"C:\\p\\b.rs","type":"file","project":"p","user_agent":"wakatime/16"}"#, 1_764_163_300);
    let hb = composed(&store, &raw, received_at);
    assert_eq!(hb["entity"], serde_json::json!("C:\\p\\b.rs"));
    assert_eq!(hb["project"], serde_json::json!("p"));
}

// (b2)/(b2b): no matching editor heartbeat -> file path entity, parent
// dir project — Windows and POSIX separators must behave the same.
#[test]
fn no_editor_falls_back_to_file_path() {
    let store = common::tmp("fallback");
    let (raw, received_at) = stored_raw(&store, &common::test_ev("e1", 1_764_163_200));
    let hb = composed(&store, &raw, received_at);
    assert_eq!(hb["entity"], serde_json::json!("C:\\p\\a.rs"));
    assert_eq!(hb["project"], serde_json::json!("C:\\p"));
    // POSIX-style path: same fallback, separators must not matter
    let mut posix = common::test_ev("e2", 1_764_163_200);
    posix.file_path = Some("/home/u/byline/src/main.rs".into());
    let (raw, received_at) = stored_raw(&store, &posix);
    let hb = composed(&store, &raw, received_at);
    assert_eq!(hb["entity"], serde_json::json!("/home/u/byline/src/main.rs"));
    assert_eq!(hb["project"], serde_json::json!("/home/u/byline/src"));
}

#[test]
fn parent_dir_root_and_bare() {
    assert_eq!(parent_dir("C:\\a.rs"), Some("C:".to_string()));
    assert_eq!(parent_dir("/a.rs"), Some("/".to_string()));
    assert_eq!(parent_dir("a.rs"), None);
    assert_eq!(parent_dir("p/a.rs"), Some("p".to_string()));
}

// (b3) editor heartbeat outside the ±30 min window does not match
#[test]
fn stale_editor_ignored() {
    let store = common::tmp("stale");
    let (raw, received_at) = stored_raw(&store, &common::test_ev("e1", 1_764_163_200));
    put_editor(&store, r#"{"time":1764163200.0,"entity":"C:\\other\\z.rs","type":"file","project":"other","user_agent":"w"}"#, 1_764_159_600);
    let hb = composed(&store, &raw, received_at);
    assert_eq!(hb["project"], serde_json::json!("C:\\p"));
}

// (b4) rider: editor match with an empty project falls through to the
// file-path/parent-dir fallback instead of sending project: ""
#[test]
fn editor_without_project_falls_back_to_file_path() {
    let store = common::tmp("emptyproj");
    let (raw, received_at) = stored_raw(&store, &common::test_ev("e1", 1_764_163_200));
    put_editor(&store, r#"{"time":1764163200.0,"entity":"C:\\other\\z.rs","type":"file","project":"","user_agent":"w"}"#, 1_764_163_200);
    let hb = composed(&store, &raw, received_at);
    assert_eq!(hb["entity"], serde_json::json!("C:\\p\\a.rs"));
    assert_eq!(hb["project"], serde_json::json!("C:\\p"));
}
