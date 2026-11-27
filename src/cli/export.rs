// byline export — JSONL export of stored raw heartbeats

use crate::cli::status::open_store;

pub fn run() -> Result<(), String> {
    let (store, _path) = open_store()?;
    let raw_rows: Vec<String> = store
        .conn
        .prepare("SELECT raw FROM heartbeats ORDER BY id")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    print!("{}", crate::report::jsonl::render(&raw_rows));
    Ok(())
}