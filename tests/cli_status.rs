mod common;
// cli::status one-liner — extracted from src (CHORE-004)

use byline::cli::status::one_line;
use byline::store::heartbeats;

// The real home DB is off-limits: it is populated on dev machines and
// receives live heartbeats. Every test opens its own temp store.

#[test]
fn one_line_counts_reflect_the_store() {
    let store = common::tmp("status");
    // empty store: zeros, never errors
    let line = one_line(&store);
    assert!(line.contains("rows=0"), "{line}");
    assert!(line.contains("unforwarded=0"), "{line}");
    assert!(line.contains("unknown-paths=0"), "{line}");
    // populated: the counts move
    heartbeats::insert(&store, r#"{"time":1.0}"#, byline::store::AuthMethod::Basic, "k").unwrap();
    let line = one_line(&store);
    assert!(line.contains("rows=1"), "{line}");
    assert!(line.contains("unforwarded=1"), "{line}");
}
