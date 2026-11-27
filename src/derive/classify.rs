// classify each minute of a session per the hours rule (docs/review-artifact.md).
// Priority order (first match wins):
//   1. HUMAN     — ≥1 heartbeat with is_write=1 in the minute       → 1.0 min credit
//   2. STRUGGLE  — inside a resolved stuck window                 → 1.0 min credit
//   3. AGENT-REVIEWED  — ≥1 agent edit; entity has human hb within ±15 min  → 0.5 min credit
//   4. AGENT-UNREVIEWED — ≥1 agent edit; NO such human heartbeat → 0 min credit
//   5. SILENT    — no event                                                                   → 0 min credit

use crate::derive::timeline::Event;
use std::collections::HashMap;

/// One minute classification class from the hours rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MinuteClass {
    Human,
    Struggle,
    AgentReviewed,
    AgentUnreviewed,
    Silent,
}

/// Classify every minute of `session` per the hours rule.
/// Returns (classified, credited_minutes, wall_clock_minutes, class_counts).
// classified: list of (minute_offset, class) for every minute in the session range
// credited: total minutes with non-zero credit (HUMAN+STRUGGLE+AGENT-REVIEWED)
// wall_clock: session span in minutes (ceil to nearest minute)
// class_counts: per-class minute counts
pub fn classify_minutes(
    session: &crate::derive::timeline::Session,
) -> (
    Vec<(i64, MinuteClass)>,
    f64,
    f64,
    HashMap<MinuteClass, f64>,
) {
    let start_min = (session.start / 60.0).floor() as i64;
    let end_min = (session.end / 60.0).ceil() as i64; // inclusive upper bound
    if end_min <= start_min {
        return (vec![], 0.0, 0.0, HashMap::new());
    }

    // Collect events per minute: (is_write_count, agent_edit_count, agent_entities)
    let mut min_data: HashMap<i64, (usize, usize, Vec<String>)> = HashMap::new();
    for ev in &session.events {
        let mi = (ev.time / 60.0).floor() as i64;
        min_data.entry(mi).or_default();
        let d = min_data.get_mut(&mi).unwrap();
        if ev.is_agent {
            d.1 += 1;
            d.2.push(ev.entity.clone());
        } else if ev.is_write {
            d.0 += 1;
        }
    }

    // Build per-minute lookup tables
    let min_has_write: HashMap<i64, bool> = min_data
        .iter()
        .map(|(mi, (is_c, _ac, _ae))| (*mi, *is_c > 0))
        .collect();
    let min_agent_entities: HashMap<i64, Vec<String>> = min_data
        .iter()
        .map(|(mi, (_, _ac, ae))| (*mi, ae.clone()))
        .collect();

    // Compute stuck windows (on human heartbeats only)
    let stuck_windows = compute_stuck_windows(session);

    // Classify each minute
    let mut classified: Vec<(i64, MinuteClass)> = Vec::new();
    let mut credited: f64 = 0.0;
    let mut class_counts: HashMap<MinuteClass, f64> = HashMap::new();

    for min in start_min..end_min {
        let mi = min - start_min; // offset within session

        // Priority 1: HUMAN
        if *min_has_write.get(&min).unwrap_or(&false) {
            classified.push((mi, MinuteClass::Human));
            credited += 1.0;
            *class_counts.entry(MinuteClass::Human).or_insert(0.0) += 1.0;
            continue;
        }

        // Priority 2: STRUGGLE — inside a resolved stuck window
        if is_in_resolved_stuck_window(min, &stuck_windows) {
            classified.push((mi, MinuteClass::Struggle));
            credited += 1.0;
            *class_counts.entry(MinuteClass::Struggle).or_insert(0.0) += 1.0;
            continue;
        }

        // Priority 3 & 4: Agent classes
        if let Some(entities) = min_agent_entities.get(&min) {
            let has_human = has_human_heartbeat_in_window(&session, entities, min);
            if has_human {
                classified.push((mi, MinuteClass::AgentReviewed));
                credited += 0.5;
                *class_counts.entry(MinuteClass::AgentReviewed).or_insert(0.0) += 1.0;
                continue;
            } else if !entities.is_empty() {
                classified.push((mi, MinuteClass::AgentUnreviewed));
                credited += 0.0;
                *class_counts.entry(MinuteClass::AgentUnreviewed).or_insert(0.0) += 1.0;
                continue;
            }
        }

        // Priority 5: SILENT
        classified.push((mi, MinuteClass::Silent));
        *class_counts.entry(MinuteClass::Silent).or_insert(0.0) += 1.0;
    }

    let wall_clock = (end_min - start_min) as f64;
    (
        classified,
        credited,
        wall_clock,
        class_counts,
    )
}

/// Returns true if minute `min` falls inside a stuck window that has resolved.
fn is_in_resolved_stuck_window(min: i64, windows: &[(i64, i64, String)]) -> bool {
    for (ws, we, _) in windows {
        if min >= *ws && min <= *we {
            return true;
        }
    }
    false
}

/// Compute stuck windows from human heartbeats.
/// A stuck window: ≥10 min of active presence where no two consecutive touches
/// are >10 min apart and no is_write=1 heartbeat. Resolves on first
/// is_write=1 or agent edit on the same file (within ±15 min).
/// Never resolves at session end — window stays open.
fn compute_stuck_windows(session: &crate::derive::timeline::Session) -> Vec<(i64, i64, String)> {
    let mut windows: Vec<(i64, i64, String)> = Vec::new();

    // Group ALL human (non-agent) heartbeats by entity — presence, not just writes.
    // A stuck window is ≥10 min of active presence with no is_write=1 heartbeat.
    let human_evts: Vec<&Event> = session.events.iter().filter(|e| !e.is_agent).collect();
    let mut by_entity: HashMap<String, Vec<i64>> = HashMap::new();
    for ev in &human_evts {
        by_entity
            .entry(ev.entity.clone())
            .or_default()
            .push((ev.time / 60.0).floor() as i64);
    }

    for (entity, mins) in &by_entity {
        let mut sorted: Vec<i64> = mins.clone();
        sorted.sort();
        sorted.dedup();
        if sorted.len() < 10 {
            // Need at least 10 min of activity to open a window
            continue;
        }

        // Sweep to find runs of ≥10 active minutes with no gap >10 min
        let mut runs: Vec<(i64, i64)> = Vec::new(); // (run_start, run_end_minute)
        let mut run_start: Option<i64> = None;
        let mut last_min: Option<i64> = None;

        for &m in &sorted {
            if let Some(start) = run_start {
                let gap = m - last_min.unwrap_or(m);
                if gap > 10 {
                    // run breaks — save it if it had ≥10 min
                    runs.push((start, last_min.unwrap_or(m - 1)));
                    run_start = Some(m);
                }
            } else {
                run_start = Some(m);
            }
            last_min = Some(m);
        }
        // last run
        if let Some(s) = run_start {
            runs.push((s, last_min.unwrap_or(s + 9)));
        }

        for (rs, re) in &runs {
            if re - rs + 1 < 10 {
                continue;
            }
            // Skip runs that contain any write heartbeat — struggle means nothing landed.
            let contains_write = session.events.iter().any(|ev| {
                if ev.is_agent || !ev.is_write || ev.entity != *entity {
                    return false;
                }
                let mi = (ev.time / 60.0).floor() as i64;
                mi >= *rs && mi <= *re
            });
            if contains_write {
                continue;
            }
            // Check if it resolves: first is_write=1 or agent edit on same file within ±15 min
            if resolves_within(&session, *rs, *re, &entity) {
                windows.push((*rs, *re, entity.clone()));
            }
        }
    }
    windows
}

/// Check if a stuck window from `run_start` to `run_end` resolves:
/// resolves on first is_write=1 or agent edit on the same file within ±15 min
/// of the window's end minute.
fn resolves_within(
    session: &crate::derive::timeline::Session,
    _run_start: i64,
    run_end: i64,
    entity: &str,
) -> bool {
    // Check human is_write=1 heartbeats within ±15 min of run_end
    for ev in &session.events {
        if ev.is_agent || !ev.is_write || ev.entity != entity {
            continue;
        }
        let ev_min = (ev.time / 60.0).floor() as i64;
        if ev_min >= run_end.saturating_sub(15) && ev_min <= run_end.saturating_add(15) {
            return true;
        }
    }
    // Check agent edits on same file within ±15 min of run_end
    for ev in &session.events {
        if !ev.is_agent || ev.entity != entity {
            continue;
        }
        let ev_min = (ev.time / 60.0).floor() as i64;
        if ev_min >= run_end.saturating_sub(15) && ev_min <= run_end.saturating_add(15) {
            return true;
        }
    }
    false
}

/// Check if any human heartbeat with is_write=1 exists in the agent's entity
/// within ±15 min of `min`.
fn has_human_heartbeat_in_window(
    session: &crate::derive::timeline::Session,
    agent_entities: &[String],
    min: i64,
) -> bool {
    let win_start = min - 15;
    let win_end = min + 15;
    for ev in &session.events {
        if ev.is_agent || !ev.is_write {
            continue;
        }
        for ae in agent_entities {
            if ev.entity == *ae {
                let ev_min = (ev.time / 60.0).floor() as i64;
                if ev_min >= win_start && ev_min <= win_end {
                    return true;
                }
            }
        }
    }
    false
}