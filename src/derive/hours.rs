// pure: stored rows in, minutes per (local day, project) out. No I/O.

use chrono::TimeZone;
use std::collections::HashSet;

pub struct RawHeartbeat {
    pub raw: String, // the payload object as received
}

/// Group heartbeats by local day of the heartbeat `time` field and project.
/// project = `project` field, else parent directory of `entity`, else `(unknown)`.
/// Duplicate heartbeats (same `entity` + `time`) are collapsed before credit
/// (FIX-003) — retried/re-sent rows would otherwise inflate hours.
pub fn per_day_project(rows: &[RawHeartbeat]) -> Vec<(String, String, f64)> {
    // (day "YYYY-MM-DD", project, minutes)
    use std::collections::BTreeMap;
    // Dedup: same entity + time = duplicate (retried/re-sent heartbeat).
    let mut seen: HashSet<(String, u64)> = HashSet::new();
    // (project, time, day) — gathered first, then gap-credited per project.
    let mut hits: Vec<(String, f64, String)> = Vec::new();
    for row in rows {
        let v: serde_json::Value = match serde_json::from_str(&row.raw) {
            Ok(v) => v,
            Err(_) => continue, // unparseable rows never crash the report
        };
        let time = v.get("time").and_then(|t| t.as_f64()).unwrap_or(0.0);
        let entity = v.get("entity").and_then(|e| e.as_str()).unwrap_or("").to_string();

        if !seen.insert((entity.clone(), time.to_bits())) {
            continue; // duplicate entity+time, skip
        }

        let day = local_day(time);
        let project = v
            .get("project")
            .and_then(|p| p.as_str())
            .map(str::to_string)
            .or_else(|| v.get("entity").and_then(|e| e.as_str()).and_then(entity_parent))
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

