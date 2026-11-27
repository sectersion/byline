// compose an agent_heartbeats row into a WakaTime heartbeat JSON
//
// Built only from the normalized fields Task 1 stored — never the hook
// payload. category "ai coding"; entity/project come from a matching editor
// heartbeat within ±30 min, else the hook's file path (parent dir as project).

use crate::store::agent_heartbeats::HOOK_SOURCE;
use serde_json::json;

pub const CATEGORY: &str = "ai coding";

/// The ±30 min window (seconds) for matching an editor heartbeat.
const MATCH_WINDOW: i64 = 1800;

/// Hook events older than this (seconds) are stale replays: dropped at drain
/// time — an old replay must not create fake activity.
pub const STALE_SECS: i64 = 600;

pub fn is_stale(received_at: i64) -> bool { now_secs() - received_at > STALE_SECS }

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

/// Composes the WakaTime heartbeat JSON for one stored agent row.
/// `raw`/`received_at` come straight from the agent_heartbeats row.
pub fn to_heartbeat(store: &crate::store::Store, raw: &str, received_at: i64) -> Result<String, String> {
    let ev: serde_json::Value = serde_json::from_str(raw).map_err(|e| format!("bad agent row: {e}"))?;
    let file_path = ev.get("file_path").and_then(|x| x.as_str());
    // editor match wins unless it carries no project (empty string is not a
    // project); then the file path's parent dir is the project.
    let matched = match_editor(store, received_at).filter(|(_, p)| !p.is_empty());
    let (entity, project) = match (matched, file_path) {
        (Some((entity, project)), _) => (Some(entity), Some(project)),
        (None, Some(p)) => (Some(p.to_string()), parent_dir(p)),
        (None, None) => (None, None),
    };
    let mut hb = serde_json::Map::new();
    hb.insert("time".into(), json!(received_at as f64));
    if let Some(e) = entity {
        hb.insert("entity".into(), json!(e));
    }
    hb.insert("type".into(), json!(if file_path.is_some() { "file" } else { "app" }));
    hb.insert("category".into(), json!(CATEGORY));
    hb.insert(
        "user_agent".into(),
        json!(format!("byline/{} ({})", env!("CARGO_PKG_VERSION"), HOOK_SOURCE)),
    );
    if let Some(p) = project {
        hb.insert("project".into(), json!(p));
    }
    for k in [
        "ai_model",
        "ai_input_tokens",
        "ai_output_tokens",
        "ai_line_changes",
    ] {
        if let Some(v) = ev.get(k).filter(|x| !x.is_null()) {
            hb.insert(k.into(), v.clone());
        }
    }
    serde_json::to_string(&serde_json::Value::Object(hb)).map_err(|e| e.to_string())
}

/// Parent directory of a path, separator-agnostic: hook paths may be Windows
/// ("C:\\p\\a.rs") or POSIX ("/home/u/p/a.rs"), whichever OS byline runs on.
/// Returns None when the path has no directory component.
fn parent_dir(p: &str) -> Option<String> {
    let trimmed = p.trim_end_matches(['/', '\\']);
    let cut = trimmed.rfind(['/', '\\'])?;
    if cut == 0 {
        // "/a.rs": the root itself is the parent
        return p.starts_with('/').then(|| "/".to_string());
    }
    let dir = &trimmed[..cut];
    (!dir.is_empty()).then(|| dir.to_string())
}

/// Nearest editor heartbeat within the window: (entity, project) from its raw.
fn match_editor(store: &crate::store::Store, received_at: i64) -> Option<(String, String)> {
    let raw: String = store
        .conn
        .query_row(
            "SELECT raw FROM heartbeats
             WHERE received_at BETWEEN ? AND ?
             ORDER BY abs(received_at - ?), id LIMIT 1",
            rusqlite::params![received_at - MATCH_WINDOW, received_at + MATCH_WINDOW, received_at],
            |r| r.get(0),
        )
        .ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some((
        v.get("entity")?.as_str()?.to_string(),
        v.get("project").and_then(|p| p.as_str()).unwrap_or("").to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{agent_heartbeats, heartbeats, testutil, AuthMethod, Store};


    /// Insert a stored agent row and hand back its raw + received_at.
    fn stored_raw(store: &Store, ev: &agent_heartbeats::HookEvent) -> (String, i64) {
        let id = agent_heartbeats::insert(store, ev, HOOK_SOURCE).unwrap();
        store
            .conn
            .query_row("SELECT raw, received_at FROM agent_heartbeats WHERE id = ?", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()

    }

    /// Store an editor heartbeat and pin its received_at (insert stamps 'now').
    fn put_editor(store: &Store, raw: &str, received_at: i64) {
        let id = heartbeats::insert(store, raw, AuthMethod::Basic, "k").unwrap();
        store.conn.execute("UPDATE heartbeats SET received_at = ? WHERE id = ?", rusqlite::params![received_at, id]).unwrap();
    }

    fn composed(store: &Store, raw: &str, received_at: i64) -> serde_json::Value {
        serde_json::from_str(&to_heartbeat(store, raw, received_at).unwrap()).unwrap()
    }

    // (a) category/time/user_agent/ai_* and no prompt contents
    #[test]
    fn composed_shape() {
        let store = testutil::tmp("shape");
        let (raw, received_at) = stored_raw(&store, &agent_heartbeats::test_ev("e1", 1_764_163_200));
        let hb = composed(&store, &raw, received_at);
        assert_eq!(hb["category"], serde_json::json!("ai coding"));
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
        let store = testutil::tmp("editor");
        let (raw, received_at) = stored_raw(&store, &agent_heartbeats::test_ev("e1", 1_764_163_200));
        put_editor(&store, r#"{"time":1764163300.0,"entity":"C:\\p\\b.rs","type":"file","project":"p","user_agent":"wakatime/16"}"#, 1_764_163_300);
        let hb = composed(&store, &raw, received_at);
        assert_eq!(hb["entity"], serde_json::json!("C:\\p\\b.rs"));
        assert_eq!(hb["project"], serde_json::json!("p"));
    }

    // (b2)/(b2b): no matching editor heartbeat -> file path entity, parent
    // dir project — Windows and POSIX separators must behave the same.
    #[test]
    fn no_editor_falls_back_to_file_path() {
        let store = testutil::tmp("fallback");
        let (raw, received_at) = stored_raw(&store, &agent_heartbeats::test_ev("e1", 1_764_163_200));
        let hb = composed(&store, &raw, received_at);
        assert_eq!(hb["entity"], serde_json::json!("C:\\p\\a.rs"));
        assert_eq!(hb["project"], serde_json::json!("C:\\p"));
        // POSIX-style path: same fallback, separators must not matter
        let mut posix = agent_heartbeats::test_ev("e2", 1_764_163_200);
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

    // (b2b) same fallback for a POSIX-style path: separators must not matter
    #[test]
    fn posix_path_parent_dir() {
        let store = db("posix");
        let ev = agent_heartbeats::HookEvent {
            event_id: "e2".into(),
            timestamp: Some(1_764_163_200),
            tool_name: "Edit".into(),
            file_path: Some("/home/u/byline/src/main.rs".into()),
            ai_model: Some("claude-sonnet-4-5".into()),
            ai_input_tokens: Some(1),
            ai_output_tokens: Some(2),
            ai_line_changes: Some(3),
        };
        let id = agent_heartbeats::insert(&store, &ev, agent_heartbeats::HOOK_SOURCE).unwrap();
        let (raw, received_at): (String, i64) = store
            .conn
            .query_row("SELECT raw, received_at FROM agent_heartbeats WHERE id = ?", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        let hb: serde_json::Value =
            serde_json::from_str(&to_heartbeat(&store, &raw, received_at).unwrap()).unwrap();
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
        let store = testutil::tmp("stale");
        let (raw, received_at) = stored_raw(&store, &agent_heartbeats::test_ev("e1", 1_764_163_200));
        put_editor(&store, r#"{"time":1764163200.0,"entity":"C:\\other\\z.rs","type":"file","project":"other","user_agent":"w"}"#, 1_764_159_600);
        let hb = composed(&store, &raw, received_at);
        assert_eq!(hb["project"], serde_json::json!("C:\\p"));
    }

    // (b4) rider: editor match with an empty project falls through to the
    // file-path/parent-dir fallback instead of sending project: ""
    #[test]
    fn editor_without_project_falls_back_to_file_path() {
        let store = testutil::tmp("emptyproj");
        let (raw, received_at) = stored_raw(&store, &agent_heartbeats::test_ev("e1", 1_764_163_200));
        put_editor(&store, r#"{"time":1764163200.0,"entity":"C:\\other\\z.rs","type":"file","project":"","user_agent":"w"}"#, 1_764_163_200);
        let hb = composed(&store, &raw, received_at);
        assert_eq!(hb["entity"], serde_json::json!("C:\\p\\a.rs"));
        assert_eq!(hb["project"], serde_json::json!("C:\\p"));
    }
}
