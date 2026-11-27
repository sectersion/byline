// byline library: one process, one sqlite file. main.rs is the thin bin.
// Tests live in tests/ (integration, outside the src/ line budget) — CHORE-004.

pub mod cli;
pub mod derive;
pub mod hooks;
pub mod ingest;
pub mod report;
pub mod store;
