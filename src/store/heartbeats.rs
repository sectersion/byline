// insert/queue operations over the heartbeats table

use super::{AuthMethod, Store};

// Row: id + raw only. auth_method/api_key stay as provenance db columns (SEC-001); nothing reads them here.
pub struct Row {
    pub id: i64,
    pub raw: String,
}

pub fn insert(store: &Store, raw: &str, auth: AuthMethod, api_key: &str) -> Result<i64, rusqlite::Error> {
    store.conn.execute(
        "INSERT INTO heartbeats (raw, received_at, forwarded_at, auth_method, api_key)
         VALUES (?, strftime('%s','now'), NULL, ?, ?)",
        rusqlite::params![raw, auth.as_str(), api_key],
    )?;
    Ok(store.conn.last_insert_rowid())
}

pub fn unforwarded(store: &Store, limit: usize) -> Result<Vec<Row>, rusqlite::Error> {
    let mut stmt = store.conn.prepare("SELECT id, raw FROM heartbeats WHERE forwarded_at IS NULL ORDER BY id LIMIT ?")?;
    let rows = stmt
        .query_map([limit as i64], |r| Ok(Row { id: r.get(0)?, raw: r.get(1)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn mark_forwarded(store: &Store, id: i64) -> Result<(), rusqlite::Error> {
    store.conn.execute(
        "UPDATE heartbeats SET forwarded_at = strftime('%s','now') WHERE id = ?",
        [id],
    )?;
    Ok(())
}

