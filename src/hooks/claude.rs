// claude-code hook events -> HookEvent (parse) -> agent_heartbeats (store)
//
// Privacy: tool_input / tool_response are dropped at parse time. Only the
// path, tool name, ai_* counts/names and event_id ever reach storage — no
// prompt text, file contents, or keystrokes.

use crate::store::agent_heartbeats::{HookEvent, HOOK_SOURCE};
use crate::store::Store;

/// FNV-1a-64 of the raw payload: derives a stable event id when the hook
/// script did not supply one. No dependency, stable across restarts.
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

/// The endpoint body for POST /api/v1/hooks/claude. Always answers
/// 202 {"ok":true} — a hook failure must never block the agent. Errors are
/// counted in the `hook_errors` config counter.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{agent_heartbeats, Store};
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, Mutex};

    const FIXTURE: &str = include_str!("../../docs/fixtures/claude-code/post-tool-use.json");
    const SECRET: &str = "SECRET-OLD-STRING-CONTENTS";

    /// Fixture payload with a secret planted in tool_input and byline's
    /// optional ai_* extension fields added, as the hook script would.
    fn enriched() -> String {
        let mut v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        v["tool_input"]["old_string"] = serde_json::json!(SECRET);
        v["ai_model"] = serde_json::json!("claude-sonnet-4-5");
        v["ai_input_tokens"] = serde_json::json!(1234);
        v["ai_output_tokens"] = serde_json::json!(567);
        v["ai_line_changes"] = serde_json::json!(3);
        v["timestamp"] = serde_json::json!(1_764_163_200);
        v.to_string()
    }

    fn tmp_store(name: &str) -> (Arc<Mutex<Store>>, std::path::PathBuf) {
        let mut dir = std::env::temp_dir();
        dir.push(format!("byline-hooks-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("byline.db");
        std::fs::remove_file(&db).ok();
        (Arc::new(Mutex::new(Store::open(&db).unwrap())), db)
    }

    fn start(port: u16, store: Arc<Mutex<Store>>) {
        std::thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            let server = tiny_http::Server::http(&addr).unwrap();
            loop {
                let req = server.recv().expect("recv");
                crate::ingest::server::handle(req, &store, &None);
            }
        });
        for _ in 0..50 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("server on {port} never came up");
    }

    fn send(port: u16, raw: &str) -> (u16, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        s.shutdown(std::net::Shutdown::Write).unwrap();
        let mut buf = String::new();
        s.read_to_string(&mut buf).unwrap();
        let status: u16 = buf
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let body = buf.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    fn post(port: u16, body: &str) -> (u16, String) {
        let raw = format!(
            "POST /api/v1/hooks/claude HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        send(port, &raw)
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

    // (a) parse extracts counts/model, drops prompt text
    #[test]
    fn parse_extracts_counts_and_model_drops_contents() {
        let ev = parse(&enriched()).unwrap();
        assert_eq!(ev.tool_name, "Edit");
        assert_eq!(ev.file_path.as_deref(), Some("/home/u/byline/src/main.rs"));
        assert_eq!(ev.ai_model.as_deref(), Some("claude-sonnet-4-5"));
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
        assert_eq!(ev.file_path.as_deref(), Some("/home/u/byline/src/main.rs"));
        // event_id derived from content when the hook script sends none
        assert_eq!(ev.event_id.len(), 16, "derived fnv id is 16 hex chars");
    }

    #[test]
    fn parse_missing_tool_name_is_err() {
        let mut v: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        v.as_object_mut().unwrap().remove("tool_name");
        assert!(parse(&v.to_string()).is_err());
        assert!(parse("not{!").is_err());
    }

    #[test]
    fn stored_row_never_contains_contents() {
        let (store, db) = tmp_store("secret");
        let ev = parse(&enriched()).unwrap();
        agent_heartbeats::insert(&store.lock().unwrap(), &ev, "claude-code").unwrap();
        let rows = all_rows(&store.lock().unwrap());
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].1.contains(SECRET), "secret reached a stored row");
        assert!(rows[0].1.contains("claude-sonnet-4-5"));
        let _ = db;
    }

    // (b) duplicate event_id inserted twice -> one row
    #[test]
    fn duplicate_event_id_is_one_row() {
        let (store, _) = tmp_store("dup");
        let s = store.lock().unwrap();
        let ev = parse(&enriched()).unwrap();
        let a = agent_heartbeats::insert(&s, &ev, "claude-code").unwrap();
        let b = agent_heartbeats::insert(&s, &ev, "claude-code").unwrap();
        assert_eq!(a, b, "second insert is a no-op returning the existing id");
        assert_eq!(all_rows(&s).len(), 1);
    }

    // (c) malformed event -> 202 + hook_errors counter, never a crash
    #[test]
    fn malformed_event_202_and_counted() {
        let (store, db) = tmp_store("malformed");
        start(19881, store.clone());
        let (code, resp) = post(19881, "not{!");
        assert_eq!(code, 202, "hook failures never block the agent");
        assert_eq!(resp, r#"{"ok":true}"#);
        let s = Store::open(&db).unwrap();
        assert_eq!(hook_errors(&s), 1, "malformed event counted");
        assert_eq!(all_rows(&s).len(), 0);
        // and a good event right after still works
        let (code, _) = post(19881, &enriched());
        assert_eq!(code, 202);
        let s = Store::open(&db).unwrap();
        assert_eq!(all_rows(&s).len(), 1);
        assert_eq!(hook_errors(&s), 1);
    }

    #[test]
    fn endpoint_stores_and_acks_202() {
        let (store, db) = tmp_store("e2e");
        start(19882, store.clone());
        let (code, resp) = post(19882, &enriched());
        assert_eq!(code, 202);
        assert_eq!(resp, r#"{"ok":true}"#);
        let s = Store::open(&db).unwrap();
        let rows = all_rows(&s);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.contains(r#""hook_source":"claude-code""#) || rows[0].1.contains("Edit"));
        assert_eq!(hook_errors(&s), 0);
    }
}
