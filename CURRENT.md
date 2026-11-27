# CURRENT.md — data flow

1. Editor plugin (stock WakaTime plugin, `~/.wakatime.cfg` repointed) POSTs
   heartbeats to `http://127.0.0.1:8787/api/v1/users/current/heartbeats.bulk`.
2. byline stores each payload object raw in sqlite (`heartbeats.raw`,
   byte-identical) with `received_at`, auth method, api key. No dedupe.
3. A drain pass posts each unforwarded row's original body to Hackatime with
   the original key; 2xx sets `forwarded_at`. Backoff 2^n capped 3600s.
   Upstream down never fails the local write.
4. Hackatime remains the ledger; it sees the same bytes it always did.
5. `byline report` derives hours per local day per project from the stored
   rows (pure functions, nothing cached).
6. `byline status` prints one line: rows, last-heartbeat age, unforwarded
   count, unknown paths hit.
7. `byline setup` repoints `~/.wakatime.cfg` at byline; `unsetup` restores.
