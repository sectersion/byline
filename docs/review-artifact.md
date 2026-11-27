# The hours rule

How a session of work-with-AI becomes defensible hours — one rule, applied
identically to every session, recomputable by any outside party from the
stored heartbeat rows alone (fields: docs/wakatime-contract.md §5). The
reviewer confirms or deflates, never invents a number.

## The rule

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
