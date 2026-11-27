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
    /// Sets one config key (upsert; config is the only mutable non-event table).
    pub fn set_config(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute("INSERT INTO config (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])?;
        Ok(())
    }

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
