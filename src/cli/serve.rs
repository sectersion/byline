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
fn load_expected(path: &std::path::Path) -> Option<String> {
    let cfg = std::fs::read_to_string(path).ok()?;
    crate::cli::setup::get_ini(&cfg, "api_key").filter(|k| !k.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_absent_key_means_accept_any() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("byline-serve-{}-key", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(".wakatime.cfg");
        assert_eq!(load_expected(&p), None, "absent cfg");
        std::fs::write(&p, "[settings]\napi_key = \napi_url = x\n").unwrap();
        assert_eq!(load_expected(&p), None, "empty value must not 401 everything");
        std::fs::write(&p, "[settings]\napi_key = real-key\n").unwrap();
        assert_eq!(load_expected(&p), Some("real-key".to_string()));
    }
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
