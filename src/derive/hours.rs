// pure: stored rows in, minutes per (local day, project) out. No I/O.

use chrono::TimeZone;

pub struct RawHeartbeat {
    pub raw: String, // the payload object as received
}

/// Group heartbeats by local day of the heartbeat `time` field and project.
/// project = `project` field, else parent directory of `entity`, else `(unknown)`.
pub fn per_day_project(rows: &[RawHeartbeat]) -> Vec<(String, String, f64)> {
    // (day "YYYY-MM-DD", project, minutes)
    use std::collections::BTreeMap;
    // (project, time, day) — gathered first, then gap-credited per project.
    let mut hits: Vec<(String, f64, String)> = Vec::new();
    for row in rows {
        let v: serde_json::Value = match serde_json::from_str(&row.raw) {
            Ok(v) => v,
            Err(_) => continue, // unparseable rows never crash the report
        };
        let time = v.get("time").and_then(|t| t.as_f64()).unwrap_or(0.0);
        let day = local_day(time);
        let project = v
            .get("project")
            .and_then(|p| p.as_str())
            .map(str::to_string)
            .or_else(|| {
                v.get("entity").and_then(|e| e.as_str()).and_then(entity_parent)
            })
            .unwrap_or_else(|| "(unknown)".to_string());
        hits.push((project, time, day));
    }

    let mut buckets: BTreeMap<(String, String), f64> = BTreeMap::new();
    // Group by project, sort by time, credit each heartbeat the gap to the
    // next heartbeat of the same project, capped (WakaTime's duration model,
    // which Hackatime inherits). Stored rows arrive in insert order; gaps are
    // only meaningful in time order.
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap()));
    for (i, (project, time, day)) in hits.iter().enumerate() {
        let credit_secs = match hits.get(i + 1) {
            Some((next_project, next_time, _)) if next_project == project => {
                (next_time - time).min(GAP_CAP_SECS).max(0.0)
            }
            // Last heartbeat of a project: WakaTime gives it a full timeout
            // window (the duration runs until start + timeout).
            _ => GAP_CAP_SECS,
        };
        *buckets.entry((day.clone(), project.clone())).or_insert(0.0) += credit_secs / 60.0;
    }
    buckets.into_iter().map(|((d, p), m)| (d, p, m)).collect()
}

/// Seconds a heartbeat may credit toward the next one. 120s = the CLI's
/// `heartbeat_rate_limit` (contract §6) = WakaTime's duration timeout, which
/// Hackatime uses. Not yet matched against a real captured day — the parity
/// fixture from FIX-005 is deferred until one is captured. Full per-minute
/// class credits (HUMAN/STRUGGLE/AGENT) come with FEAT-003.
const GAP_CAP_SECS: f64 = 120.0;

/// Local day string from unix seconds, in the machine's local timezone
/// (correct per instant, so DST transitions land on the right side).
fn local_day(unix_secs: f64) -> String {
    let secs = unix_secs.floor() as i64;
    let dt = chrono::Local.timestamp_opt(secs, 0).unwrap();
    dt.format("%Y-%m-%d").to_string()
}

fn entity_parent(entity: &str) -> Option<String> {
    // entity may be a file path; take its parent directory's name
    let trimmed = entity.trim_end_matches(['/', '\\']);
    let idx = trimmed.rfind(['/', '\\'])?;
    let parent = &trimmed[..idx];
    let name = parent.rsplit(['/', '\\']).next()?;
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        use chrono::TimeZone;
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

    // Local offset comes from the process's real timezone via chrono; a
    // heartbeat one minute before local midnight lands on the previous date.
    #[test]
    fn day_of_heartbeat_is_its_local_date() {
        let mid = local_midnight(2025, 11, 26);
        let rows = vec![hb(mid - 60.0, "C:\\p\\a.rs", None)];
        let out = per_day_project(&rows);
        assert_eq!(out[0].0, "2025-11-25");
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
}
