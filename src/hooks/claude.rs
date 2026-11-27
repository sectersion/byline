// claude-code hook events -> HookEvent (parse) -> agent_heartbeats (store)
// Privacy: tool_input / tool_response are dropped at parse time. Only the
// path, tool name, ai_* counts/names and event_id ever reach storage — no
// prompt text, file contents, or keystrokes.

use crate::store::agent_heartbeats::{HookEvent, HOOK_SOURCE};
use crate::store::Store;

/// FNV-1a-64 of the raw payload: stable event id when the hook supplied none.
fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Parses one Claude Code hook payload into the minimal normalized shape.
/// Malformed input -> Err; the endpoint still answers 202 and counts it.
pub fn parse(event_json: &str) -> Result<HookEvent, String> {
    let v: serde_json::Value =
        serde_json::from_str(event_json).map_err(|e| format!("invalid json: {e}"))?;
    let obj = v.as_object().ok_or("payload is not an object")?;
    let tool_name = obj
        .get("tool_name")
        .and_then(|x| x.as_str())
        .ok_or("missing tool_name")?
        .to_string();
    let event_id = match obj.get("event_id").and_then(|x| x.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => format!("{:016x}", fnv1a64(event_json.as_bytes())),
    };
    let file_path = obj
        .get("tool_input")
        .and_then(|t| t.get("file_path"))
        .and_then(|x| x.as_str())
        .or_else(|| obj.get("file_path").and_then(|x| x.as_str()))
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

/// The POST /api/v1/hooks/claude body. Always 202 {"ok":true} — a failing
/// hook must never block the agent; errors bump the `hook_errors` counter.
pub fn handle(body: &[u8], store: &Store) -> (u16, String) {
    let text = String::from_utf8_lossy(body);
    match parse(&text) {
        Ok(ev) => {
            if crate::store::agent_heartbeats::insert(store, &ev, HOOK_SOURCE).is_err() {
                crate::store::agent_heartbeats::bump_hook_errors(store);
            }
        }
        Err(_) => crate::store::agent_heartbeats::bump_hook_errors(store),
    }
    // never an error status: a failing hook must not block the agent
    (202, r#"{"ok":true}"#.to_string())
}

