# byline

One process, one sqlite file, one input (a local WakaTime heartbeat endpoint),
one output (`byline report` / `export`). Tracks human involvement around AI,
never agent runtime.

## Enforced by budget_test.rs. Do not edit the test to make it pass.
- no file over 400 lines
- total under src/ under 2,500 lines
- max 1 new dependency per PR, added to ALLOWED in budget_test.rs in the same commit
- no new top-level directory, no second crate
- one surface per PR: ingest, hooks, derive, report. Never two.

## Stop and ask. Do not decide these.
- adding any dependency
- any new root directory, crate, or module directory
- any schema change. The 6 tables are hand-written and frozen.
- anything needing a second process, store, or runtime: server, sync, auth,
  wasm, bundler, frontend framework, a second page, a second route
- any file about to pass 300 lines
- any request that sounds like "while you're in there"

## Never
- poll files on disk for AI activity. Signals arrive as pushes from hooks.
- store prompts, code, file contents, keystrokes, or window titles anywhere,
  including logs and error messages.
- store a number that isn't derivable from events. No cached totals.
- merge a migration whose number already exists. List the dir first.
- edit a migration that has already run.
- put CLI parsing, the daemon loop, and service install in one file.
- add a retry, telemetry ping, or update check outside cli/.

## Layout: put the change where it belongs
- cli/        one file per subcommand, under 200 lines
- ingest/     heartbeat endpoint + auth
- hooks/      one file per tool (claude.rs only in v1)
- store/      sqlite, one file per concern, under 400 lines
- derive/     pure functions, events in, minutes out. no I/O.
- report/     text + jsonl export
- budget_test.rs

## Workflow
- every change needs at least 1 issue + 1 PR. Never merge to main.
- keep code clean: separate files, single concern each.

## Naming contract: every tracked item carries a tag
Format: `<PREFIX>-NNN`. Numbers come from the highest existing number in that
prefix + 1 (check `git log --oneline`, issues, and branches before claiming).
Never reuse a number, even for a closed item.

| Prefix | Covers | Example issue | Example branch | Example commit/PR |
|---|---|---|---|---|
| FEAT | new surface or capability | `FEAT-007: byline export` | `feat/byline-export-007` | `FEAT-007: add byline export` |
| FIX | bug fix | `FIX-012: setup clobbered by hackatime-setup` | `fix/setup-clobber-012` | `FIX-012: survive hackatime-setup rewrite` |
| DOCS | doc-only change | `DOCS-003: wire contract` | `docs/wire-contract-003` | `DOCS-003: add wakatime-contract.md` |
| SEC | security/privacy/correctness of data handling | `SEC-001: never log raw payloads` | `sec/no-payload-logs-001` | `SEC-001: redact payloads from logs` |
| CHORE | builds, CI, budget_test, deps, tooling | `CHORE-004: CI on two OSes` | `chore/ci-two-os-004` | `CHORE-004: add GitHub Actions` |

- issue title, branch name, commit subject, PR title, and PR body heading all
  start with the same tag. One item = one tag everywhere; a PR carries the tag
  of the issue it closes (`Closes FEAT-007`).
- a PR touches exactly one prefix. Never two.
- the number is global across prefixes in spirit but tracked per-prefix:
  FEAT-007 has no relationship to FIX-007.
- 3-digit zero-padded always (`FEAT-007` not `FEAT-7`). Past 999 it becomes 4.

## Done means
- cargo test passes, including budget_test
- `byline explain <session>` shows the new behaviour, one reason per minute
- CURRENT.md updated if data flow changed
- new fixture with NOTES.md if hook parsing changed
