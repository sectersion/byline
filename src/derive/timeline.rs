// pure: merge human heartbeats + agent rows into gap-bounded sessions.
// A session is a maximal run of events (both tables) per project whose
// consecutive gaps stay under 30 minutes (docs/review-artifact.md).

use std::collections::BTreeMap;

/// One merged event from either table. `is_agent` distinguishes hook rows.
#[derive(Debug, Clone)]
pub struct Event {
    pub time: f64,      // unix seconds
    pub entity: String, // file path ("" when the row carries none)
    pub project: String,
    pub is_write: bool, // human heartbeats only; agent rows always false
    pub is_agent: bool,
}

/// A maximal run of events, per project, gaps < SESSION_GAP_SECS.
#[derive(Debug, Clone)]
pub struct Session {
    pub project: String,
    pub start: f64,
    pub end: f64,
    pub events: Vec<Event>,
}

/// 30-minute session gap (docs/review-artifact.md).
pub const SESSION_GAP_SECS: f64 = 1800.0;

/// Merge raw heartbeat + agent rows into sessions. `heartbeats` and `agent`
/// are the stored `raw` JSON strings from each table. Unparseable rows are
/// skipped, never crash. Project = explicit field, else parent dir of entity,
/// else "(unknown)" — same fallback as derive::hours.
pub fn sessions(heartbeats: &[String], agent: &[String]) -> Vec<Session> {
    let mut events: Vec<Event> = Vec::new();
    for raw in heartbeats {
        if let Some(e) = parse_human(raw) {
            events.push(e);
        }
    }
    for raw in agent {
        if let Some(e) = parse_agent(raw) {
            events.push(e);
        }
    }
    events.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));

    // group per project, then split on gaps >= SESSION_GAP_SECS
    let mut by_project: BTreeMap<String, Vec<Event>> = BTreeMap::new();
    for e in events {
        by_project.entry(e.project.clone()).or_default().push(e);
    }
    let mut out = Vec::new();
    for (project, mut evs) in by_project {
        evs.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));
        let mut cur: Vec<Event> = Vec::new();
        for e in evs {
            if let Some(last) = cur.last() {
                if e.time - last.time >= SESSION_GAP_SECS {
                    out.push(finish(&project, &cur));
                    cur = Vec::new();
                }
            }
            cur.push(e);
        }
        if !cur.is_empty() {
            out.push(finish(&project, &cur));
        }
    }
    out
}

fn finish(project: &str, evs: &[Event]) -> Session {
    Session {
        project: project.to_string(),
        start: evs.first().map(|e| e.time).unwrap_or(0.0),
        end: evs.last().map(|e| e.time).unwrap_or(0.0),
        events: evs.to_vec(),
    }
}

fn parse_human(raw: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let time = v.get("time")?.as_f64()?;
    let entity = v.get("entity").and_then(|e| e.as_str()).unwrap_or("").to_string();
    let project = v
        .get("project")
        .and_then(|p| p.as_str())
        .map(str::to_string)
        .or_else(|| entity_parent(&entity))
        .unwrap_or_else(|| "(unknown)".to_string());
    let is_write = match v.get("is_write") {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0) == 1,
        _ => false,
    };
    Some(Event { time, entity, project, is_write, is_agent: false })
}

fn parse_agent(raw: &str) -> Option<Event> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let time = v.get("time").and_then(|t| t.as_f64()).unwrap_or_else(|| {
        v.get("timestamp").and_then(|t| t.as_f64()).unwrap_or(0.0)
    });
    let entity = v.get("file_path").and_then(|e| e.as_str()).unwrap_or("").to_string();
    let project = v
        .get("project")
        .and_then(|p| p.as_str())
        .map(str::to_string)
        .or_else(|| entity_parent(&entity))
        .unwrap_or_else(|| "(unknown)".to_string());
    Some(Event { time, entity, project, is_write: false, is_agent: true })
}

/// Parent directory name of a path, separator-agnostic. Same as derive::hours.
fn entity_parent(entity: &str) -> Option<String> {
    let trimmed = entity.trim_end_matches(['/', '\\']);
    let idx = trimmed.rfind(['/', '\\'])?;
    let parent = &trimmed[..idx];
    let name = parent.rsplit(['/', '\\']).next()?;
    (!name.is_empty()).then(|| name.to_string())
}