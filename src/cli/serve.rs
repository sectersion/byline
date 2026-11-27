// byline serve — bind 127.0.0.1:8787 and run the ingest server

use std::sync::{Arc, Mutex};

pub fn run() -> Result<(), String> {
    let port: u16 = std::env::var("BYLINE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let (store, _path) = crate::cli::status::open_store()?;
    crate::ingest::server::run(port, Arc::new(Mutex::new(store)), load_expected(&crate::cli::setup::wakatime_cfg()))
}

/// The user's real key from the cfg setup manages. Unreadable file OR an
/// empty value -> None (accept-any fallback, never brick).
pub fn load_expected(path: &std::path::Path) -> Option<String> {
    let cfg = std::fs::read_to_string(path).ok()?;
    crate::cli::setup::get_ini(&cfg, "api_key").filter(|k| !k.trim().is_empty())
}

pub fn home_dir() -> std::path::PathBuf {
    // BYLINE_HOME overrides (tests/e2e); else Windows USERPROFILE, else HOME
    if let Some(b) = std::env::var_os("BYLINE_HOME") {
        return std::path::PathBuf::from(b);
    }
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .unwrap_or_default()
}
