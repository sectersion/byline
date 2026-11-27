// insert operations over the agent_heartbeats table (FEAT-002 task 1)
//
// The row's `raw` column holds the *composed, sanitized* agent heartbeat
// (counts and names only), never the hook payload as received.

use super::Store;
use serde_json::json;

pub const HOOK_SOURCE: &str = "claude-code";

/// The minimal normalized shape parsed from a hook event. Text fields beyond
/// the path and model name are dropped at parse time (privacy).
#[derive(Debug, Clone)]
pub struct HookEvent {
    pub event_id: String,      // from the hook script, or derived from content
    pub timestamp: Option<i64>, // unix seconds; None -> store's receive time
    pub tool_name: String,
    pub file_path: Option<String>,
    pub ai_model: Option<String>,
    pub ai_input_tokens: Option<i64>,
    pub ai_output_tokens: Option<i64>,
    pub ai_line_changes: Option<i64>,
}

/// Idempotent insert keyed on event_id: a repeat delivery is a no-op that
/// returns the existing row id. Enforced with a content lookup, since the
/// schema is frozen and carries no event_id column.
pub fn insert(store: &Store, event: &HookEvent, hook_source: &str) -> Result<i64, rusqlite::Error> {
    let needle = format!(r#""event_id":"{}""#, event.event_id);
    let existing: Option<i64> = store
        .conn
        .query_row(
            "SELECT id FROM agent_heartbeats WHERE instr(raw, ?) > 0 LIMIT 1",
            rusqlite::params![needle],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let received_at = event.timestamp.unwrap_or_else(now_secs);
    let raw = json!({
        "event_id": event.event_id,
        "timestamp": received_at,
        "tool_name": event.tool_name,
        "file_path": event.file_path,
        "ai_model": event.ai_model,
        "ai_input_tokens": event.ai_input_tokens,
        "ai_output_tokens": event.ai_output_tokens,
        "ai_line_changes": event.ai_line_changes,
    })
    .to_string();
    store.conn.execute(
        "INSERT INTO agent_heartbeats (raw, received_at, forwarded_at, hook_source)
         VALUES (?, ?, NULL, ?)",
        rusqlite::params![raw, received_at, hook_source],
    )?;
    Ok(store.conn.last_insert_rowid())
}

/// A queued agent row as the drain sees it.
pub struct AgentRow {
    pub id: i64,
    pub raw: String,
    pub received_at: i64,
}

pub fn unforwarded(store: &Store, limit: usize) -> Result<Vec<AgentRow>, rusqlite::Error> {
    let mut stmt = store.conn.prepare(
        "SELECT id, raw, received_at FROM agent_heartbeats
         WHERE forwarded_at IS NULL ORDER BY id LIMIT ?",
    )?;
    let rows = stmt
        .query_map([limit as i64], |r| {
            Ok(AgentRow { id: r.get(0)?, raw: r.get(1)?, received_at: r.get(2)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn mark_forwarded(store: &Store, id: i64) -> Result<(), rusqlite::Error> {
    store.conn.execute(
        "UPDATE agent_heartbeats SET forwarded_at = strftime('%s','now') WHERE id = ?",
        [id],
    )?;
    Ok(())
}

/// Malformed/failed hook events are counted here (never 4xx to the agent).
pub fn bump_hook_errors(store: &Store) {
    let _ = store.conn.execute(
        "INSERT INTO config (key, value) VALUES ('hook_errors', '1')
         ON CONFLICT(key) DO UPDATE SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT)",
        [],
    );
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

