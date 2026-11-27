// transparent drain: post unforwarded rows to their original api_url with
// their original key; 2xx -> forwarded_at; backoff 2^n capped 3600s in config.

use crate::store::{heartbeats, Store};
use std::sync::{Arc, Mutex};

/// Which queue to drain. Agent rows are composed (hooks::compose) before send.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Table {
    Heartbeats,
    AgentHeartbeats,
}

#[derive(Clone)]
pub struct ForwardConfig {
    pub api_url: String, // full base incl. version prefix, e.g. https://hackatime.../api/hackatime/v1
    pub api_key: String,
}

impl ForwardConfig {
    pub fn from_store(store: &Store) -> Option<ForwardConfig> {
        let get = |k: &str| -> Option<String> {
            store.conn.query_row("SELECT value FROM config WHERE key = ?", [k], |r| r.get(0)).ok()
        };
        Some(ForwardConfig { api_url: get("upstream_api_url")?, api_key: get("upstream_api_key")? })
    }
}

/// Backoff seconds after n consecutive failures: 15 * 2^n capped 3600.
pub fn backoff_secs(failures: u32) -> u64 {
    15u64.saturating_mul(1u64 << failures.min(8)).min(3600)
}

/// Drains up to `limit` unforwarded rows from `table`; returns count forwarded.
/// Config missing -> no-op. Never errors upward. Backoff state (config
/// `forward_failures`) is shared across tables.
pub fn drain(store: &Arc<Mutex<Store>>, cfg: Option<ForwardConfig>, limit: usize, table: Table) -> usize {
    let Some(cfg) = cfg else { return 0 };
    let s = store.lock().unwrap();
    match table {
        Table::Heartbeats => drain_editor(&s, &cfg, limit),
        // task 3: agent rows batch (<=100/POST, <=1 POST/30s) in agent_batch
        Table::AgentHeartbeats => crate::ingest::agent_batch::drain_agent(&s, &cfg, limit),
    }
}

/// Editor heartbeats go as stored, one row per POST, unbatched (unchanged).
fn drain_editor(s: &Store, cfg: &ForwardConfig, limit: usize) -> usize {
    let rows: Vec<(i64, String)> = heartbeats::unforwarded(s, limit)
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.id, r.raw))
        .collect();
    let mut forwarded = 0usize;
    for (id, raw) in rows {
        // bulk path appends bare paths to the base
        let url = format!("{}/users/current/heartbeats.bulk", cfg.api_url.trim_end_matches('/'));
        match post_one(&url, &cfg.api_key, &raw) {
            Ok(()) => {
                let _ = heartbeats::mark_forwarded(s, id);
                forwarded += 1;
                set_failures(s, 0);
            }
            Err(_) => {
                set_failures(s, failures(s) + 1);
                break; // backoff handled by caller pacing; stop this pass
            }
        }
    }
    forwarded
}

/// FIX-004: forward one non-heartbeat request to upstream verbatim — local
/// `/api/v1` prefix dropped, upstream key used (the caller's may differ).
/// Errors -> caller falls back to 404.
pub fn proxy(cfg: &ForwardConfig, method: &str, path_and_query: &str, body: &[u8]) -> Result<(u16, String), String> {
    let rest = path_and_query.strip_prefix("/api/v1").unwrap_or(path_and_query);
    // FIX-009: query-param auth (Basic is rejected by Hackatime); append after
    // any existing query string the caller's path may carry.
    let sep = if rest.contains('?') { "&" } else { "?" };
    let url = format!("{}{}{}api_key={}", cfg.api_url.trim_end_matches('/'), rest, sep, &cfg.api_key);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(5)))
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into();
    // ureq 3 only has named-method builders; WakaTime endpoints are GET/POST.
    let result = match method {
        "GET" => agent.get(&url).call(),
        "POST" => agent.post(&url).send(body),
        "DELETE" => agent.delete(&url).call(),
        "HEAD" => agent.head(&url).call(),
        _ => return Err(format!("unsupported method {method}")),
    };
    let mut resp = result.map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    Ok((status, text))
}

pub(crate) fn failures(s: &Store) -> u32 {
    s.conn
        .query_row("SELECT CAST(value AS INTEGER) FROM config WHERE key = 'forward_failures'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

pub(crate) fn set_failures(s: &Store, n: u32) {
    let _ = s.conn.execute(
        "INSERT INTO config (key, value) VALUES ('forward_failures', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [n.to_string()],
    );
}

pub(crate) fn post_one(url: &str, key: &str, body: &str) -> Result<(), String> {
    // one POST to the bulk endpoint; the body goes out as one JSON array.
    // FIX-009: query-param auth — Hackatime rejects this key as a Basic header.
    // FIX-010: Content-Type json — octet-stream (ureq's default for send_bytes)
    // is a 400 on Hackatime.
    let payload = format!("[{body}]");
    let req = ureq::post(&format!("{url}?api_key={key}")).header("Content-Type", "application/json");
    let mut resp = req.send(payload.as_bytes()).map_err(|e| e.to_string())?;
    if resp.status().as_u16() < 300 {
        let _ = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err(format!("upstream status {}", resp.status()))
    }
}


