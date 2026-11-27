# WakaTime / Hackatime wire contract

Evidence base: source reads at **wakatime-cli v2.28.4** (tag, commit `9fee1b4`) and
**hackatime** HEAD `f4e2710` (no release tags exist; latest verified 2026-11-23) and
**hackatime-setup v1.5.27** (commit `c83d140`). Claims cite `file:line`. Live traffic
captures were not taken this session — see UNVERIFIED.

## 1. api_url

- wakatime-cli default: `https://api.wakatime.com/api/v1`, baked into `BaseURL`
  (`pkg/api/api.go:16`). Endpoint paths are appended with no joining logic:
  `c.baseURL + "/users/current/heartbeats.bulk"` (`pkg/api/heartbeat.go:201`).
- **The api_url includes the version path prefix.** A proxy must be given the full
  prefix, because the client appends bare paths to it.
- A real Hackatime user's `api_url` is `https://hackatime.hackclub.com/api/hackatime/v1`
  — written by hackatime-setup (`src/main.rs:29,275-279`). Routes live under
  `namespace :api do namespace :hackatime do namespace :v1` (`config/routes.rb:352-361`).
  This is **not** `/api/v1` and **not** the domain root.
- Overridable: `--api-url` flag (`cmd/root.go:60-64`), config keys
  `api_url`/`api-url`/`apiurl`/`settings.api_url` (`pkg/params/params.go:465`), and
  per-project `api_urls` map (`params.go:405-455`). A per-heartbeat `APIURL` field
  overrides the client baseURL (`heartbeat.go:196-199`).
- Custom api_url never gets the DNS-fallback-to-IP behavior (`api.go:71-73`).

## 2. Endpoint table

Client → server. `{base}` = api_url as configured.

| # | Method | Path | Trigger | Auth | Request | Response | Evidence |
|---|---|---|---|---|---|---|---|
| 1 | POST | `{base}/users/current/heartbeats.bulk` | every send; the only send path | Basic | JSON array of heartbeats (≤10 per send, grouped by api_url+key) | see §4 | `heartbeat.go:201`, `offline.go:41` |
| 2 | GET | `{base}/users/current/statusbar/today` | `--today` | Basic | — | statusbar JSON | `pkg/api/statusbar.go:19` |
| 3 | GET | `{base}/users/current/goals/{id}` | `--today-goal <id>` | Basic | — | goal JSON | `pkg/api/goal.go:19` |
| 4 | POST | `{base}/users/current/file_experts` | `--file-experts` | Basic | file path(s) | experts JSON | `pkg/api/fileexperts.go:24` |
| 5 | POST | `{base}/plugins/errors` | diagnostics on panic/error | none shown | error JSON | — | `cmd/run.go:358-380`, `pkg/api/diagnostic.go:27-79` |

**There is no single-heartbeat endpoint and no /durations or summaries call in
wakatime-cli.** The single-heartbeat POST exists server-side at
`POST {base}/users/:id/heartbeats` (used by hackatime-setup's test heartbeat,
`send_test_heartbeat`, `main.rs:63-94`) but the CLI never calls it.
Hackatime additionally serves: `GET {base}/users/:id` (returns that key's user,
ignoring `:id`), `GET {base}/users/:id/summaries`, `GET {base}/users/:id/statusbar/today`,
`GET {base}/users/current/stats/last_7_days` (`config/routes.rb:352-361`,
`hackatime_controller.rb:17-31`). `.bulk` is not a separate route — Rails parses
`.bulk` as a format param (`hackatime_controller.rb:59`).

## 3. Auth

- wakatime-cli emits `Authorization: Basic base64(key)` where the base64 encodes
  **the raw key alone, no `username:` separator, no empty-password colon**
  (`pkg/api/heartbeat.go:436-440` → `pkg/api/auth.go:148-152`, which base64s
  `Secret` when `User` is empty). The spec's "empty password" framing was wrong.
- Hackatime accepts all three: `Bearer <key>`, `Basic <base64>` (strict-decodes the
  whole payload as the token), and `?api_key=<key>` query
  (`app/controllers/concerns/user_api_authentication.rb:17,29-35`; sources ordered
  `%i[bearer basic query]` at `hackatime_controller.rb:245-248`).
- wakatime-cli **never** sends `?api_key=` (`pkg/api/statusbar.go:28-29` builds an
  empty query). Invalid key → CLI exits before HTTP (`params.go:47` UUID-v4 regex);
  invalid key server-side → `401 {"error":"Unauthorized"}`
  (`render_helpers.rb:15-17`).

## 4. Responses (bulk)

Success for the client = HTTP **201 or 202** only (`pkg/api/heartbeat.go:259-260`).

- Hackatime bulk success: `201` with body
  `{"responses": [[heartbeatAttrs, 201], ...]}` — one inner `[attrs, status]` pair
  per heartbeat, same order as sent, **201 even when some items failed**; failed
  items are `[{"error": msg, "type": errClass}, 422]` (`hackatime_controller.rb:67,228,238`).
- Single success: `202` with the heartbeat attrs (`hackatime_controller.rb:74`).
- Client per-item handling (`pkg/offline/offline.go`, `handleResults`): per-item
  **400 → dropped, never retried**; any other non-2xx per-item status → requeued and
  the run stops; heartbeats missing from the response → requeued. Order matched by
  `Heartbeat.ID()` fingerprint when the echo is absent.
- Client HTTP-level: 401 → `ErrAuth`, 400 → `ErrBadRequest`, everything else
  generic error → whole batch requeued. **No status code causes the client to give
  up** — 401/402/403/429 all mean "retry later". Client backoff: `15 × 2^retries`
  seconds, cap 3600s, persisted in `~/.wakatime/wakatime-internal.cfg`
  (`pkg/backoff/backoff.go:19-24,80-114`).
- Bulk cap server-side: **100 heartbeats/request** → error render otherwise
  (`hackatime_controller.rb:14,62-65`). Client sends ≤10 anyway (`offline.go:41`).

**Proxy rule:** byline should ack 201 with a well-formed `{"responses":[[...]]}`
body promptly — a malformed or slow response trains the client into backoff while
byline looks healthy.

## 5. Heartbeat payload

Bare JSON array of objects (`pkg/api/heartbeat.go:227`); fields from
`pkg/heartbeat/heartbeat.go:22-57`:

- Always sent: `time` (**float64 epoch seconds**), `entity`, `type`, `user_agent`.
- Optional (omitempty): `ai_line_changes`, `ai_cached_input_tokens`, `ai_session`,
  `ai_subscription_plan`, `ai_input_tokens`, `ai_output_tokens`, `ai_prompt_length`
  (ints), `branch`, `category`, `cursorpos`, `dependencies`, `is_write`, `language`,
  `lineno`, `lines`, `human_line_changes`, `project`, `project_root_count`.
- Never serialized: `APIKey`, `APIURL`, `LocalFile`, overrides (`json:"-"`).
- Hackatime's accepted field set (`HEARTBEAT_KEYS`, `hackatime_controller.rb:7-12`)
  overlaps but is **not identical** (e.g. no `dependencies`; adds `plugin`, `machine`,
  `operating_system`, `created_at`). Server normalizes `time` scales
  (ms/µs/ns/FILETIME → seconds, `heartbeat_ingest.rb:388-407`) and dedupes on
  `fields_hash` — duplicates are accepted, not failed (`heartbeat_ingest.rb:151-190`).

## 6. Config table

`~/.wakatime.cfg`, INI, keys read under `[settings]` (`pkg/ini/ini.go:196`).
File resolution: `--config` → `$WAKATIME_HOME/.wakatime.cfg` → `~/.wakatime.cfg` →
`$XDG_CONFIG_HOME/wakatime/wakatime.cfg` (`cmd/root.go:79-83`).

| Key | Default | Overridden by |
|---|---|---|
| `api_url` (+ alias spellings) | `https://api.wakatime.com/api/v1` | `--api-url` flag, `api_urls` per-project map |
| `api_key` (+ aliases) | — | `--key` flag, `WAKATIME_API_KEY` env, vault cmd (`params.go:573-598`) |
| `api_urls` | — | per-project regex→url/key override |
| `heartbeat_rate_limit` | 120s between sends (`params.go:941-945`) | config |
| `sync_offline_activity` | 1000 | config |
| `offline-queue-file` | `~/.wakatime/offline_heartbeats.bdb` | config |
| `timeout` | 120s (`api.go:22`) | config |
| `proxy`, `no_ssl_verify`, `ssl_certs_file` | — | config |
| `hostname`, `plugin`, `hide_*` | — | config |

Env vars: **only** `WAKATIME_HOME` and `WAKATIME_API_KEY` exist — there is no
`WAKATIME_API_URL` (no `BindEnv`/`AutomaticEnv` in the repo). `HTTP_PROXY` etc.
honored via `httpproxy.FromEnvironment`.

**Editor shadowing:** the CLI mechanism is the `--api-url` flag, which an editor
plugin can pass. Whether any given plugin exposes it in its own settings is
**UNVERIFIED** (below) — this is the risk that makes byline look healthy while
seeing nothing.

## 7. Offline queue

- **bbolt database**, not JSON lines: `~/.wakatime/offline_heartbeats.bdb`
  (overridable), bucket `heartbeats`, values are JSON-marshaled Heartbeat structs
  (`pkg/offline/offline.go:28-29,96-113`).
- Replay: every heartbeat invocation pops up to `sync_offline_activity` (default
  1000) queued heartbeats and sends them in batches of 10 before fresh heartbeats
  (`offline.go:~145-192`).
- Dedupe: `DeleteDuplicates()` on pop removes same-entity entries within a
  1-second window — **within the queue only; no dedupe against the fresh batch**.
- Pop commits before send: crash between pop and successful send loses those
  heartbeats client-side; failed sends requeue. At-least-once with a loss window.
- **Implication for byline:** the client already tolerates duplicates server-side
  (Hackatime dedupes on `fields_hash`) and already tolerates loss on crash. Byline
  forwarding with its own `forwarded_at`-null queue composes fine; the double-send
  risk is bounded by Hackatime's dedupe. Do not add byline-side dedupe — it would
  be a third copy of the same logic.

## 8. hackatime-setup coexistence

- Installs its own CLI + editor extensions (`hackatime.hackatime-time-tracker` for
  VS Code), not wakatime-cli (`install.sh:56-69`, `src/editor_plugins/vscode.rs:9`).
- **Rewrites `~/.wakatime.cfg` as a full-file replacement** — a fresh `Ini` not
  merged with existing content (`src/main.rs:96-127,275-279`): sets `api_url` to
  `https://hackatime.hackclub.com/api/hackatime/v1`, `api_key`, and
  `heartbeat_rate_limit_seconds = 30`. One-shot installer; re-running it re-prompts
  and rewrites, defaulting to yes (`main.rs:262-273`). Nothing polls the file later.
- It also merge-aware-writes `~/.wakatime/wakatime-internal.cfg` (`main.rs:199-204`).
- **Conflict surface is exactly `~/.wakatime.cfg`.** If byline's setup repoints
  `api_url` at the local daemon and the user later re-runs hackatime-setup, the file
  is overwritten and heartbeats go straight to Hackatime again — byline sees
  nothing. Byline's `byline status` should therefore surface last-heartbeat age
  prominently (already specced), and byline's setup should be idempotent/re-runnable
  but not assume it is the only writer.

## UNVERIFIED

- **Live traffic captures** (the brief's step 1): not taken — requires a human
  editing a file in VS Code with the plugin while a logging listener runs. What I
  tried instead: full source reads at a tagged release of both sides, cross-checked.
  Recommended capture recipe for the next session: `wakatime-cli --verbose --logfiles`
  pointed at a `nc -l`/tiny logger; keep files in `research/captures/`.
- Whether the VS Code / JetBrains plugins override `api_url` in their own settings
  (passing `--api-url`) rather than relying on the cfg file. The CLI mechanism
  exists; plugin defaults were not read.
- wakatime-cli behavior between v2.9 and v2.28 (intermediate versions).
- `GET /durations`: absent from both repos read; if any older client calls it,
  Hackatime 404s it (route absence — inferred, not tested).
- Response body of `POST {base}/plugins/errors` (does Hackatime accept it at all —
  no such route found in routes.rb).

## Decisions — RESOLVED 2026-11-26

1. **Local api_url shape.** Resolved: `http://127.0.0.1:8787/api/v1`. Serve
   endpoints under the `/api/v1` prefix. Must never change once shipped.
2. **Auth parsing.** Resolved: Basic header is `base64(key)` with **no colon** —
   decode the whole payload as the key, never split on `:`.
3. **Ack body.** Resolved: HTTP 201 with `{"responses":[[<heartbeat>,201],...]}`,
   one entry per heartbeat, in order. Echo the upstream verdict into the stored
   row once forwarding completes.
4. **Null-project heartbeats.** Resolved: group by the **parent directory name**
   of `entity`. Never drop them.
5. **hackatime-setup coexistence.** Resolved: `byline setup` prints the warning
   and requires acknowledgement during interactive setup, and warns the user
   (via stale last-heartbeat age) when the redirect stops working.
