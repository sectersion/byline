mod common;
// derive::hours — pure gap-credit model, extracted from src (CHORE-004)

use byline::derive::hours::{per_day_project, RawHeartbeat};
use chrono::TimeZone;

fn hb(time: f64, entity: &str, project: Option<&str>) -> RawHeartbeat {
    let proj = match project {
        Some(p) => format!(r#","project":"{p}""#),
        None => String::new(),
    };
    // serde_json-escape the entity so test paths stay valid JSON
    let esc = serde_json::to_string(entity).unwrap().trim().to_string();
    RawHeartbeat { raw: format!(r#"{{"time":{time},"entity":{esc},"type":"file","user_agent":"u"{proj}}}"#) }
}

// 1764115200 = 2025-11-26 00:00 UTC. Local midnight of the same date,
// computed for the machine's own timezone, so the test holds anywhere.
const DAY0: f64 = 1764115200.0;
fn local_midnight(y: i32, m: u32, d: u32) -> f64 {
    chrono::Local
        .with_ymd_and_hms(y, m, d, 0, 0, 0)
        .earliest()
        .unwrap()
        .timestamp() as f64
}

#[test]
fn midnight_span_correct_sides() {
    let mid = local_midnight(2025, 11, 26);
    let rows = vec![
        hb(mid - 60.0, "C:\\p\\a.rs", None), // 1 min before local midnight
        hb(mid + 60.0, "C:\\p\\a.rs", None), // 1 min after
    ];
    let out = per_day_project(&rows);
    assert_eq!(out.len(), 2, "two days: {out:?}");
    let days: Vec<&str> = out.iter().map(|(d, _, _)| d.as_str()).collect();
    assert!(days.contains(&"2025-11-25") && days.contains(&"2025-11-26"), "{days:?}");
}

#[test]
fn null_project_uses_parent_dir() {
    let rows = vec![hb(DAY0, "C:\\Users\\oliver\\proj\\src\\main.rs", None)];
    let out = per_day_project(&rows);
    assert_eq!(out[0].1, "src");
}

#[test]
fn root_entity_unknown_never_dropped() {
    let rows = vec![hb(DAY0, "main.rs", None)];
    let out = per_day_project(&rows);
    assert_eq!(out[0].1, "(unknown)");
}

#[test]
fn empty_store_empty_report() {
    assert!(per_day_project(&[]).is_empty());
}

#[test]
fn explicit_project_wins() {
    let rows = vec![hb(DAY0, "C:\\x\\y\\f.rs", Some("byline"))];
    assert_eq!(per_day_project(&rows)[0].1, "byline");
}

#[test]
fn garbage_row_never_crashes() {
    let rows = vec![RawHeartbeat { raw: "not json".into() }, hb(DAY0, "a.rs", None)];
    assert_eq!(per_day_project(&rows).len(), 1);
}

// Gap credit: rows arrive out of order; consecutive heartbeats 2 min
// apart credit 2 min each; a lone heartbeat credits the full cap.
#[test]
fn gap_credit_two_min_apart() {
    let rows = vec![
        hb(DAY0 + 240.0, "C:\\p\\a.rs", None),
        hb(DAY0, "C:\\p\\a.rs", None),
        hb(DAY0 + 120.0, "C:\\p\\a.rs", None),
    ];
    let out = per_day_project(&rows);
    assert_eq!(out.len(), 1);
    assert!((out[0].2 - 6.0).abs() < 1e-9, "{}", out[0].2);
}

#[test]
fn long_gap_credits_cap_not_wall_clock() {
    // Two heartbeats an hour apart credit 2 + 2 minutes, not 62.
    let rows = vec![hb(DAY0, "C:\\p\\a.rs", None), hb(DAY0 + 3600.0, "C:\\p\\a.rs", None)];
    let out = per_day_project(&rows);
    assert!((out[0].2 - 4.0).abs() < 1e-9, "{}", out[0].2);
}

#[test]
fn burst_credits_actual_gap() {
    // Heartbeats 10s apart credit 10s each, not a flat minute.
    let rows = vec![hb(DAY0, "C:\\p\\a.rs", None), hb(DAY0 + 10.0, "C:\\p\\a.rs", None)];
    let out = per_day_project(&rows);
    assert!((out[0].2 - (10.0 + 120.0) / 60.0).abs() < 1e-9, "{}", out[0].2);
}

#[test]
fn projects_credit_independently() {
    // Interleaved projects never credit across the boundary.
    let rows = vec![
        hb(DAY0, "C:\\a\\f.rs", None),
        hb(DAY0 + 30.0, "C:\\b\\f.rs", None),
        hb(DAY0 + 150.0, "C:\\a\\f.rs", None),
    ];
    let out = per_day_project(&rows);
    let total: f64 = out.iter().map(|(_, _, m)| m).sum();
    // a: 150s gap capped to 120s, plus cap for last; b: cap
    assert!((total - (120.0 + 120.0 + 120.0) / 60.0).abs() < 1e-9, "{out:?}");
}
