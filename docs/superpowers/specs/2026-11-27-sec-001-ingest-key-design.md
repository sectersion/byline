# SEC-001 design: ingest validates the key, stops storing it plaintext

Date: 2026-11-27 · Issue: SEC-001 (#14) · Branch: `sec/ingest-key-001`

## Intent and why

The local ingest (127.0.0.1:8787) accepts **any** api key and stores the real
key in plaintext in `heartbeats.api_key`. Any local process can forge
heartbeats, and the sqlite file carries a live credential (backups, sync
tools, anything that reads the db). byline's whole point is careful handling
of human-involvement data; both halves are SEC.

## Goals

- Requests whose key does not match the user's real key are rejected (401)
  and never stored.
- sqlite never again receives the full plaintext key; the existing column
  keeps a last-4-chars fingerprint for provenance only.
- The daemon never bricks: if the expected key cannot be read, requests are
  accepted (accept-any fallback), rows still fingerprinted.

## Non-goals / anti-goals

- No new secret storage (no key copy in config table, no sidecar key file).
- No new dependency, no hand-rolled crypto, no schema change — schema.sql is
  frozen and untouched; the `(needed to forward)` comment is already stale
  (forwarding reads the config table, not the row) and its staleness is noted
  in CURRENT.md.
- Not in scope: wiring setup → config table so forwarding actually fires
  (dormant-forward gap; separate PR).

## Approaches considered

- **A (chosen): validate against `~/.wakatime.cfg`.** The user's real key is
  already on disk there; setup already reads/writes that file; e2e already
  seeds it. serve loads it once at startup and every request is checked
  against it. No new secret copy, no crypto, no migration. Cost: ingest now
  presumes the cfg file exists — it already does (setup's precondition).
- **B: pin on first sight.** First key seen is hashed and pinned in the
  config table. Rejected: needs a hash function (new dep or ~70 lines of
  hand-rolled SHA-256) and still writes a secret-derived blob into sqlite.
- **C: half-fix.** Keep accepting any key, only fingerprint rows. Rejected:
  leaves the forge-from-localhost half open.

## Decisions

- `serve` reads the expected key once at startup via `setup::wakatime_cfg()`
  + `get_ini` (made `pub(crate)`); unreadable/absent → `None`.
- `server::run` / `handle` receive `expected_key: Option<String>` as a
  parameter — tests pass `None` explicitly so they can never read the
  developer's real cfg (the FIX-002 class of bug).
- Mismatch → 401 `{"error":"invalid api_key"}`, zero rows, nothing logged.
- `handle_body` / `handle_single` pass `&key[len-4..]` (empty when key ≤ 4
  chars) to `heartbeats::insert`; `Row.api_key` becomes provenance only.
- e2e `post_bulk` switches to base64 of `e2e-key`, which e2e already seeds
  into its temp `.wakatime.cfg`.

## Left to the builder

- Exact placement of the fingerprint helper and its one test.
- Test names/structure per the existing per-file test style.
- Workflow: issue exists (#14); one PR, branch `sec/ingest-key-001`,
  `Closes SEC-001`; budget_test must stay green.
