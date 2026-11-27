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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::heartbeats;

    // The real home DB is off-limits: it is populated on dev machines and
    // receives live heartbeats. Every test opens its own temp store.
    fn temp_store(name: &str) -> crate::store::Store {
        let mut dir = std::env::temp_dir();
        dir.push(format!("byline-status-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("byline.db");
        std::fs::remove_file(&db).ok();
        crate::store::Store::open(&db).unwrap()
    }

    #[test]
    fn empty_store_zeros_not_errors() {
        let line = one_line(&temp_store("empty"));
        assert!(line.contains("rows=0"), "{line}");
        assert!(line.contains("unforwarded=0"), "{line}");
        assert!(line.contains("unknown-paths=0"), "{line}");
    }

    #[test]
    fn populated_store_counts() {
        let store = temp_store("populated");
        heartbeats::insert(&store, r#"{"time":1.0}"#, crate::store::AuthMethod::Basic, "k").unwrap();
        let line = one_line(&store);
        assert!(line.contains("rows=1"), "{line}");
        assert!(line.contains("unforwarded=1"), "{line}");
    }
}
