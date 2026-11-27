// verdict — map a session's classified minutes to a defensible verdict
// (docs/review-artifact.md Step 4). Pure: minutes in, verdict out.

/// The four verdict outcomes the rule can produce.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    DefendFull,
    Partial,
    Flagged,
    Refused(String),
}

/// How many of `counted` minutes were credited (0..1).
pub fn ratio(credited_minutes: f64, counted_minutes: f64) -> f64 {
    if counted_minutes > 0.0 { (credited_minutes / counted_minutes).clamp(0.0, 1.0) } else { 0.0 }
}

/// Fraction of the session's wall-clock minutes that carried any event.
/// Coverage < 70% caps the verdict at PARTIAL regardless of ratio.
pub fn coverage(counted_minutes: f64, wall_clock_minutes: f64) -> f64 {
    if wall_clock_minutes > 0.0 { (counted_minutes / wall_clock_minutes).clamp(0.0, 1.0) } else { 0.0 }
}

/// Defensible hours = credited minutes / 60, capped at the session's wall-clock hours.
pub fn defensible_hours(credited_minutes: f64, wall_clock_minutes: f64) -> f64 {
    (credited_minutes / 60.0).min(wall_clock_minutes / 60.0)
}

/// Step 4 of the rule. `refuse` short-circuits to Refused (the three refusal
/// cases: agent-authored, phantom, rebuilt timeline).
pub fn evaluate(
    credited_minutes: f64,
    counted_minutes: f64,
    wall_clock_minutes: f64,
    refuse: Option<String>,
) -> (f64, Verdict) {
    if let Some(reason) = refuse {
        return (0.0, Verdict::Refused(reason));
    }
    let hours = defensible_hours(credited_minutes, wall_clock_minutes);
    let r = ratio(credited_minutes, counted_minutes);
    let cov = coverage(counted_minutes, wall_clock_minutes);

    // Any refuse-to-defend case -> 0 hours (handled above).
    // Coverage < 70% caps at PARTIAL regardless of ratio.
    let verdict = if cov < 0.70 {
        Verdict::Partial
    } else if r >= 0.80 {
        Verdict::DefendFull
    } else if r >= 0.40 {
        Verdict::Partial
    } else {
        Verdict::Flagged
    };
    (hours, verdict)
}