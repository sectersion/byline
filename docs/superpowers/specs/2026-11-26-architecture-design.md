# byline architecture — design doc

**Status:** approved approach (conversation 2026-11-26). Plans: FEAT-001…004 in
`docs/superpowers/plans/`.

## Intent and why

Track productive struggle for teenagers building with AI. The lever is
approved-hours-per-logged-hour; the differentiator is a review artifact that
shows human vs agent work per session. byline counts human involvement around
AI, never agent runtime. MVP: byline tracks byline.

## Architecture (decided)

Two senders, one daemon, Hackatime as ledger.

```
editor (stock WakaTime plugin) ─▶ byline daemon ─▶ Hackatime   proxy, untouched
agent harness (byline hook)    ─▶ byline daemon ─▶ Hackatime   direct POST,
                                                                category "ai coding"
                                  │
                                  └─ local sqlite (raw rows only, no cached totals)
```

- **Borrow, don't replace** the editor layer. Stock WakaTime plugins stay;
  byline's proxy is the compatibility story. No byline editor extension unless
  a capture proves a real gap.
- **Agent stream is byline's own signal.** Harness hooks (Claude Code first)
  feed the daemon; the daemon composes agent heartbeats and POSTs them direct
  to Hackatime with `category: "ai coding"` — Hackatime's native AI marker
  (`heartbeat_ingest.rb:436-441`, `wakatime_user_agent_parser.rb:107`).
- **Never route agent heartbeats through wakatime-cli** (shared bbolt queue
  `offline_heartbeats.bdb`, shared backoff `wakatime-internal.cfg` —
  starves the editor plugin). Daemon POSTs direct.
- **Worker (Cloudflare):** homepage, docs, install script, submitted-hours
  storage, live hero number (on by default, anonymous, opt-out). No accounts
  server, no dashboard, no second review pipeline. Hackatime submission stays
  the ledger; review artifact travels via export.

## Goals

1. Wire compatibility: stock plugins work unchanged; Hackatime sees nothing
   new from the proxy path.
2. The hours rule produces defensible hours from stored rows; nothing cached.
3. Faking is expensive and visible, not impossible — the reviewer is the
   fraud control (README one-liner).
4. Runs on a 10-year-old low-spec Windows laptop; one process locally.

## Non-goals

No editor extensions (until proven), no accounts system, no dashboard beyond
homepage, no second review pipeline, no self-reporting/timers, no polling
the filesystem, no storing prompts/code/file contents/window titles.

## Constraints (from CLAUDE.md + brief)

- Budgets enforced by budget_test.rs: no file >400 lines (warn 300), src/
  <2,500 lines total, ≤1 new dep per PR, one surface per PR.
- Schema: hand-written, frozen; migrations never renumbered or edited.
- Wire: base URL `http://127.0.0.1:8787/api/v1`, never changes once shipped.
  Auth `Basic base64(key)` with NO colon. Ack: 201 +
  `{"responses":[[<heartbeat>,201],...]}` in order.
- Hackatime dual-sender rules (from research): same project/entity as the
  editor session; only the agent source sends ai_* fields; only send while
  the agent is active; batch agent heartbeats.
- hackatime-setup full-replaces ~/.wakatime.cfg — setup prints a warning and
  status surfaces stale heartbeat age.

## The plans

- **FEAT-001 daemon:** serve (4 endpoints), store raw, forward, setup/unsetup,
  report, status. The original v0.0.1 plan, restated.
- **FEAT-002 agent stream:** hooks/claude.rs — hook events → agent heartbeats
  (category "ai coding", same entity/project as the active editor session,
  direct POST, batched). Claude Code plugin packaging + interactive setup
  detection.
- **FEAT-003 hours rule:** derive/ implements the rule from
  docs/review-artifact.md; `byline explain <session>` one reason per minute.
- **FEAT-004 worker:** homepage/docs/install + hero-number ingest (one POST,
  one counter) + submitted-hours storage for the review flow.

## What's left to the builder

Exact thresholds (±15 min reviewed window, 80/40 verdict, 2-min gap) are
`ponytail:` constants until real data tunes them. Fixture formats for hook
parsing are the builder's, with NOTES.md per the done-rule.
