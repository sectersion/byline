mod common;
// ingest::server path dispatch — extracted from src (CHORE-004)

use byline::store::{heartbeats, Store};
use std::io::Write;
use std::net::TcpStream;

#[allow(unused_imports)]
use common::{AUTH, BULK_PATH, WRONG};

#[test]
fn wrong_key_is_401_no_rows() {
    let (store, _db) = common::test_store("wrongkey");
    common::start(19877, store.clone(), Some("key-uuid".to_string()));
    let body = r#"[{"time":1.0,"entity":"e","type":"file","user_agent":"u"}]"#;
    let raw = format!("POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{WRONG}Content-Length: {}\r\n\r\n{body}", body.len());
    let (code, resp) = common::send(19877, &raw);
    assert_eq!(code, 401, "code={code}");
    assert!(resp.contains("invalid api_key"), "resp={resp}");
    // absent auth header is refused by the same branch, never accept-any
    let raw_noauth = format!("POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}", body.len());
    let (code, _) = common::send(19877, &raw_noauth);
    assert_eq!(code, 401, "no Authorization header: code={code}");
    assert_eq!(heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 0);
}

#[test]
fn bulk_of_three_stored_and_acked_in_order() {
    let (store, db) = common::test_store("bulk3");
    common::start(19871, store.clone(), None);
    let hb = |t: f64| format!(r#"{{"time":{t},"entity":"C:\\p\\a.rs","type":"file","user_agent":"w/1.0"}}"#);
    let body = format!("[{},{},{}]", hb(1.0), hb(2.0), hb(3.0));
    let raw = format!(
        "POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{AUTH}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let (code, resp) = common::send(19871, &raw);
    assert_eq!(code, 201, "code={code}");
    assert!(resp.starts_with(r#"{"responses":["#), "resp={resp}");
    // FIX-006: each slot echoes back the posted heartbeat, not an empty object
    let parsed: serde_json::Value = serde_json::from_str(&resp).expect("resp is JSON");
    let responses = parsed.get("responses").expect("has responses array").as_array().expect("responses is array");
    assert_eq!(responses.len(), 3, "one slot per heartbeat");
    let first = &responses[0];
    assert_eq!(first[0]["entity"], serde_json::json!("C:\\p\\a.rs"), "first slot echoes entity");
    assert_eq!(first[0]["type"], serde_json::json!("file"), "first slot echoes type");
    assert_eq!(first[0]["user_agent"], serde_json::json!("w/1.0"), "first slot echoes user_agent");
    assert!(first[0]["time"].is_number(), "time is a number");
    assert_eq!(first[1], serde_json::json!(201), "status 201");
    let s = Store::open(&db).unwrap();
    assert_eq!(heartbeats::unforwarded(&s, 10).unwrap().len(), 3);
    // same handler serves the single-object endpoint (one row, one ack)
    let body = r#"{"time":4.0,"entity":"C:\\p\\b.rs","type":"file","user_agent":"w/1.0"}"#;
    let raw = format!(
        "POST /api/v1/users/current/heartbeats HTTP/1.1\r\nHost: x\r\n{AUTH}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let (code, _) = common::send(19871, &raw);
    assert_eq!(code, 201);
    assert_eq!(heartbeats::unforwarded(&s, 10).unwrap().len(), 4);
}

#[test]
fn malformed_body_400_no_row() {
    let (store, db) = common::test_store("bad");
    common::start(19873, store.clone(), None);
    let raw = format!(
        "POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{AUTH}Content-Length: 5\r\n\r\nnot{{!");
    let (code, _) = common::send(19873, &raw);
    assert_eq!(code, 400);
    let s = Store::open(&db).unwrap();
    assert_eq!(heartbeats::unforwarded(&s, 10).unwrap().len(), 0);
    let dbg = byline::cli::serve::home_dir().join(".byline/debug");
    assert!(dbg.join("last-bad-body.json").exists(), "debug file missing");
}

#[test]
fn unknown_path_proxied_to_upstream_status_passthrough() {
    let (store, _db) = common::test_store("proxypass");
    let (up_port, th) = common::stub_upstream(
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
    common::start(19878, store.clone(), None);
    let raw = "GET /api/v1/users/current/durations?date=2026-01-01 HTTP/1.1\r\nHost: x\r\nAuthorization: Basic a2V5LXV1aWQ=\r\n\r\n";
    let (code, resp) = common::send(19878, &raw);
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
    assert!(got_lower.contains("api_key=up-key"), "query-param auth arrived, req={got}");
    assert!(!got_lower.contains("authorization:"), "no basic header, req={got}");
    // caller sees the upstream status and body unmodified
    assert_eq!(code, 200, "upstream status passthrough");
    assert_eq!(resp, r#"{"upstream":true}"#);
    let n: i64 = store.lock().unwrap().conn.query_row("SELECT count(*) FROM unknown_paths", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "diagnostic row still recorded");
}

#[test]
fn unknown_path_404_and_recorded() {
    let (store, _db) = common::test_store("unknown");
    common::start(19874, store.clone(), None);
    let raw = "GET /api/v1/nope HTTP/1.1\r\nHost: x\r\n\r\n";
    let (code, _) = common::send(19874, &raw);
    assert_eq!(code, 404);
    let n: i64 = store.lock().unwrap().conn.query_row("SELECT count(*) FROM unknown_paths", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}

// FEAT-002: the hook route is exempt from the key gate — no key and a
// wrong key both succeed and store (wrong key on BULK is still 401,
// pinned by wrong_key_is_401_no_rows).
#[test]
fn hook_route_stores_without_any_key() {
    let (store, db) = common::test_store("hookexempt");
    common::start(19879, store.clone(), Some("key-uuid".to_string()));
    let post = |auth: &str, id: &str| {
        let body = format!(r#"{{"tool_name":"Edit","event_id":"{id}"}}"#);
        common::send(19879, &format!("POST /api/v1/hooks/claude HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\n\r\n{body}", body.len()))
    };
    assert_eq!(post(WRONG, "evt-a").0, 202, "wrong key must not 401 the hook route");
    assert_eq!(post("", "evt-b").0, 202, "missing key must not 401 the hook route");
    let s = Store::open(&db).unwrap();
    assert_eq!(byline::store::agent_heartbeats::unforwarded(&s, 10).unwrap().len(), 2, "both stored");
}

#[test]
fn keepalive_two_requests_one_connection() {
    let (store, _db) = common::test_store("keepalive");
    common::start(19875, store.clone(), None);
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
        common::read_request(&mut s),
        common::read_request(&mut s)
    );
    assert_eq!(out.matches("HTTP/1.1 201").count(), 2, "two responses on one connection");
    assert_eq!(heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 2);
}

#[test]
fn chunked_body_parsed() {
    let (store, db) = common::test_store("chunked");
    common::start(19876, store.clone(), None);
    let part1 = r#"[{"time":9.0,"entity":"c.rs""#;
    let part2 = r#","type":"file","user_agent":"u"}]"#;
    let raw = format!(
        "POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{AUTH}Transfer-Encoding: chunked\r\n\r\n{:x}\r\n{part1}\r\n{:x}\r\n{part2}\r\n0\r\n\r\n",
        part1.len(),
        part2.len()
    );
    let (code, _) = common::send(19876, &raw);
    assert_eq!(code, 201, "chunked body must be handled");
    let s = Store::open(&db).unwrap();
    assert_eq!(heartbeats::unforwarded(&s, 10).unwrap().len(), 1);
}
