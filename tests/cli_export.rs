mod common;
// cli::export JSONL — extracted from src (CHORE-004)

use byline::report::jsonl::render;
use byline::store::heartbeats;

#[test]
fn export_emits_jsonl() {
    let store = common::tmp("export");
    heartbeats::insert(&store, r#"{"time":1.0,"entity":"a.py","project":"foo"}"#, byline::store::AuthMethod::Basic, "k").unwrap();
    heartbeats::insert(&store, r#"{"time":2.0,"entity":"b.py","project":"foo"}"#, byline::store::AuthMethod::Basic, "k").unwrap();

    let raw_rows: Vec<String> = store
        .conn
        .prepare("SELECT raw FROM heartbeats ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let out = render(&raw_rows);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], r#"{"time":1.0,"entity":"a.py","project":"foo"}"#);
    assert_eq!(lines[1], r#"{"time":2.0,"entity":"b.py","project":"foo"}"#);
}

#[test]
fn export_empty_db_emits_nothing() {
    let store = common::tmp("export-empty");
    let raw_rows: Vec<String> = store
        .conn
        .prepare("SELECT raw FROM heartbeats ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let out = render(&raw_rows);
    assert_eq!(out, "");
}