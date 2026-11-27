# FEAT-002: the agent stream Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harness hooks (Claude Code first) become agent heartbeats: byline receives hook events, composes heartbeats with `category: "ai coding"`, stores them locally, and POSTs them direct to Hackatime — so Hackatime's dashboards split AI vs human time natively and the hours rule gets real ai_* data.

**Architecture:** The daemon gains a second ingest path (hook events, not WakaTime shape) and reuses FEAT-001's forward drain parameterized over the agent_heartbeats table. A Claude Code plugin (hooks + settings snippet) is the first packaged harness. Editor heartbeats are untouched; dual-sender rules from the research apply.

**Tech Stack:** Same as FEAT-001. No new deps expected.

**Spec:** docs/superpowers/specs/2026-11-26-architecture-design.md; dual-sender rules (same project/entity as active editor session; only the agent source sends ai_*; only send while agent active; batch).

## Global Constraints

- Same CLAUDE.md budgets; deps frozen (nothing new expected; anything new = stop and ask).
- Agent heartbeat composition: `category: "ai coding"`, `ai_line_changes`/`ai_output_tokens`/`ai_model` from the hook event, `entity`/`project` matched to the editor's active session when known (else the hook's file), `time` = event time as float epoch seconds, `user_agent` = `byline/<version> (claude-code)`.
- Only forward agent heartbeats when the agent was active in the event window (hook events are inherently active — do not synthesize idle ones).
- Privacy: hook events may contain prompt text. Byline stores counts and model names only; prompt/file contents are never persisted, not even in debug files (SEC rule).
- One harness per PR: claude-code only. Next harnesses are separate PRs.

## Review Focus

- **Prompt leakage** — hook events carry full tool input. Every log/debug path must be payload-blind: test that a hook event containing a fake "secret" string results in zero occurrences of that string in the sqlite db or debug dir. Test in Task 1.
- **Duplicate hook deliveries** — Claude Code may re-deliver a hook (crash recovery). Same event id arriving twice must not double `ai_line_changes` in Hackatime's SUM: event-level idempotence (key on event id, ignore seen). Test in Task 2.
- **Agent active but editor closed** — no editor session to match entity against: compose with the hook's own file path, never drop. Test in Task 3.
- **Burst rate** — a long agent session emits many events per minute; batch to ≤100 per POST (Hackatime cap, contract §4) and ≤1 POST per 30s. Test in Task 3.

---

### Task 1: hook ingest — receive and store agent events

**Files:**
- Create: `src/hooks/mod.rs`, `src/hooks/claude.rs` (one file per tool, per layout)
- Modify: `src/ingest/server.rs` (add `POST /api/v1/hooks/claude` — same listener, one more match arm), `src/store/agent_heartbeats.rs`

**Interfaces:**
- Consumes: store open.
- Produces:
  - `hooks::claude::parse(event_json: &str) -> Result<HookEvent>` — the minimal normalized shape: {event_id, timestamp, tool_name, file_path, ai_model, ai_input_tokens, ai_output_tokens, ai_line_changes}. Counts and names only; text fields beyond path/model are dropped at parse time.
  - `store::agent_heartbeats::insert(store, &HookEvent) -> Result<i64>` — idempotent on event_id (unique index; second insert of same event_id = no-op).
  - Endpoint: `POST /api/v1/hooks/claude` → 202 `{"ok":true}` always (hook failures must never block the agent).

- [ ] **Step 1: Failing tests** — (a) parse extracts counts/model, drops prompt text (assert the secret string appears nowhere in any stored row); (b) duplicate event_id inserted twice → one row; (c) malformed event → 202 + counted in a `hook_errors` counter in config table, never crashes.
- [ ] **Step 2: Implement.** The Claude Code hook payload shape is pinned by a fixture: `fixtures/claude-code/post-tool-use.json` + `NOTES.md` (done-rule). If the real payload differs, fixture first, then code.
- [ ] **Step 3: Green → PR.**

### Task 2: compose agent heartbeats + reuse the drain

**Files:**
- Create: `src/hooks/compose.rs`
- Modify: `src/ingest/forward.rs` (drain parameterized: heartbeats | agent_heartbeats tables)

**Interfaces:**
- Consumes: Task 1 rows; FEAT-001 forward::drain; config table (upstream).
- Produces: `hooks::compose::to_heartbeat(store, hook_row) -> String` (JSON heartbeat, category "ai coding", fields per Global Constraints) and drain call wired for agent_heartbeats.

- [ ] **Step 1: Failing tests** — (a) composed heartbeat has category "ai coding", float `time`, `user_agent` names byline + claude-code, ai_* populated, NO prompt/secret anywhere; (b) entity/project: when a matching editor heartbeat exists within ±30 min, entity/project come from it; else from hook file path (parent dir as project); (c) drain sends composed row upstream, marks forwarded, never double-sends on restart (reuse FEAT-001 test pattern).
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Green → PR.**

### Task 3: batching + activity gating

**Files:**
- Modify: `src/ingest/forward.rs` (batch agent rows ≤100/POST, ≤1 POST/30s), `src/hooks/compose.rs` (drop hook events older than 10 min at compose time — stale replays don't create fake activity)

**Interfaces:**
- Produces: same drain signature; batching internal.

- [ ] **Step 1: Failing tests** — (a) 250 queued agent rows → 3 POSTs of ≤100; (b) two drains 5s apart → one POST, second defers; (c) hook event timestamped 2h ago → never composed.
- [ ] **Step 2: Implement.**
- [ ] **Step 3: Green → PR.**

### Task 4: Claude Code plugin packaging + interactive setup

**Files:**
- Create: `plugin/claude-code/` — hooks JSON + README (the installable plugin), `src/cli/setup.rs` (extend: detect `claude` on PATH / ~/.claude, ask, install plugin)
- Modify: `docs/superpowers/plans/` none; `CURRENT.md` (agent stream added)

**Interfaces:**
- Produces: `byline setup` flow: detect claude code → prompt → install hook config (writes to the plugin dir or settings, prints every file touched — same discipline as wakatime.cfg edits). MVP self-tracking: byline's own dev sessions are the first data.

- [ ] **Step 1: Failing test** — setup with a fake `claude` binary present → detects, installs hook, prints files; with `--no-plugins` flag → skips.
- [ ] **Step 2: Implement + write plugin README with install/verify instructions.**
- [ ] **Step 3: Green → PR. Then the real thing: install on Oliver's machine, work a session with Claude Code, and confirm Hackatime shows `ai coding` minutes — that's the MVP gate.**
