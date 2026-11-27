// sqlite, one file per concern

use std::path::Path;

pub mod agent_heartbeats;
pub mod heartbeats;

pub struct Store {
    pub conn: rusqlite::Connection,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthMethod {
    Basic,
    Query,
}

impl AuthMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthMethod::Basic => "basic",
            AuthMethod::Query => "query",
        }
    }
}

impl Store {
    /// Opens (creating if needed) and runs schema.sql if the database is
    /// fresh. Errors on version mismatch.
    pub fn open(path: &Path) -> Result<Store, rusqlite::Error> {
        let conn = rusqlite::Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i64 = conn.query_row(
            "SELECT version FROM schema_meta",
            [],
            |r| r.get(0),
        ).unwrap_or(-1);
        if version == -1 {
            // fresh: run schema.sql
            let schema = include_str!("../../docs/schema.sql");
            conn.execute_batch(schema)?;
            conn.execute("INSERT INTO schema_meta (version) VALUES (1)", [])?;
        } else if version != 1 {
            return Err(rusqlite::Error::InvalidParameterCount(
                "schema version mismatch".len(),
                version as usize,
            ));
        }
        Ok(Store { conn })
    }
}

// shared test helpers: every test module used to carry its own tmp_store copy
#[cfg(test)]
pub(crate) mod testutil {
    use super::Store;

    /// Fresh test store in a unique temp dir (pid + tag make it collision-free).
    pub(crate) fn tmp_path(tag: &str) -> (Store, std::path::PathBuf) {
        let mut dir = std::env::temp_dir();
        dir.push(format!("byline-test-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("byline.db");
        std::fs::remove_file(&path).ok();
        (Store::open(&path).unwrap(), path)
    }

    pub(crate) fn tmp(tag: &str) -> Store {
        tmp_path(tag).0
    }
}
