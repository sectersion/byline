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
        (None, Some(p)) => (Some(p.to_string()), std::path::Path::new(p).parent().map(|d| d.to_string_lossy().into_owned())),
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
    use crate::store::{agent_heartbeats, heartbeats, AuthMethod, Store};

    fn tmp_store(name: &str) -> Store {
        let mut dir = std::env::temp_dir();
        dir.push(format!("byline-compose-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("byline.db");
        std::fs::remove_file(&path).ok();
        Store::open(&path).unwrap()
    }

    fn stored_raw(store: &Store) -> (String, i64) {
        let ev = agent_heartbeats::HookEvent {
            event_id: "e1".into(),
            timestamp: Some(1_764_163_200),
            tool_name: "Edit".into(),
            file_path: Some("C:\\p\\a.rs".into()),
            ai_model: Some("claude-sonnet-4-5".into()),
            ai_input_tokens: Some(1234),
            ai_output_tokens: Some(567),
            ai_line_changes: Some(3),
        };
        let id = agent_heartbeats::insert(store, &ev, HOOK_SOURCE).unwrap();
        store
            .conn
            .query_row("SELECT raw, received_at FROM agent_heartbeats WHERE id = ?", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
    }

    // (a) category/time/user_agent/ai_* and no prompt contents
    #[test]
    fn composed_shape() {
        let store = tmp_store("shape");
        let (raw, received_at) = stored_raw(&store);
        let hb: serde_json::Value = serde_json::from_str(&to_heartbeat(&store, &raw, received_at).unwrap()).unwrap();
        assert_eq!(hb["category"], serde_json::json!("ai coding"));
        assert!(hb["time"].as_f64().is_some(), "time is a float");
        assert_eq!(hb["time"].as_f64(), Some(1_764_163_200.0));
        assert!(hb["user_agent"].as_str().unwrap().starts_with("byline/"));
        assert!(hb["user_agent"].as_str().unwrap().contains("(claude-code)"));
        assert_eq!(hb["ai_model"], serde_json::json!("claude-sonnet-4-5"));
        assert_eq!(hb["ai_input_tokens"], serde_json::json!(1234));
        assert_eq!(hb["ai_output_tokens"], serde_json::json!(567));
        assert_eq!(hb["ai_line_changes"], serde_json::json!(3));
        // no prompt/contents: the shape only carries names and counts
        let s = serde_json::to_string(&hb).unwrap();
        assert!(!s.contains("old_string") && !s.contains("SECRET"));
    }

    // (b) editor heartbeat within ±30 min supplies entity/project
    #[test]
    fn matching_editor_supplies_entity_project() {
        let store = tmp_store("editor");
        let (raw, received_at) = stored_raw(&store);
        let id = heartbeats::insert(
            &store,
            r#"{"time":1764163300.0,"entity":"C:\\p\\b.rs","type":"file","project":"p","user_agent":"wakatime/16"}"#,
            AuthMethod::Basic,
            "k",
        )
        .unwrap();
        // insert stamps 'now'; align the row to the event's own clock
        store.conn.execute("UPDATE heartbeats SET received_at = 1764163300 WHERE id = ?", [id]).unwrap();
        let hb: serde_json::Value =
            serde_json::from_str(&to_heartbeat(&store, &raw, received_at).unwrap()).unwrap();
        assert_eq!(hb["entity"], serde_json::json!("C:\\p\\b.rs"));
        assert_eq!(hb["project"], serde_json::json!("p"));
    }

    // (b2) no matching editor heartbeat -> file path entity, parent dir project
    #[test]
    fn no_editor_falls_back_to_file_path() {
        let store = tmp_store("fallback");
        let (raw, received_at) = stored_raw(&store);
        let hb: serde_json::Value =
            serde_json::from_str(&to_heartbeat(&store, &raw, received_at).unwrap()).unwrap();
        assert_eq!(hb["entity"], serde_json::json!("C:\\p\\a.rs")); assert_eq!(hb["project"], serde_json::json!("C:\\p"));
    }

    // (b3) editor heartbeat outside the ±30 min window does not match
    #[test]
    fn stale_editor_ignored() {
        let store = tmp_store("stale");
        let (raw, received_at) = stored_raw(&store);
        heartbeats::insert(
            &store,
            r#"{"time":1764163200.0,"entity":"C:\\other\\z.rs","type":"file","project":"other","user_agent":"w"}"#,
            AuthMethod::Basic,
            "k",
        )
        .unwrap();
        // push the editor heartbeat's received_at 1 hour before the event
        store
            .conn
            .execute("UPDATE heartbeats SET received_at = received_at - 3600", [])
            .unwrap();
        let hb: serde_json::Value =
            serde_json::from_str(&to_heartbeat(&store, &raw, received_at).unwrap()).unwrap();
        assert_eq!(hb["project"], serde_json::json!("C:\\p"));
    }

    // (b4) rider: editor match with an empty project falls through to the
    // file-path/parent-dir fallback instead of sending project: ""
    #[test]
    fn editor_without_project_falls_back_to_file_path() {
        let store = tmp_store("emptyproj");
        let (raw, received_at) = stored_raw(&store);
        heartbeats::insert(&store, r#"{"time":1764163200.0,"entity":"C:\\other\\z.rs","type":"file","project":"","user_agent":"w"}"#, AuthMethod::Basic, "k").unwrap();
        store.conn.execute("UPDATE heartbeats SET received_at = 1764163200", []).unwrap();
        let hb: serde_json::Value = serde_json::from_str(&to_heartbeat(&store, &raw, received_at).unwrap()).unwrap();
        assert_eq!(hb["entity"], serde_json::json!("C:\\p\\a.rs"));
        assert_eq!(hb["project"], serde_json::json!("C:\\p"));
    }
}
