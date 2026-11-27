// byline status — one line: rows, last-heartbeat age, unforwarded, unknown paths

use crate::cli::serve::home_dir;

pub fn open_store() -> Result<(crate::store::Store, std::path::PathBuf), String> {
    let mut path = home_dir();
    std::fs::create_dir_all(&path).ok();
    path.push(".byline");
    std::fs::create_dir_all(&path).ok();
    path.push("byline.db");
    let store = crate::store::Store::open(&path).map_err(|e| e.to_string())?;
    Ok((store, path))
}

pub fn one_line(store: &crate::store::Store) -> String {
    let count: i64 = store.conn.query_row("SELECT count(*) FROM heartbeats", [], |r| r.get(0)).unwrap_or(0);
    let last: Option<i64> = store.conn.query_row("SELECT max(received_at) FROM heartbeats", [], |r| r.get(0)).ok();
    let unforwarded: i64 = store.conn.query_row("SELECT count(*) FROM heartbeats WHERE forwarded_at IS NULL", [], |r| r.get(0)).unwrap_or(0);
    let unknown: i64 = store.conn.query_row("SELECT count(*) FROM unknown_paths", [], |r| r.get(0)).unwrap_or(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let age = last.map(|l| now - l).unwrap_or(-1);
    format!("rows={count} last-heartbeat={age}s ago unforwarded={unforwarded} unknown-paths={unknown}")
}

