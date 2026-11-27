# FEAT-003: the hours rule Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement docs/review-artifact.md's hours rule in `derive/`: classify every counted minute of a session from stored rows, produce defensible hours and a verdict, and expose `byline explain <session>` with one reason per minute.

**Architecture:** Pure functions over the two row stores (heartbeats + agent_heartbeats), merged timeline, no I/O, nothing cached. CLI renders.

**Tech Stack:** No new deps.

**Spec:** docs/review-artifact.md (the rule, verbatim); docs/wakatime-contract.md §5 (fields).

## Global Constraints

- Pure: rows in, minutes out, no I/O (CLAUDE.md layout rule).
- Thresholds are `ponytail:` constants: ±15 min reviewed window, 30-min session gap, 10-min stuck window, 80/40 verdict ratios, 70% coverage cap, 2-min duration gap.
- Never drop heartbeats; unknown-project → parent dir → `(unknown)`.
- Refusal cases (agent-authored, phantom, rebuilt timeline) produce a reason string, never a silent zero.

## Review Focus

- **Session boundary at midnight** — a session spanning 00:00 counts each minute to its own local day; the session is not split by the date roll. Test in Task 2.
- **Human and AI in the same minute** — HUMAN beats STRUGGLE beats AGENT-*; a minute with both human and reviewed-AI heartbeats credits 1.0 once, not 1.5. Test in Task 1.
- **Agent heartbeat without editor stream** (agent ran but editor closed): minutes classify AGENT-REVIEWED only if the hook's own human heartbeat exists (it doesn't) — so they classify AGENT-UNREVIEWED (0) even though real work happened. Byline must surface this as "no human signal in window", not silently zero. Test in Task 2.
- **Overlapping sessions from two projects** — two editors active at once (rare, real): minutes credit to both projects' sessions? No — the minute credits once, to the project with the most heartbeats in it; the other session's counted minutes exclude it. Test in Task 2.

---

### Task 1: timeline merge + minute classification

**Files:**
- Create: `src/derive/timeline.rs`, `src/derive/classify.rs`
- Test: in-file unit tests + `fixtures/hours/` (synthetic session fixtures with NOTES.md)

**Interfaces:**
- Produces:
  - `derive::timeline::merge(heartbeats: &[RawHeartbeat], agent: &[AgentRow]) -> Timeline` — sorted by time, gap-bounded sessions (30-min gap), each minute labeled.
  - `derive::classify::minute_class(timeline_minute) -> Class` where `Class = Human | Struggle | AgentReviewed | AgentUnreviewed | Silent`.

- [ ] **Step 1: Failing tests** — (a) minute with human + reviewed-AI heartbeat = Human (not 1.5); (b) stuck window (10 min, ~zero changes, file churn) that resolves on same files = Struggle at 1.0; (c) AI change ±15 min of human in same file = AgentReviewed; (d) AI change alone = AgentUnreviewed; (e) no heartbeat = Silent.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Green → PR.**

### Task 2: sessions across midnight, multi-project, missing-human cases

**Files:**
- Modify: `src/derive/timeline.rs`, `src/derive/classify.rs`

**Interfaces:**
- Produces: `derive::timeline::sessions(...) -> Vec<Session>` where Session carries per-minute classes + coverage + refusal flags.

- [ ] **Step 1: Failing tests** — (a) midnight span counts both days, one session; (b) agent-only session → AgentUnreviewed minutes + surfaced "no human signal" note, not a silent zero; (c) two concurrent projects → minute credited once to the majority project.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Green → PR.**

### Task 3: defensible hours + verdicts + refusals

**Files:**
- Create: `src/derive/verdict.rs`

**Interfaces:**
- Consumes: sessions.
- Produces: `derive::verdict::evaluate(session) -> Verdict { defensible_hours: f64, verdict: Defend | Partial | Flagged | Refused(reason), refusal: Option<RefusalKind> }` — the three refusal cases from the artifact doc produce named reasons.

- [ ] **Step 1: Failing tests** — (a) all-human session → DEFEND FULL, hours = wall-clock; (b) 60% agent-unreviewed → PARTIAL with defensible = human+struggle hours only; (c) zero human anywhere → Refused(agent-authored); (d) coverage <70% caps at PARTIAL; (e) rebuilt timeline (out-of-order epochs past repair) → Refused(timeline).
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Green → PR.**

### Task 4: `byline explain <session>` + report integration

**Files:**
- Create: `src/cli/explain.rs`, `src/report/explain.rs` (text render: one reason per minute)
- Modify: `src/cli/report.rs` (add defensible hours column), `src/main.rs`

**Interfaces:**
- Consumes: verdict.
- Produces: `byline explain <session-id>` prints the evidence block (minutes classified with reasons) and the verdict line. Done-rule satisfied: one reason per minute.

- [ ] **Step 1: Failing test** — a fixture session explains each minute with its class and why, and the total matches the rule by hand.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Green → PR. Then: byline tracks byline's own dev for a week; tune the constants against real data (the MVP gate for FEAT-003).**
