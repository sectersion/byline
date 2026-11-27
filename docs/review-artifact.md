# The review artifact

What a reviewer reads to approve or deflate a session of work-with-AI, and the
signals in byline that produce each line of it. Design doc; no code.

Everything here derives from what byline already stores: the raw heartbeat
payload (`docs/wakatime-contract.md` §5). No new signal source, no second
process. If a claim below can't be made from heartbeats, the artifact says so
instead of guessing — that honesty is the product.

## What the reviewer sees

One block per session (a session = heartbeats with gaps under 30 minutes,
followed by a 30-minute gap — a derivation, not a stored number). The block:

```
SESSION 14:32–17:06 · shipyard-tui · 2h 34m counted
Attempted:    Rust, TUI table rendering (14 files, 3 in src/render/)
Stuck:        40 min — editing widgets/scroll.rs, 0 writes, 9 file switches,
              then 22-min silence
Resolved:     back to scroll.rs, 61 human line changes over 18 min
Agent work:   74 AI line changes, 2 AI sessions (claude-code), 11k output tokens
Human work:   130 human line changes, 0 unreviewed AI dumps
Verdict:      DEFEND FULL — sustained human edits before and after agent
              assistance; struggle followed by resolution on the same files.
              Evidence: 3 heartbeat groups, coverage 96%.
```

Ten hours of work is therefore ~4–6 session blocks. Reading one block is
seconds; the reviewer's judgment happens on the ones where the verdict is weak.

## Which signal produces which claim

| Claim | Signal (heartbeat fields) | Rule |
|---|---|---|
| Attempted | `project`, `language`, `entity`, `type` | distinct entities and languages touched; project per the frozen decision (parent directory when `project` is null) |
| Stuck | inter-heartbeat time deltas, `is_write`, `entity` churn | a window (≥10 min) with heartbeats arriving (so the editor is open) but ~zero `human_line_changes` **and** `ai_line_changes`, plus repeated switching between the same few entities; a silent gap ≥20 min *inside* the window extends it |
| Resolved | `human_line_changes` (or `ai_line_changes`) after a stuck window | sustained positive changes on ≥1 entity that appeared in the stuck window |
| Agent vs human | `ai_line_changes` vs `human_line_changes`, `ai_session`, `ai_model`, `ai_output_tokens` | reported as raw counts per session; "unreviewed dump" = AI changes landed with zero human changes within ±15 min in the same files |
| Coverage | heartbeats per wall-clock minute vs the session window | every block carries it; it is the confidence number, not decoration |

All rules above are derivations over stored rows — nothing cached, per the
frozen rule. The exact thresholds are tuning knobs, marked `ponytail:`
constants in code when implemented.

## What byline says when evidence is thin

The artifact distinguishes three thinness levels and never blurs them:

- **Coverage < 70%:** the block header says `coverage N% — the rest is
  silent`, and the verdict is capped at PARTIAL. Silence is not absence of
  work, but byline cannot claim it either.
- **A session with heartbeats but no change fields at all** (e.g. a plugin
  version that doesn't send `*_line_changes`): claims degrade to Attempted
  only. The stuck/resolved/agent lines print `no change data from this
  plugin`, not a zero — a zero is a measurement, a missing field is not.
- **Pre-1.0 plugins / unknown fields:** the block prints which fields were
  absent, so the reviewer knows the difference between "the human produced
  nothing" and "byline never received the human signal."

## The three cases byline must refuse to defend

Refusing is the artifact's value — a recommendation of FULL WEIGHT on
unfounded sessions is the exact deflation the thesis describes.

1. **Agent-authored, never touched.** The session shows `ai_line_changes`
   (or AI output tokens) but zero `human_line_changes` anywhere in it, and no
   human heartbeat within 15 minutes of any AI change. Byline refuses the
   verdict entirely: it prints `cannot defend — no human edit detected in
   session` and the reviewer sees why instead of a number.
2. **The phantom project.** Hours land on a project whose `entity` paths
   don't exist in a coherent tree — e.g. everything under a single generated
   directory, or `entity` values that are all non-file types (`type: app`,
   `domain`) with no file writes. Byline prints `cannot defend — no file
   writes in session`; browsing isn't building.
3. **The rebuilt session.** Heartbeat timestamps arrive out of order,
   duplicated, or with epochs the contract doc's repair rules would rewrite
   (ms/µs/FILETIME scales — `docs/wakatime-contract.md` §5). Byline prints
   `cannot defend — timeline integrity failure` rather than deriving struggle
   curves from a timeline it doesn't trust.

In all three the hours still count as *logged* locally (storage is never
withheld); they are flagged as *indefensible* for submission. The distinction
is the privacy guarantee in the brief: local truth, submitted evidence.

## Worked example

Reconstructed from a real 3-hour evening on a real project, replayed through
the fields the contract says the CLI actually sends — not live-captured.
Shapes are exact; this is what the artifact would print.

```
SESSION 19:02–21:47 · markdown-lint · 2h 51m counted · coverage 91%
Attempted:    Rust, CLI argument parsing (7 files, 1 in tests/)
Stuck #1:     26 min — args.rs open, 2 human line changes, 6 switches
              between args.rs and mod.rs, then 14-min silence
Resolved #1:  args.rs, 22 human line changes in 9 min
Agent work:   210 AI line changes, 1 session (claude-code), 41k output tokens,
              all within cli/report.rs
Human work:   96 human line changes spread across 5 files, 3 within ±10 min
              of the AI changes
Verdict:      PARTIAL — agent concentrated in one file the human edited
              within 10 min afterwards (review pass plausible), but 78 of
              the 210 AI lines landed in the final 20 min with no human
              heartbeat after them. Defensible: 1.6 h (the 96 human
              minutes). Flagged for review: the trailing agent block.
```

The reviewer's two-minute read: human work is continuous and interleaved with
the agent except at the end. They deflate the tail, approve the rest — an
outcome the raw logged hours would have hidden.

## What the reviewer still checks by hand

Byline is evidence assembly, not a judge. It cannot check:

- Whether the code **runs and ships** — deployment/repo state is outside
  byline's input; "shipped" comes from the project record on the server, not
  the daemon.
- Whether the author is who the API key says — the daemon has no identity
  beyond the API key; authorship of the underlying repo is the server's
  problem.
- Whether the "stuck" window was struggle or **lunch** — a 22-minute silence
  is indistinguishable from being away; only the human knows. The artifact
  says "silence," never "stuck," on those windows — stuck requires the
  file-switching pattern, and even then it's a description, not a diagnosis.
- Whether AI changes were actually **read** by the human (interleaving is
  evidence, not proof — a fast approval session can fake cadence).
- Coverage gaps themselves: 9% of the worked session is unexplained, and no
  artifact fixes that.

## The hours rule (how an hour is actually graded)

One rule, applied identically to every session, recomputable by any outside
party from the stored heartbeat rows alone. The reviewer's job is to confirm
or deflate, never to invent a number. The output is **defensible hours** —
a plain hour count, no invented unit.

**Step 1 — classify each counted minute of the session** (a minute inherits
the classification of the heartbeats in it):

| Minute class | Test (from stored rows) | Credit |
|---|---|---|
| HUMAN | ≥1 heartbeat with `human_line_changes` > 0 | **1.0 min** |
| STRUGGLE | falls inside a stuck window that later resolved on the same files (the stuck/resolved rule above) | **1.0 min** |
| AGENT-REVIEWED | AI change with a human heartbeat in the same files within ±15 min | **0.5 min** |
| AGENT-UNREVIEWED | AI change with no such human heartbeat | **0 min** |
| SILENT | no heartbeat | **0 min** |

**Step 2 — sum into hours.** `defensible hours = (Σ credited minutes) / 60`,
capped at the session's wall-clock hours. No double counting: STRUGGLE beats
AGENT-* in a minute; HUMAN beats STRUGGLE.

**Step 3 — the verdict derives from the ratio:**

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

## Review flow

- **Channel (v0.0.1):** the teen runs `byline export <project>` and attaches the
  evidence block to their submission wherever the program's flow accepts text.
  No new server surface.
- **Channel (worker):** `byline submit` POSTs the evidence block to the byline
  worker, which stores it at a URL the teen pastes into their Hackatime
  submission. Still nothing through Hackatime's ingest. Native Hackatime
  integration is ruled out.
- **The ruling:** reviewer reads the block, checks the defensible-hours line,
  confirms or deflates downward with a fixed reason. The hours rule gives the
  starting number; the three refusal cases say when to zero it; the hand-check
  list says what the rule cannot see.
- **Audit:** the block is self-attested until recomputed. The rule is pure and
  the rows are the evidence — a doubtful reviewer asks for the raw heartbeats
  and re-runs the rule (see Evidence, below).

## Worker: the hero number

**On by default** (opt-out with one flag): the daemon POSTs one number — total
**unreviewed logged hours** — to the byline worker, which shows the aggregate
on the homepage as the live hero. It ships on because it is anonymous and is
just a number: no rows, no projects, no timestamps, no identity, so the
privacy line (logged activity stays local) holds — a count is the one thing
that can cross without leaking work. The MVP for the whole system is
tracking itself: byline's own development, tracked by byline.

## Evidence, and the fact that it can be faked

"Evidence" here means the stored heartbeat rows — that is all it is. The
evidence block is a rendering of them, and the hours rule is a pure function
over them. So the fake question is exactly: can a user fabricate rows?

**Yes.** The daemon is a local HTTP server; anyone can POST synthesized
heartbeats to 127.0.0.1:8787 with plausible `human_line_changes` spread over
an evening. No local scheme — hashing, signing, obfuscation — changes that:
the attacker owns the machine the signatures are made on. Anyone claiming
otherwise is selling something. What defends the number is what happens at
review:

1. **Faking has no upside for honest users.** HUMAN and STRUGGLE minutes
   already credit at 1.0. Fabrication only pays someone who has no honest
   hours — and the program's own acceptance bar (reviewed *and shipped*)
   still requires a real project at the end.
2. **Review recomputes.** A doubtful reviewer takes the raw rows (export
   includes them) and re-runs the rule. Fabricated rows must stay internally
   consistent: timestamps ordered, file paths real, language matching file
   extensions, line-change counts summing plausibly, coverage sane. The
   daemon itself refuses to defend rebuilt timelines; a fabricator who
   escapes that has to hand-write a *coherent* timeline, which is harder
   than doing the work — and reviewers already read code; implausible rows
   read like implausible essays.
3. **The wire is shared with a system that knows the user.** Rows forwarded
   to Hackatime come from the real plugin against a real account, with
   Hackatime-side dedupe and ingest validation. A submitter who exports
   fabricated rows that were never forwarded has a local story that the
   Hackatime ledger doesn't corroborate — cross-checking the two is the
   auditor's move.
4. **Statistical honesty at scale.** The hero number is unreviewed logged
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
