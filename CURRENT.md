# CURRENT.md — data flow

1. Editor plugin (stock WakaTime plugin, `~/.wakatime.cfg` repointed by
   `byline setup`) POSTs heartbeats to
   `http://127.0.0.1:8787/api/v1/users/current/heartbeats.bulk`.
2. byline stores each payload object raw in sqlite (`%USERPROFILE%/.byline/byline.db`,
   table `heartbeats`, column `raw` byte-identical) with `received_at`,
   auth method, api key. No dedupe. Malformed bodies → 400 + debug file,
   never stored. Unknown paths → 404 + `unknown_paths` row.
3. A drain pass posts each unforwarded row's original body to the upstream
   (api_url + key recorded by setup) with the original key; 2xx sets
   `forwarded_at`. Backoff 15·2^n capped 3600s, failure count in config.
   Upstream down never fails the local write. Kicked after each write,
   plus a 30s timer.
4. Hackatime remains the ledger; it sees the same bytes it always did.
5. `byline report` derives hours per local day per project from the stored
   rows (pure functions, nothing cached). Project = explicit `project`
   field, else parent dir of `entity`, else `(unknown)`. Local day uses
   `BYLINE_TZ_OFFSET` (seconds east of UTC), else UTC.
6. `byline status` prints one line: rows, last-heartbeat age, unforwarded
   count, unknown paths hit.
7. `byline setup` repoints `~/.wakatime.cfg` at byline (idempotent, re-claims
   after a foreign rewrite); `unsetup` restores the backup byte-identical.
8. FEAT-002 will add agent heartbeats (hooks → daemon → same forward drain);
   FEAT-003 the hours rule; FEAT-004 the worker.
