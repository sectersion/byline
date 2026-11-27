// Shared helpers for the integration tests (CHORE-004). Each tests/*.rs
// binary imports what it needs; unused items are fine.
#![allow(dead_code)]

use byline::ingest::server::handle;
use byline::store::{agent_heartbeats, AuthMethod, Store};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

// ---- store helpers (were store::testutil) ----

/// Fresh test store in a unique temp dir (pid + tag make it collision-free).
pub fn tmp_path(tag: &str) -> (Store, PathBuf) {
    let mut dir = std::env::temp_dir();
    dir.push(format!("byline-test-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("byline.db");
    std::fs::remove_file(&path).ok();
    (Store::open(&path).unwrap(), path)
}

pub fn tmp(tag: &str) -> Store {
    tmp_path(tag).0
}

/// Store behind the server's Arc<Mutex<>> plus its db path.
pub fn test_store(name: &str) -> (Arc<Mutex<Store>>, PathBuf) {
    let (store, db) = tmp_path(name);
    (Arc::new(Mutex::new(store)), db)
}

/// Canonical agent test event: id and timestamp vary, the rest is fixed.
/// Change `file_path` by mutating the returned value when a test needs another path.
pub fn test_ev(id: &str, timestamp: i64) -> agent_heartbeats::HookEvent {
    agent_heartbeats::HookEvent {
        event_id: id.to_string(),
        timestamp: Some(timestamp),
        tool_name: "Edit".into(),
        file_path: Some("C:\\p\\a.rs".into()),
        ai_model: Some("claude-sonnet-4-5".into()),
        ai_input_tokens: Some(10),
        ai_output_tokens: Some(20),
        ai_line_changes: Some(3),
    }
}

// ---- tiny_http harness (was ingest::server::tests) ----

/// Spawn the server's handle loop on `port`; waits until it accepts.
pub fn start(port: u16, store: Arc<Mutex<Store>>, expected_key: Option<String>) {
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

/// One raw HTTP request in, (status, body) out.
pub fn send(port: u16, raw: &str) -> (u16, String) {
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

pub const BULK_PATH: &str = "/api/v1/users/current/heartbeats.bulk";
pub const AUTH: &str = "Authorization: Basic a2V5LXV1aWQ=\r\n"; // base64("key-uuid")
pub const WRONG: &str = "Authorization: Basic c3Bvb2Yta2V5\r\n"; // base64("spoof-key")

// ---- stub upstream (was ingest::forward::tests) ----

/// accept() that never blocks past `deadline` (None instead of hanging).
pub fn accept_until(listener: &TcpListener, deadline: std::time::Instant) -> Option<std::net::TcpStream> {
    listener.set_nonblocking(true).unwrap();
    loop {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_nonblocking(false).unwrap();
                return Some(s);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return None,
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Headers + full body arrived on the wire?
pub fn request_complete(got: &str) -> bool {
    let Some((_, body)) = got.split_once("\r\n\r\n") else { return false };
    let clen = got
        .lines()
        .find(|l| l.to_lowercase().starts_with("content-length"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    body.len() >= clen
}

/// Read one HTTP request off `s` (until complete, close, or read timeout).
pub fn read_request(s: &mut std::net::TcpStream) -> String {
    let (mut buf, mut got) = (Vec::new(), String::new());
    let mut chunk = [0u8; 65536];
    while !request_complete(&got) {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(k) => {
                buf.extend_from_slice(&chunk[..k]);
                got = String::from_utf8_lossy(&buf).into_owned();
            }
        }
    }
    got
}

/// Shared stub upstream: accepts up to `n` requests, records each raw request
/// (split the body off at "\r\n\r\n" where only the body matters), replies
/// with `reply` (empty reply -> close without responding). Ephemeral port.
pub fn stub_upstream(n: usize, reply: &'static str) -> (u16, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let th = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut reqs = Vec::new();
        for _ in 0..n {
            let Some(mut s) = accept_until(&listener, deadline) else { break };
            s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let got = read_request(&mut s);
            if !request_complete(&got) { break; }
            reqs.push(got);
            if !reply.is_empty() {
                let _ = s.write_all(reply.as_bytes());
                let _ = s.flush();
            }
        }
        reqs
    });
    (port, th)
}

/// Body of a recorded raw request.
pub fn body_of(req: &str) -> &str {
    req.split("\r\n\r\n").nth(1).unwrap_or("")
}

pub const REPLY_201: &str =
    "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 25\r\n\r\n{\"responses\":[[{},201]]}\r\n";

// ---- setup harness (was cli::setup::tests) ----

// TempHome mutates process-global env vars; tests share one HOME_LOCK.
pub static HOME_LOCK: Mutex<()> = Mutex::new(());

pub struct TempHome {
    dir: PathBuf,
}
impl TempHome {
    pub fn new(name: &str) -> (TempHome, PathBuf) {
        let mut dir = std::env::temp_dir();
        dir.push(format!("byline-setup-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        std::env::set_var("USERPROFILE", &dir);
        std::env::set_var("HOME", &dir);
        (TempHome { dir: dir.clone() }, dir)
    }
}
impl Drop for TempHome {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

pub const CFG: &str = "[settings]\napi_key = k-k-k\napi_url = https://hackatime.hackclub.com/api/hackatime/v1\nheartbeat_rate_limit_seconds = 30\n";

pub fn write_cfg(home: &PathBuf, content: &str) {
    let mut p = home.clone();
    p.push(".wakatime.cfg");
    std::fs::write(p, content).unwrap();
}

pub fn read_cfg(home: &PathBuf) -> String {
    let mut p = home.clone();
    p.push(".wakatime.cfg");
    std::fs::read_to_string(p).unwrap()
}

pub fn insert_hb(store: &Store, raw: &str, key: &str) -> i64 {
    byline::store::heartbeats::insert(store, raw, AuthMethod::Basic, key).unwrap()
}
