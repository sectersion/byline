// auth parsing: Basic base64(key) NO colon split, or ?api_key= fallback

use super::AuthResult;
use crate::store::AuthMethod;

pub fn parse(auth_header: Option<&str>, query: Option<&str>) -> AuthResult {
    // query fallback: ?api_key=<key>
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some(v) = pair.strip_prefix("api_key=") {
                return AuthResult::ok(v.to_string(), AuthMethod::Query);
            }
        }
    }
    if let Some(h) = auth_header {
        if let Some(b64) = h.strip_prefix("Basic ") {
            // whole payload is the key — never split on ':'
            if let Some(key) = b64decode(b64.trim()) {
                return AuthResult::ok(key, AuthMethod::Basic);
            }
        }
    }
    AuthResult::bad()
}

fn b64decode(s: &str) -> Option<String> {
    // hand-rolled base64 decode of ASCII payload to a String key — no dep.
    // wakatime keys are UUIDv4 (ASCII), so this suffices.
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for ch in s.bytes() {
        if ch == b'=' || ch == b'\r' || ch == b'\n' {
            continue;
        }
        let idx = TABLE.iter().position(|&t| t == ch)? as u32;
        buf = (buf << 6) | idx;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    String::from_utf8(out).ok()
}

