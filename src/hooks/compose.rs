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
        json!(format!(
            "byline/{} ({})",
            env!("CARGO_PKG_VERSION"),
            ev.get("hook_source").and_then(|x| x.as_str()).unwrap_or(HOOK_SOURCE)
        )),
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
pub fn parent_dir(p: &str) -> Option<String> {
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

