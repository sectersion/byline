// heartbeat endpoint + auth

pub mod auth;
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
    NotFound,
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
                if crate::store::heartbeats::insert(store, &raw, method, key).is_ok() {
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
    crate::store::heartbeats::insert(store, &raw, method, key).map(|_| 1).unwrap_or(0)
}
