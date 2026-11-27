// heartbeat endpoint + auth

pub mod agent_batch;
pub mod auth;
pub mod forward;
pub mod server;

use crate::store::Store;
pub use crate::store::AuthMethod;

#[derive(Debug)]
pub struct AuthResult {
    key: Option<String>,
    method: Option<AuthMethod>,
}

impl AuthResult {
    pub fn ok(key: String, method: AuthMethod) -> AuthResult {
        AuthResult { key: Some(key), method: Some(method) }
    }
    pub fn bad() -> AuthResult {
        AuthResult { key: None, method: None }
    }
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }
    pub fn method(&self) -> Option<AuthMethod> {
        self.method
    }
}

pub enum IngestOutcome {
    Stored(usize),      // number of heartbeats stored
    BadRequest(String), // complaint
}

/// Last 4 bytes of the key, empty for short keys. sqlite keeps provenance,
/// never the credential.
fn fingerprint(key: &str) -> &str {
    if key.len() <= 4 {
        return "";
    }
    key.get(key.len() - 4..).unwrap_or("")
}

/// Handles one parsed request body for the heartbeat endpoints.
/// Malformed body -> debug file + no store write.
pub fn handle_body(body: &[u8], store: &Store, auth: AuthResult) -> IngestOutcome {
    let parsed: serde_json::Result<serde_json::Value> = serde_json::from_slice(body);
    match parsed {
        Ok(serde_json::Value::Array(items)) => {
            let key = auth.key().unwrap_or("");
            let method = auth.method().unwrap_or(AuthMethod::Basic);
            let mut n = 0;
            for item in &items {
                let raw = item.to_string();
                if crate::store::heartbeats::insert(store, &raw, method, fingerprint(key)).is_ok() {
                    n += 1;
                }
            }
            IngestOutcome::Stored(n)
        }
        Ok(_) => IngestOutcome::Stored(handle_single(body, store, auth)),
        Err(e) => IngestOutcome::BadRequest(e.to_string()),
    }
}

fn handle_single(body: &[u8], store: &Store, auth: AuthResult) -> usize {
    let key = auth.key().unwrap_or("");
    let method = auth.method().unwrap_or(AuthMethod::Basic);
    // single object arrived; store as-is
    let raw = String::from_utf8_lossy(body).to_string();
    crate::store::heartbeats::insert(store, &raw, method, fingerprint(key)).map(|_| 1).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_caps_short_keys() {
        assert_eq!(fingerprint("abcdefgh"), "efgh");
        assert_eq!(fingerprint("abc"), "");  // short: nothing, never partial plaintext
        assert_eq!(fingerprint(""), "");     // empty auth present in code today
        assert_eq!(fingerprint("héllo"), ""); // multibyte boundary: no panic, no split char
    }
}
