// byline explain <session> — show minute-by-minute classification + verdict.

use crate::derive::{classify, timeline, verdict};
use crate::store::Store;
use std::io::Write;

/// Open the byline store at the user's home directory (~/.byline/byline.db).
fn open_store() -> Result<Store, String> {
    let mut path = crate::cli::serve::home_dir();
    std::fs::create_dir_all(&path).ok();
    path.push(".byline");
    std::fs::create_dir_all(&path).ok();
    path.push("byline.db");
    Store::open(&path).map_err(|e| e.to_string())
}

/// Run `byline explain <session>` — show minute-by-minute classification + verdict.
pub fn run(session_id: &str) -> Result<(), String> {
    let store = open_store()?;

    // Load human heartbeats + agent heartbeats
    let heartbeats: Vec<String> = store
        .conn
        .prepare("SELECT raw FROM heartbeats ORDER BY received_at")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let agent: Vec<String> = store
        .conn
        .prepare("SELECT raw FROM agent_heartbeats ORDER BY received_at")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    // Build sessions
    let sessions = timeline::sessions(&heartbeats, &agent);

    // Find the session matching the ID (or list all if no match)
    let target_sessions: Vec<&timeline::Session> = if session_id == "all" || session_id.is_empty() {
        sessions.iter().collect()
    } else {
        // Try to find by index
        sessions
            .iter()
            .enumerate()
            .filter(|(i, _)| i.to_string() == session_id)
            .map(|(_, s)| s)
            .collect()
    };

    if target_sessions.is_empty() {
        eprintln!("No sessions found for '{}'. Available: 0-{}", session_id, sessions.len().saturating_sub(1));
        return Ok(());
    }

    let mut stdout = std::io::stdout();
    for (idx, session) in target_sessions.iter().enumerate() {
        if idx > 0 {
            writeln!(stdout).ok();
        }
        writeln!(stdout, "=== Session {} ({}) ===", idx, session.project).ok();
        writeln!(stdout, "  start: {}  end: {}  events: {}", fmt_ts(session.start), fmt_ts(session.end), session.events.len()).ok();

        // Classify minutes
        let (classified, credited, wall_clock, class_counts) = classify::classify_minutes(session);

        // Print per-minute
        for (offset, cls) in &classified {
            writeln!(stdout, "  {:>4} min: {:?}", offset, cls).ok();
        }

        // Summary
        writeln!(stdout).ok();
        writeln!(stdout, "  wall-clock minutes: {:.1}", wall_clock).ok();
        writeln!(stdout, "  credited minutes:   {:.1}", credited).ok();
        let counted: f64 = class_counts.values().sum();
        writeln!(stdout, "  counted minutes:    {:.0}", counted).ok();

        // Verdict (Step 4)
        let (def_hours, v) = verdict::evaluate(credited, counted, wall_clock, None);
        writeln!(stdout, "  defensible hours:   {:.2}", def_hours).ok();
        writeln!(stdout, "  verdict:            {:?}", v).ok();
    }
    Ok(())
}

fn fmt_ts(ts: f64) -> String {
    use chrono::TimeZone;
    let secs = ts.floor() as i64;
    chrono::Local.timestamp_opt(secs, 0).unwrap().format("%Y-%m-%d %H:%M:%S").to_string()
}