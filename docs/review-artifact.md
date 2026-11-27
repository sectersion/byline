# The hours rule

How a session of work-with-AI becomes defensible hours — one rule, applied
identically to every session, computed by a pure function over the stored
event rows. The reviewer confirms or deflates, never invents a number.

## The events the rule reads

Every input of the rule is a row in one of the two stored tables; nothing
else exists for it to read.

| Event | Source | Stored in | Fields the rule reads |
|---|---|---|---|
| heartbeat (human) | the stock editor plugin | `heartbeats.raw` | `time`, `entity`, `is_write` |
| prompt (agent) | harness hook (FEAT-002) | `agent_heartbeats.raw` | `time` |
| edit (agent) | harness hook (FEAT-002) | `agent_heartbeats.raw` | `time`, `entity` |

Heartbeat fields come from the frozen wire list (docs/wakatime-contract.md
§5); agent rows are composed by the hook ingest
(docs/superpowers/plans/2026-11-26-feat-002-agent-stream.md), with
`hook_source` naming the harness. The rule never reads a field the stock
plugin does not send. In particular the wire struct carries
`human_line_changes`, but the stock plugin never sets it, so no class
depends on it.

A **session** is a maximal run of events from both tables merged and sorted
by `time`, per project, whose consecutive gaps stay under 30 minutes.

## The rule

One rule, applied identically to every session, recomputable by any outside
party from the stored rows of the two tables above — both export in full.
The reviewer's job is to confirm or deflate, never to invent a number. The
output is **defensible hours** — a plain hour count, no invented unit.

**Step 1 — stuck windows.** Stuck windows are computed first, on the human
heartbeat stream of one file (`entity`):

- **Opens:** a run of at least 10 minutes in which the file receives
  heartbeats with no two consecutive touches more than 10 minutes apart and
  no `is_write=1` heartbeat on the file — active presence, nothing landed.
- **Thrash filter:** an `is_write=1` heartbeat on the file ends the run (a
  new run may start after it); a gap over 10 minutes ends the run; a run
  shorter than 10 minutes never opens a window.
- **Resolves:** the first `is_write=1` heartbeat on the file after the run,
  or a reviewed agent edit on the file (an agent edit with a human heartbeat
  on the same entity within ±15 min). Only a resolution inside the same
  session counts.
- **Never resolves:** a window still open at the end of its session credits
  nothing — no retroactive rescue at session end.

**Step 2 — classify each counted minute.** Classes apply in priority order;
the first match wins:

| Minute class | Test (over the stored rows) | Credit |
|---|---|---|
| HUMAN | ≥ 1 heartbeat with `is_write=1` in the minute | **1.0 min** |
| STRUGGLE | falls inside a stuck window that resolved | **1.0 min** |
| AGENT-REVIEWED | ≥ 1 agent edit in the minute whose entity has ≥ 1 human heartbeat within ±15 min | **0.5 min** |
| AGENT-UNREVIEWED | ≥ 1 agent edit with no such human heartbeat | **0 min** |
| SILENT | no event in the minute | **0 min** |

A minute matching no credited test — heartbeats present, nothing landed, not
inside a resolved window — credits 0. HUMAN beats STRUGGLE beats AGENT-*.

**Step 3 — sum into hours.** `defensible hours = (Σ credited minutes) / 60`,
capped at the session's wall-clock hours.

**Step 4 — the verdict derives from the ratio:**

- `≥ 80% of counted minutes credited` → **DEFEND FULL**
- `40–80%` → **PARTIAL** (defensible: the HUMAN + STRUGGLE minutes)
- `< 40%` → **FLAGGED**, showing which minutes made the difference
- Any refuse-to-defend case (§ three cases) → **0 hours, with the reason line**
- Coverage < 70% caps the verdict at PARTIAL regardless of ratio

**Overrides are downward-only and reasoned.** A reviewer may deflate
(`lower hours`, must cite a reason from a fixed list: *agent dump, phantom
files, timeline failure, thin evidence, reviewer discretion with note*) but
never inflate above the rule's output. Every override is stored on the
submission, so an outside auditor sees both the rule's hours and the human's.

**Why these credits:** HUMAN and STRUGGLE at a full minute each is the
thesis — being stuck and pushing through is the work, and reviewers'
historic deflation punished exactly those minutes. AGENT-REVIEWED at half
pays for verified delegation without letting it out-earn authorship.
AGENT-UNREVIEWED at zero is the lever: it is the only way to move
approved-hours-per-logged-hour at scale, and it can't be gamed without
producing the very pattern (zero human heartbeats) the refusal rules catch.

**Where it runs:** only in the daemon, over stored rows — nothing cached,
derivable per the frozen rule. Hackatime's ingest sees nothing new: byline
forwards the original payloads untouched (pass-through), so the wire stays
exactly as compatible as before. The evidence block and its defensible hours
are a byline artifact, delivered to a reviewer via export/submission flow —
not through the heartbeat ingest, where Hackatime's field filter would strip
it (docs/wakatime-contract.md §5) and where inserting it would break the
pass-through promise.

**What the worker is (and is not).** The Cloudflare worker is a homepage,
docs, the install script, and storage for submitted hours. Submission itself
is not a new system: submitting a byline project is as regular as submitting
a Hackatime project — Hackatime remains the ledger, so human and agent time
are tracked seamlessly in the same place. Byline adds no accounts server, no
dashboard beyond the homepage, no second review pipeline.

**Bounty requirement 9 — open scope decision, not yet confirmed.** Requirement
9 asks for a centralized server with accounts, public-by-default profiles,
projects, git integration and a real-time stats dashboard. This doc takes the
read that Hackatime satisfies requirement 9 (it is the ledger, and byline
forwards to it as the labeling layer on top), with requirement 19's
local-until-submit clause supporting that read. As of 2026-11-26 this read is
an assertion in this doc, not a decision from the program: **it has not been
answered in writing by Zach.** Byline deliberately builds none of the
requirement-9 surfaces itself — no accounts, no profiles, no dashboard beyond
the homepage. Until the written answer lands, this is treated as an unresolved
scope decision; if the answer is "byline must own it", FEAT-004 gets replanned
before any code and the README's positioning line changes with it.

## When the hooks are not installed

The classes degrade downward, never upward:

- HUMAN and STRUGGLE read `heartbeats` only; they survive a heartbeats-only
  setup unchanged.
- AGENT-REVIEWED and AGENT-UNREVIEWED exist only where agent rows exist.
  With no hooks installed there are none: the agent's working time is
  invisible — silent gaps, or ordinary HUMAN minutes where the editor saw
  the human — and credits 0. A heartbeats-only user gets an honest, lower
  number, not a flattering one.
- If the plugin never sets `is_write`, no change ever lands on the human
  stream: HUMAN credits 0 and no stuck window can resolve, so only
  AGENT-REVIEWED can still earn.

## Evidence, and the fact that it can be faked

"Evidence" here means the stored rows of the two tables — that is all it is.
The evidence block is a rendering of them, and the hours rule is a pure
function over them. So the fake question is exactly: can a user fabricate
rows?

**Yes.** The daemon is a local HTTP server; anyone can POST synthesized
heartbeats to 127.0.0.1:8787 with plausible `is_write` flags spread over an
evening, and synthesized hook events down the hook path. No local scheme —
hashing, signing, obfuscation — changes that: the attacker owns the machine
the signatures are made on. Anyone claiming otherwise is selling something.
What defends the number is what happens at review:

1. **Faking has no upside for honest users.** HUMAN and STRUGGLE minutes
   already credit at 1.0. Fabrication only pays someone who has no honest
   hours — and the program's own acceptance bar (reviewed *and shipped*)
   still requires a real project at the end.
2. **Review recomputes.** A doubtful reviewer takes the raw rows (export
   includes both tables) and re-runs the rule. Fabricated rows must stay
   internally consistent: timestamps ordered, file paths real, language
   matching file extensions, writes landing where the presence stream says
   they should, coverage sane. The daemon itself refuses to defend rebuilt
   timelines; a fabricator who escapes that has to hand-write a *coherent*
   timeline, which is harder than doing the work — and reviewers already
   read code; implausible rows read like implausible essays.
3. **The wire is shared with a system that knows the user.** Rows forwarded
   to Hackatime come from the real plugin against a real account, with
   Hackatime-side dedupe and ingest validation. A submitter who exports
   fabricated rows that were never forwarded has a local story that the
   Hackatime ledger doesn't corroborate — cross-checking the two is the
   auditor's move.
4. **Statistical honesty at scale.** The aggregate on the homepage is unreviewed logged
   hours precisely because it needs no trust; the defensible-hours number
   only ever counts after a human review. Fraud caught at review is a
   deflation, not a loss.

So the honest statement of the guarantee: **byline makes faking expensive
and visible, not impossible.** Same as every other measurement system,
including grades, and the fix is the same one schools use — the reviewer,
not the instrument.

## Deliberately not in this artifact

Self-reporting ("how stuck did you feel?"), timers, session start/stop, and
any number that isn't produced by the hours rule. No other scoring path
exists. Nothing new flows through the heartbeat wire — Hackatime's ingest is
untouched, and the artifact travels only via export/submission, never as a
payload Hackatime's ingest must understand.
