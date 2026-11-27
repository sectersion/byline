// tiny_http listener + path dispatch (a match, no router abstraction)

use super::{handle_body, AuthResult};
use crate::store::Store;
use std::sync::{Arc, Mutex};

const BULK: &str = "/api/v1/users/current/heartbeats.bulk";
const SINGLE: &str = "/api/v1/users/current/heartbeats";
const HOOK_CLAUDE: &str = "/api/v1/hooks/claude"; // FEAT-002: agent events in
const HOOK_OPENCODE: &str = "/api/v1/hooks/opencode"; // FEAT-008: OpenCode agent events
const CURRENT: &str = "/api/v1/users/current";

pub fn run(port: u16, store: Arc<Mutex<Store>>, expected_key: Option<String>) -> ! {
    let addr = format!("127.0.0.1:{port}");
    let server = tiny_http::Server::http(&addr).expect("bind 127.0.0.1");
    // slow periodic drain (30s) so rows flush even without new requests
    {
        let store = store.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            kick(&store);
        });
    }
    loop {
        let req = server.recv().expect("recv");
        handle(req, &store, &expected_key);
    }
}

fn kick(store: &Arc<Mutex<Store>>) {
    let store = store.clone();
    std::thread::spawn(move || {
        let cfg = { let s = store.lock().unwrap(); super::forward::ForwardConfig::from_store(&s) };
        super::forward::drain(&store, cfg.clone(), 100, super::forward::Table::Heartbeats);
        super::forward::drain(&store, cfg.clone(), 100, super::forward::Table::AgentHeartbeats);
    });
}

pub fn handle(mut req: tiny_http::Request, store: &Arc<Mutex<Store>>, expected_key: &Option<String>) {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    let query = url.split_once('?').map(|(_, q)| q.to_string());
    let auth = AuthResult::parse(&req, query.as_deref());
    // HOOK_CLAUDE is exempt from the key gate: localhost-only bind, the hook
    // payload carries no credential-protectable content, and hook POSTs are
    // 202-always. SEC-001's gate stays on the WakaTime endpoints.
    // Only the user's own key (read from ~/.wakatime.cfg at startup) may
    // write heartbeats. None expected -> accept-any fallback, never brick.
    let gated = if path == HOOK_CLAUDE || path == HOOK_OPENCODE { &None } else { expected_key };
    if let Some(expected) = gated {
        if auth.key() != Some(expected) {
            // refuse without reading the body or storing anything
            respond(req, 401, r#"{"error":"invalid api_key"}"#.to_string());
            return;
        }
    }
    let body = {
        let mut b = Vec::new();
        let _ = std::io::Read::read_to_end(req.as_reader(), &mut b);
        b
    };

    let method = req.method().as_str();
    let owned = (method == "POST" && (path == BULK || path == SINGLE || path == HOOK_CLAUDE || path == HOOK_OPENCODE))
        || (method == "GET" && path == CURRENT);

    let (code, resp) = if owned {
        let (code, resp) = {
            let s = store.lock().expect("lock");
            match (method, path.as_str()) {
                ("POST", p) if p == BULK || p == SINGLE => match handle_body(&body, &s, auth) {
                    super::IngestOutcome::Stored(_n) => {
                        drop(s);
                        kick(store);
                        // Echo back the actual heartbeat objects posted, per the WakaTime contract:
                        //   {"responses": [[heartbeatAttrs, 201], ...]}
                        let responses: Vec<serde_json::Value> = if p == BULK {
                            let items: Vec<serde_json::Value> =
                                serde_json::from_slice(&body).expect("bulk body JSON array");
                            items.iter().map(|v| serde_json::json!([v, 201])).collect()
                        } else {
                            // single heartbeat endpoint: echo back the one posted object
                            let item: serde_json::Value =
                                serde_json::from_slice(&body).expect("single heartbeat JSON object");
                            vec![serde_json::json!([item, 201])]
                        };
                        let responses = serde_json::to_string(&responses).expect("serialize ack");
                        (201, format!(r#"{{"responses":{responses}}}"#))
                    }
                    super::IngestOutcome::BadRequest(msg) => {
                        write_debug(&body);
                        (400, format!(r#"{{"error":"malformed body: {msg}"}}"#))
                    }
                },
                ("POST", p) if p == HOOK_CLAUDE => crate::hooks::claude::handle(&body, &s),
                ("POST", p) if p == HOOK_OPENCODE => crate::hooks::opencode::handle(&body, &s),
                ("GET", _) => (200, r#"{"data":{"username":"byline-local"}}"#.to_string()),
                _ => unreachable!("owned implies heartbeat POST or GET current"),
            }
        };
        (code, resp)
    } else {
        // FIX-004: not ours -> proxy upstream verbatim, status passthrough.
        // Still recorded as a diagnostic row. No upstream configured (local
        // only) or upstream down -> 404 as before; never blocks the store.
        {
            let s = store.lock().expect("lock");
            let _ = s.conn.execute(
                "INSERT INTO unknown_paths (path, method, hit_at) VALUES (?, ?, strftime('%s','now'))",
                rusqlite::params![path, method],
            );
        }
        let cfg = super::forward::ForwardConfig::from_store(&store.lock().unwrap());
        match cfg {
            Some(cfg) => match super::forward::proxy(&cfg, method, &url, &body) {
                Ok((up_code, text)) => (up_code, text),
                Err(_) => (404, r#"{"error":"not found"}"#.to_string()),
            },
            None => (404, r#"{"error":"not found"}"#.to_string()),
        }
    };

    respond(req, code, resp);
}

fn respond(req: tiny_http::Request, code: u16, resp: String) {
    let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let mut r = tiny_http::Response::from_string(resp).with_status_code(code);
    r.add_header(header);
    let _ = req.respond(r);
}

fn write_debug(body: &[u8]) {
    let mut dir = crate::cli::serve::home_dir();
    dir.push(".byline/debug");
    std::fs::create_dir_all(&dir).ok();
    dir.push("last-bad-body.json");
    let _ = std::fs::write(dir, body);
}

// AuthResult needs header access from the request; implemented here to keep
// auth.rs free of tiny_http types.
impl AuthResult {
    pub fn parse(req: &tiny_http::Request, query: Option<&str>) -> AuthResult {
        let header = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.as_str().to_string());
        super::auth::parse(header.as_deref(), query)
    }
}

