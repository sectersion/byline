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
    let mut buckets: BTreeMap<(String, String), f64> = BTreeMap::new();
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
        *buckets.entry((day, project)).or_insert(0.0) += MINUTE_PER_HEARTBEAT;
    }
    buckets.into_iter().map(|((d, p), m)| (d, p, m)).collect()
}

/// Minutes credited per heartbeat occurrence (presence, not duration —
/// gap-based duration math comes with the hours rule, FEAT-003).
const MINUTE_PER_HEARTBEAT: f64 = 0.5;

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
}
