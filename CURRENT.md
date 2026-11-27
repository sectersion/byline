# CURRENT.md — data flow

1. Editor plugin (stock WakaTime plugin, `~/.wakatime.cfg` repointed by
   `byline setup`) POSTs heartbeats to
   `http://127.0.0.1:8787/api/v1/users/current/heartbeats.bulk`.
2. byline stores each payload object raw in sqlite (`%USERPROFILE%/.byline/byline.db`,
   table `heartbeats`, column `raw` byte-identical) with `received_at`,
   auth method, and the key's last 4 chars only (SEC-001: sqlite never
   holds the plaintext key; the column is provenance). No dedupe. Malformed
   bodies → 400 + debug file, never stored. Unknown paths (anything but the
   two heartbeat POSTs and the `GET /users/current` stub, FIX-004) → proxied
   verbatim to the upstream (method, path with the local `/api/v1` prefix
   dropped, query, body; authenticated with the upstream key, since the
   caller's header carries the local key) and the upstream response returned
   unmodified; upstream down or unconfigured → 404. Either way an
   `unknown_paths` row is recorded. Requests whose key does not match the
   user's real key (read once at startup from `~/.wakatime.cfg`, the file
   setup manages) → 401, nothing read or stored; unreadable cfg → accept-any
   fallback. (docs/schema.sql's `api_key ... (needed to forward)` comment is
   stale: forwarding reads the config table, not the row.)
3. A drain pass posts each unforwarded editor row's original body to the
   upstream (api_url + key recorded by setup) with the original key; 2xx sets
   `forwarded_at`. Agent rows (FEAT-002) are composed into WakaTime
   heartbeats at send time and batched: ≤100 per POST, at most one agent
   POST per 30s (gate stamp in config `agent_last_post`); hook events older
   than 10 min are stale replays and are dropped, never composed. Backoff
   15·2^n capped 3600s, failure count in config. Upstream down never fails
   the local write. Kicked after each write, plus a 30s timer.
4. Hackatime remains the ledger; it sees the same bytes it always did.
5. `byline report` derives hours per local day per project from the stored
   rows (pure functions, nothing cached). Project = explicit `project`
   field, else parent dir of `entity`, else `(unknown)`. Local day is the
   machine's timezone via chrono (FIX-001), correct per instant across DST.
6. `byline status` prints one line: rows, last-heartbeat age, unforwarded
   count, unknown paths hit.
7. `byline setup` repoints `~/.wakatime.cfg` at byline (idempotent, re-claims
   after a foreign rewrite); `unsetup` restores the backup byte-identical.
8. FEAT-002 will add agent heartbeats (hooks → daemon → same forward drain);
   FEAT-003 the hours rule; FEAT-004 the worker.
