// OpenCode hook events -> HookEvent (parse) -> agent_heartbeats (store)
// Privacy: the plugin whitelists fields before POST; this side drops anything
// text-shaped it does not recognise. Only path, tool name, ai_* counts/names
// and event_id ever reach storage — no prompt text, file contents, keystrokes.

use crate::store::agent_heartbeats::{self, HookEvent};
use crate::store::Store;

pub const HOOK_SOURCE: &str = "opencode";

/// FNV-1a-64 of the raw payload: stable event id when the plugin sent none.
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Parses one OpenCode hook payload into the minimal normalized shape.
/// Reads `tool` -> `tool_name`, `file_path` from top-level or the OpenCode
/// nesting (args.file_path / metadata.file.path); ai_* at top level.
/// Malformed -> Err.
pub fn parse(event_json: &str) -> Result<HookEvent, String> {
    let v: serde_json::Value =
        serde_json::from_str(event_json).map_err(|e| format!("invalid json: {e}"))?;
    let obj = v.as_object().ok_or("payload is not an object")?;
    let tool_name = obj
        .get("tool_name")
        .and_then(|x| x.as_str())
        .or_else(|| obj.get("tool").and_then(|x| x.as_str()))
        .ok_or("missing tool_name")?
        .to_string();
    let event_id = match obj.get("event_id").and_then(|x| x.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => format!("{:016x}", fnv1a64(event_json.as_bytes())),
    };
    let file_path = obj
        .get("file_path")
        .and_then(|x| x.as_str())
        .or_else(|| obj.get("args").and_then(|a| a.get("file_path")).and_then(|x| x.as_str()))
        .or_else(|| {
            obj.get("output")
                .and_then(|o| o.get("metadata"))
                .and_then(|m| m.get("file"))
                .and_then(|f| f.get("path"))
                .and_then(|x| x.as_str())
        })
        .map(str::to_string);
    let num = |k: &str| obj.get(k).and_then(|x| x.as_f64()).map(|f| f as i64);
    Ok(HookEvent {
        event_id,
        timestamp: obj.get("timestamp").and_then(|x| x.as_f64()).map(|f| f as i64),
        tool_name,
        file_path,
        ai_model: obj.get("ai_model").and_then(|x| x.as_str()).map(str::to_string),
        ai_input_tokens: num("ai_input_tokens"),
        ai_output_tokens: num("ai_output_tokens"),
        ai_line_changes: num("ai_line_changes"),
    })
}

/// The POST /api/v1/hooks/opencode body. Always 202 {"ok":true} — a failing
/// hook must never block the agent; errors bump the `hook_errors` counter.
pub fn handle(body: &[u8], store: &Store) -> (u16, String) {
    let text = String::from_utf8_lossy(body);
    match parse(&text) {
        Ok(ev) => {
            if agent_heartbeats::insert(store, &ev, HOOK_SOURCE).is_err() {
                agent_heartbeats::bump_hook_errors(store);
            }
        }
        Err(_) => agent_heartbeats::bump_hook_errors(store),
    }
    (202, r#"{"ok":true}"#.to_string())
}