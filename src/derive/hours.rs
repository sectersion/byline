// pure: stored rows in, minutes per (local day, project) out. No I/O.

use chrono::TimeZone;
use std::collections::HashSet;

pub struct RawHeartbeat {
    pub raw: String, // the payload object as received
}

/// Episode-based credit. Rows carrying verification fields are evaluated as
/// problem episodes; old WakaTime-only rows retain the pre-FEAT-003 fallback
/// until the verification producer (SEC-002) is enabled.
pub fn per_day_project_episode(rows: &[RawHeartbeat], agent_rows: &[String]) -> Vec<(String, String, f64)> {
    let mut all: Vec<(String, f64, bool, Option<bool>)> = Vec::new();
    let mut verified = false;
    for row in rows {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&row.raw) {
            let t = v.get("time").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let p = project_of(&v);
            let outcome = verification_outcome(&v);
            verified |= outcome.is_some();
            all.push((p, t, v.get("is_write").and_then(|x| x.as_i64()).unwrap_or(0) == 1, outcome));
        }
    }
    for raw in agent_rows {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            let t = v.get("time").or_else(|| v.get("timestamp")).and_then(|x| x.as_f64()).unwrap_or(0.0);
            let p = project_of(&v);
            let outcome = verification_outcome(&v);
            verified |= outcome.is_some();
            all.push((p, t, false, outcome));
        }
    }
    if !verified {
        return per_day_project(rows);
    }

    all.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap()));
    let mut out: std::collections::BTreeMap<(String, String), f64> = std::collections::BTreeMap::new();
    let mut i = 0;
    while i < all.len() {
        let project = all[i].0.clone();
        let start = i;
        while i < all.len() && all[i].0 == project { i += 1; }
        let group = &all[start..i];
        let mut ep = 0;
        while ep < group.len() {
            let begin = ep;
            while ep + 1 < group.len() && group[ep + 1].1 - group[ep].1 < 1800.0 { ep += 1; }
            let end = ep;
            let failures: Vec<usize> = (begin..=end).filter(|&n| group[n].3 == Some(false)).collect();
            let resolves = (begin..=end).any(|n| group[n].3 == Some(true));
            let same_failure_grind = failures.len() >= 4;
            let credit = if resolves && failures.len() >= 2 && !same_failure_grind {
                (group[end].1 - group[begin].1).max(0.0) / 60.0
            } else if resolves && failures.is_empty() {
                // A one-shot is not productive struggle; only a small human
                // approval window is eligible, represented by one minute.
                if group.iter().any(|x| x.2) { 1.0 } else { 0.0 }
            } else { 0.0 };
            if credit > 0.0 {
                let day = local_day(group[begin].1);
                *out.entry((day, project.clone())).or_insert(0.0) += credit;
            }
            ep += 1;
        }
    }
    out.into_iter().map(|((d, p), m)| (d, p, m)).collect()
}

fn project_of(v: &serde_json::Value) -> String {
    v.get("project").and_then(|x| x.as_str()).map(str::to_string)
        .or_else(|| v.get("entity").or_else(|| v.get("file_path")).and_then(|x| x.as_str()).and_then(entity_parent))
        .unwrap_or_else(|| "(unknown)".to_string())
}

fn verification_outcome(v: &serde_json::Value) -> Option<bool> {
    v.get("exit_code").and_then(|x| x.as_i64()).map(|x| x == 0)
        .or_else(|| v.get("status").and_then(|x| x.as_str()).map(|x| matches!(x, "passed" | "pass" | "success" | "resolved")))
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

