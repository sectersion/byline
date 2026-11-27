// insert/queue operations over the heartbeats table

use super::{AuthMethod, Store};

pub struct Row {
    pub id: i64,
    pub raw: String,
    pub auth_method: String,
    pub api_key: String,
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
    let mut stmt = store.conn.prepare(
        "SELECT id, raw, auth_method, api_key FROM heartbeats
         WHERE forwarded_at IS NULL ORDER BY id LIMIT ?",
    )?;
    let rows = stmt
        .query_map([limit as i64], |r| {
            Ok(Row {
                id: r.get(0)?,
                raw: r.get(1)?,
                auth_method: r.get(2)?,
                api_key: r.get(3)?,
            })
        })?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_roundtrips_raw() {
        let (store, _) = crate::store::testutil::tmp_path("roundtrip");
        let raw = r#"{"time":1764163200.5,"entity":"C:\\proj\\src\\main.rs","type":"file","user_agent":"wakatime/1.0"}"#;
        let id = insert(&store, raw, AuthMethod::Basic, "key-1").unwrap();
        assert!(id > 0);
        let rows = unforwarded(&store, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].raw, raw); // byte-identical
        assert_eq!(rows[0].auth_method, "basic");
        assert_eq!(rows[0].api_key, "key-1");
        assert_eq!(rows[0].id, id);
    }

    #[test]
    fn no_dedupe_two_identical_inserts() {
        let (store, _) = crate::store::testutil::tmp_path("nodedupe");
        let raw = r#"{"time":1,"entity":"a","type":"file","user_agent":"x"}"#;
        insert(&store, raw, AuthMethod::Basic, "k").unwrap();
        insert(&store, raw, AuthMethod::Basic, "k").unwrap();
        assert_eq!(unforwarded(&store, 10).unwrap().len(), 2);
    }

    #[test]
    fn mark_forwarded_removes_from_queue() {
        let (store, _) = crate::store::testutil::tmp_path("mark");
        let id = insert(&store, "{}", AuthMethod::Query, "k").unwrap();
        assert_eq!(unforwarded(&store, 10).unwrap().len(), 1);
        mark_forwarded(&store, id).unwrap();
        assert_eq!(unforwarded(&store, 10).unwrap().len(), 0);
    }
}
