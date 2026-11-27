// FEAT-003: timeline merge + minute classification + verdict.
// Each test pins a behavior named in docs/review-artifact.md.

use byline::derive::{classify, timeline, verdict};
use byline::derive::classify::MinuteClass;

// Build a human heartbeat raw JSON. is_write controls HUMAN credit.
fn hb(ts: f64, entity: &str, is_write: bool) -> String {
    let w = if is_write { 1 } else { 0 };
    let e = serde_json::to_string(entity).unwrap();
    format!(r#"{{"time":{ts},"entity":{e},"is_write":{w},"type":"file","user_agent":"u"}}"#)
}

// Build an agent row raw JSON (composed shape: time + file_path).
fn agent(ts: f64, entity: &str) -> String {
    let e = serde_json::to_string(entity).unwrap();
    format!(r#"{{"time":{ts},"file_path":{e},"tool_name":"Edit","hook_source":"claude-code"}}"#)
}

const BASE: f64 = 1_764_115_200.0; // 2025-11-26 00:00 UTC

// --- sessions ---

#[test]
fn sessions_split_on_30_min_gap() {
    let hb_raw = vec![hb(BASE, "proj/a.rs", true), hb(BASE + 3600.0, "proj/a.rs", true)];
    let sessions = timeline::sessions(&hb_raw, &[]);
    assert_eq!(sessions.len(), 2, "1h gap > 30min splits: {sessions:?}");
}

#[test]
fn sessions_merge_small_gap() {
    let hb_raw = vec![hb(BASE, "proj/a.rs", true), hb(BASE + 600.0, "proj/a.rs", true)];
    let sessions = timeline::sessions(&hb_raw, &[]);
    assert_eq!(sessions.len(), 1, "10min gap stays one session");
    assert_eq!(sessions[0].events.len(), 2);
}

#[test]
fn sessions_per_project_do_not_mix() {
    let hb_raw = vec![hb(BASE, "p1/a.rs", true), hb(BASE + 30.0, "p2/b.rs", true)];
    let sessions = timeline::sessions(&hb_raw, &[]);
    assert_eq!(sessions.len(), 2, "distinct projects -> distinct sessions");
}

// --- classification: HUMAN ---

#[test]
fn human_write_minute_credits_full() {
    let raw = vec![hb(BASE, "proj/a.rs", true), hb(BASE + 120.0, "proj/a.rs", true)];
    let s = timeline::sessions(&raw, &[]);
    let (classes, credited, _wall, counts) = classify::classify_minutes(&s[0]);
    assert!(classes.iter().any(|(_, c)| *c == MinuteClass::Human));
    assert_eq!(credited, 1.0, "one is_write minute credits 1.0");
    assert_eq!(counts.get(&MinuteClass::Human).copied().unwrap_or(0.0), 1.0);
}

// --- classification: agent reviewed vs unreviewed ---

#[test]
fn agent_edit_with_nearby_human_is_reviewed_half_credit() {
    // human is_write on same entity, agent edit 15 sec later (within ±15 min window)
    // filler human at end to extend session span past agent minute
    let raw = vec![hb(BASE + 60.0, "proj/a.rs", true), hb(BASE + 120.0, "proj/a.rs", true)];
    let agent_raw = vec![agent(BASE + 75.0, "proj/a.rs")];
    let s = timeline::sessions(&raw, &agent_raw);
    assert_eq!(s.len(), 1);
    let (_, credited, _wall, counts) = classify::classify_minutes(&s[0]);
    assert!(counts.contains_key(&MinuteClass::AgentReviewed), "reviewed: {counts:?}");
    // human minute (1.0) + agent reviewed minute (0.5) = 1.5
    assert!((credited - 1.5).abs() < 1e-9, "credited={credited}");
}

#[test]
fn agent_edit_without_human_is_unreviewed_zero() {
    // agent edits alone, no human heartbeat on that entity
    let agent_raw = vec![agent(BASE, "proj/a.rs"), agent(BASE + 60.0, "proj/a.rs")];
    let s = timeline::sessions(&[], &agent_raw);
    assert_eq!(s.len(), 1);
    let (_, credited, _wall, counts) = classify::classify_minutes(&s[0]);
    assert!(counts.contains_key(&MinuteClass::AgentUnreviewed), "{counts:?}");
    assert_eq!(credited, 0.0, "unreviewed agent credits 0");
}

// --- verdict ---

#[test]
fn verdict_full_when_ratio_high_and_coverage_high() {
    // 10 counted minutes, all credited, wall-clock 10 -> ratio 1.0, coverage 1.0
    let (hours, v) = verdict::evaluate(10.0, 10.0, 10.0, None);
    assert_eq!(v, verdict::Verdict::DefendFull);
    assert!((hours - 10.0 / 60.0).abs() < 1e-9, "hours={hours}");
}

#[test]
fn verdict_partial_when_ratio_mid() {
    // 10 counted, 6 credited -> 0.6 ratio
    let (_, v) = verdict::evaluate(6.0, 10.0, 10.0, None);
    assert_eq!(v, verdict::Verdict::Partial);
}

#[test]
fn verdict_flagged_when_ratio_low() {
    // 10 counted, 2 credited -> 0.2 ratio
    let (_, v) = verdict::evaluate(2.0, 10.0, 10.0, None);
    assert_eq!(v, verdict::Verdict::Flagged);
}

#[test]
fn verdict_coverage_below_70_caps_partial() {
    // ratio 1.0 but wall-clock 100 min, counted 60 -> coverage 0.6 < 0.70
    let (_, v) = verdict::evaluate(60.0, 60.0, 100.0, None);
    assert_eq!(v, verdict::Verdict::Partial, "low coverage caps at PARTIAL");
}

#[test]
fn verdict_refused_is_zero_hours() {
    let (hours, v) = verdict::evaluate(60.0, 60.0, 60.0, Some("agent-authored".into()));
    assert_eq!(hours, 0.0);
    assert_eq!(v, verdict::Verdict::Refused("agent-authored".into()));
}

#[test]
fn verdict_hours_capped_at_wall_clock() {
    // credited 120 min but wall-clock only 30 min -> cap at 30/60 = 0.5h
    let (hours, _) = verdict::evaluate(120.0, 120.0, 30.0, None);
    assert!((hours - 0.5).abs() < 1e-9, "capped hours={hours}");
}