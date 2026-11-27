// byline serve — bind 127.0.0.1:8787 and run the ingest server

use std::sync::{Arc, Mutex};

pub fn run() -> Result<(), String> {
    let port: u16 = std::env::var("BYLINE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8787);
    let mut path = home_dir();
    std::fs::create_dir_all(&path).ok();
    path.push("byline.db");
    let store = crate::store::Store::open(&path).map_err(|e| e.to_string())?;
    crate::ingest::server::run(port, Arc::new(Mutex::new(store)))
}

pub fn home_dir() -> std::path::PathBuf {
    std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .unwrap_or_default()
}
