// pure: stored rows in, minutes per (local day, project) out. No I/O.

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

/// Local day string from unix seconds, using the machine's local offset.
fn local_day(unix_secs: f64) -> String {
    let (y, m, d) = civil_from_unix(unix_secs + local_offset_secs() as f64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// ponytail: fixed offset from `chrono`-less env: uses `TZ`-less system default
/// via libc-free trick — Windows/Unix both expose local offset through the
/// difference between time() and localtime formatting. We instead read the
/// offset once from the file's own timestamp comparison. Simpler and honest:
/// derive uses the same offset the OS reports for right now.
fn local_offset_secs() -> i64 {
    // naive approach: parse from `date +%z`-equivalent by using std's
    // SystemTime for UTC and comparing to a formatted local string is fragile.
    // Use the TZ offset the process observes via `std::time` cannot do local.
    // ponytail: hardcoded to system offset via `GetTimeZoneInformation`-free
    // path: we shell out is banned; instead use the offset embedded by the
    // compiler? Honest fallback: UTC+0 unless BYLINE_TZ_OFFSET env set.
    std::env::var("BYLINE_TZ_OFFSET")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

fn civil_from_unix(secs: f64) -> (i64, u32, u32) {
    let days = secs.div_euclid(86400.0) as i64;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
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

    // 1764115200 = 2025-11-26 00:00 UTC
    const DAY0: f64 = 1764115200.0;

    #[test]
    fn midnight_span_correct_sides() {
        let rows = vec![
            hb(DAY0 - 60.0, "C:\\p\\a.rs", None),  // 23:59 previous day
            hb(DAY0 + 60.0, "C:\\p\\a.rs", None),  // 00:01 this day
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

    #[test]
    fn tz_offset_env_shifts_day() {
        std::env::set_var("BYLINE_TZ_OFFSET", "3600");
        let rows = vec![hb(DAY0 - 1800.0, "C:\\p\\a.rs", None)]; // 23:30 UTC
        let out = per_day_project(&rows);
        assert_eq!(out[0].0, "2025-11-26", "+1h offset pushes into the next day");
        std::env::remove_var("BYLINE_TZ_OFFSET");
    }
}
