// tiny_http listener + path dispatch (a match, no router abstraction)

use super::{handle_body, AuthResult};
use crate::store::Store;
use std::sync::{Arc, Mutex};

const BULK: &str = "/api/v1/users/current/heartbeats.bulk";
const SINGLE: &str = "/api/v1/users/current/heartbeats";
const HOOK_CLAUDE: &str = "/api/v1/hooks/claude"; // FEAT-002: agent events in
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

pub(crate) fn handle(mut req: tiny_http::Request, store: &Arc<Mutex<Store>>, expected_key: &Option<String>) {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    let query = url.split_once('?').map(|(_, q)| q.to_string());
    let auth = AuthResult::parse(&req, query.as_deref());
    // HOOK_CLAUDE is exempt from the key gate: localhost-only bind, the hook
    // payload carries no credential-protectable content, and hook POSTs are
    // 202-always. SEC-001's gate stays on the WakaTime endpoints.
    // Only the user's own key (read from ~/.wakatime.cfg at startup) may
    // write heartbeats. None expected -> accept-any fallback, never brick.
    let gated = if path == HOOK_CLAUDE { &None } else { expected_key };
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
    let owned = (method == "POST" && (path == BULK || path == SINGLE || path == HOOK_CLAUDE))
        || (method == "GET" && path == CURRENT);

    let (code, resp) = if owned {
        let (code, resp) = {
            let s = store.lock().expect("lock");
            match (method, path.as_str()) {
                ("POST", p) if p == BULK || p == SINGLE => match handle_body(&body, &s, auth) {
                    super::IngestOutcome::Stored(n) => {
                        drop(s);
                        kick(store);
                        let responses = serde_json::to_string(
                            &(0..n).map(|_| serde_json::json!([serde_json::json!({}), 201])).collect::<Vec<_>>(),
                        )
                        .expect("serialize ack");
                        (201, format!(r#"{{"responses":{responses}}}"#))
                    }
                    super::IngestOutcome::BadRequest(msg) => {
                        write_debug(&body);
                        (400, format!(r#"{{"error":"malformed body: {msg}"}}"#))
                    }
                },
                ("POST", p) if p == HOOK_CLAUDE => crate::hooks::claude::handle(&body, &s),
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::store::heartbeats;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, Mutex};

    // shared with hooks::claude's tests (same Arc<Mutex<Store>> + db shape)
    pub(crate) fn test_store(name: &str) -> (Arc<Mutex<Store>>, std::path::PathBuf) {
        let (store, db) = crate::store::testutil::tmp_path(name);
        (Arc::new(Mutex::new(store)), db)
    }

    // shared with hooks::claude's endpoint tests (same tiny_http harness)
    pub(crate) fn start(port: u16, store: Arc<Mutex<Store>>, expected_key: Option<String>) {
        std::thread::spawn(move || {
            let addr = format!("127.0.0.1:{port}");
            let server = tiny_http::Server::http(&addr).unwrap();
            loop {
                let req = server.recv().expect("recv");
                handle(req, &store, &expected_key);
            }
        });
        // wait for the listener to actually accept — spawned thread may lose
        // the race on CI (Linux refused immediately when tests connected first)
        for _ in 0..50 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("server on {port} never came up");
    }

    // shared with hooks::claude's endpoint tests (same tiny_http harness)
    pub(crate) fn send(port: u16, raw: &str) -> (u16, String) {
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

    const BULK_PATH: &str = "/api/v1/users/current/heartbeats.bulk";
    const AUTH: &str = "Authorization: Basic a2V5LXV1aWQ=\r\n"; // base64("key-uuid")
    const WRONG: &str = "Authorization: Basic c3Bvb2Yta2V5\r\n"; // base64("spoof-key")

    #[test]
    fn wrong_key_is_401_no_rows() {
        let (store, _db) = test_store("wrongkey");
        start(19877, store.clone(), Some("key-uuid".to_string()));
        let body = r#"[{"time":1.0,"entity":"e","type":"file","user_agent":"u"}]"#;
        let raw = format!("POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{WRONG}Content-Length: {}\r\n\r\n{body}", body.len());
        let (code, resp) = send(19877, &raw);
        assert_eq!(code, 401, "code={code}");
        assert!(resp.contains("invalid api_key"), "resp={resp}");
        // absent auth header is refused by the same branch, never accept-any
        let raw_noauth = format!("POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let (code, _) = send(19877, &raw_noauth);
        assert_eq!(code, 401, "no Authorization header: code={code}");
        assert_eq!(heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 0);
    }

    #[test]
    fn bulk_of_three_stored_and_acked_in_order() {
        let (store, db) = test_store("bulk3");
        start(19871, store.clone(), None);
        let hb = |t: f64| format!(r#"{{"time":{t},"entity":"C:\\p\\a.rs","type":"file","user_agent":"w/1.0"}}"#);
        let body = format!("[{},{},{}]", hb(1.0), hb(2.0), hb(3.0));
        let raw = format!(
            "POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{AUTH}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (code, resp) = send(19871, &raw);
        assert_eq!(code, 201, "code={code}");
        assert!(resp.starts_with(r#"{"responses":["#), "resp={resp}");
        assert_eq!(resp.matches("201").count(), 3);
        let s = Store::open(&db).unwrap();
        assert_eq!(heartbeats::unforwarded(&s, 10).unwrap().len(), 3);
        // same handler serves the single-object endpoint (one row, one ack)
        let body = r#"{"time":4.0,"entity":"C:\\p\\b.rs","type":"file","user_agent":"w/1.0"}"#;
        let raw = format!(
            "POST /api/v1/users/current/heartbeats HTTP/1.1\r\nHost: x\r\n{AUTH}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (code, _) = send(19871, &raw);
        assert_eq!(code, 201);
        assert_eq!(heartbeats::unforwarded(&s, 10).unwrap().len(), 4);
    }

    #[test]
    fn malformed_body_400_no_row() {
        let (store, db) = test_store("bad");
        start(19873, store.clone(), None);
        let raw = format!(
            "POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{AUTH}Content-Length: 5\r\n\r\nnot{{!");
        let (code, _) = send(19873, &raw);
        assert_eq!(code, 400);
        let s = Store::open(&db).unwrap();
        assert_eq!(heartbeats::unforwarded(&s, 10).unwrap().len(), 0);
        let dbg = crate::cli::serve::home_dir().join(".byline/debug");
        assert!(dbg.join("last-bad-body.json").exists(), "debug file missing");
    }

    #[test]
    fn unknown_path_proxied_to_upstream_status_passthrough() {
        let (store, _db) = test_store("proxypass");
        let (up_port, th) = crate::ingest::forward::tests::stub_upstream(
            1,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 17\r\n\r\n{\"upstream\":true}",
        );
        {
            let s = store.lock().unwrap();
            s.conn
                .execute("INSERT INTO config (key, value) VALUES ('upstream_api_url', ?)", [format!("http://127.0.0.1:{up_port}/api/hackatime/v1")])
                .unwrap();
            s.conn
                .execute("INSERT INTO config (key, value) VALUES ('upstream_api_key', 'up-key')", [])
                .unwrap();
        }
        start(19878, store.clone(), None);
        let raw = "GET /api/v1/users/current/durations?date=2026-01-01 HTTP/1.1\r\nHost: x\r\nAuthorization: Basic a2V5LXV1aWQ=\r\n\r\n";
        let (code, resp) = send(19878, &raw);
        let got = th.join().unwrap();
        assert_eq!(got.len(), 1);
        let got = &got[0];
        // proxied with method and query; local /api/v1 prefix replaced by the
        // upstream base's own version prefix
        assert!(
            got.starts_with("GET /api/hackatime/v1/users/current/durations?date=2026-01-01"),
            "req={got}"
        );
        let got_lower = got.to_lowercase();
        assert!(got_lower.contains("authorization: basic"), "auth header arrived, req={got}");
        // caller sees the upstream status and body unmodified
        assert_eq!(code, 200, "upstream status passthrough");
        assert_eq!(resp, r#"{"upstream":true}"#);
        let n: i64 = store.lock().unwrap().conn.query_row("SELECT count(*) FROM unknown_paths", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "diagnostic row still recorded");
    }

    #[test]
    fn unknown_path_404_and_recorded() {
        let (store, _db) = test_store("unknown");
        start(19874, store.clone(), None);
        let raw = "GET /api/v1/nope HTTP/1.1\r\nHost: x\r\n\r\n";
        let (code, _) = send(19874, &raw);
        assert_eq!(code, 404);
        let n: i64 = store.lock().unwrap().conn.query_row("SELECT count(*) FROM unknown_paths", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    // FEAT-002: the hook route is exempt from the key gate — no key and a
    // wrong key both succeed and store (wrong key on BULK is still 401,
    // pinned by wrong_key_is_401_no_rows).
    #[test]
    fn hook_route_stores_without_any_key() {
        let (store, db) = test_store("hookexempt");
        start(19879, store.clone(), Some("key-uuid".to_string()));
        let post = |auth: &str, id: &str| {
            let body = format!(r#"{{"tool_name":"Edit","event_id":"{id}"}}"#);
            send(19879, &format!("POST /api/v1/hooks/claude HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\n\r\n{body}", body.len()))
        };
        assert_eq!(post(WRONG, "evt-a").0, 202, "wrong key must not 401 the hook route");
        assert_eq!(post("", "evt-b").0, 202, "missing key must not 401 the hook route");
        let s = Store::open(&db).unwrap();
        assert_eq!(crate::store::agent_heartbeats::unforwarded(&s, 10).unwrap().len(), 2, "both stored");
    }

    #[test]
    fn keepalive_two_requests_one_connection() {
        let (store, _db) = test_store("keepalive");
        start(19875, store.clone(), None);
        let mut s = TcpStream::connect(("127.0.0.1", 19875)).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        let req = |i: u32| {
            let body = format!(r#"[{{"time":{i}.0,"entity":"e","type":"file","user_agent":"u"}}]"#);
            format!(
                "POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{AUTH}Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
        };
        s.write_all(req(1).as_bytes()).unwrap();
        s.write_all(req(2).as_bytes()).unwrap();
        // both responses arrive on the one connection (shared read helper)
        let out = format!(
            "{}{}",
            crate::ingest::forward::tests::read_request(&mut s),
            crate::ingest::forward::tests::read_request(&mut s)
        );
        assert_eq!(out.matches("HTTP/1.1 201").count(), 2, "two responses on one connection");
        assert_eq!(heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 2);
    }

    #[test]
    fn chunked_body_parsed() {
        let (store, db) = test_store("chunked");
        start(19876, store.clone(), None);
        let part1 = r#"[{"time":9.0,"entity":"c.rs""#;
        let part2 = r#","type":"file","user_agent":"u"}]"#;
        let raw = format!(
            "POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{AUTH}Transfer-Encoding: chunked\r\n\r\n{:x}\r\n{part1}\r\n{:x}\r\n{part2}\r\n0\r\n\r\n",
            part1.len(),
            part2.len()
        );
        let (code, _) = send(19876, &raw);
        assert_eq!(code, 201, "chunked body must be handled");
        let s = Store::open(&db).unwrap();
        assert_eq!(heartbeats::unforwarded(&s, 10).unwrap().len(), 1);
    }
}
