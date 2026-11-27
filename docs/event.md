# The heartbeat envelope

What byline stores in `heartbeats.raw` — the payload object exactly as
received (docs/wakatime-contract.md §5). Fields the editor plugin may send:

Always sent:
- `time` — float64 epoch seconds (the user's local clock; report groups by
  the local day of this, not of `received_at`)
- `entity` — file path or other entity identifier
- `type` — usually `file`
- `user_agent` — plugin string

Optional (omitempty — absent unless set):
- `ai_line_changes`, `ai_input_tokens`, `ai_output_tokens`,
  `ai_cached_input_tokens`, `ai_session`, `ai_subscription_plan`,
  `ai_prompt_length`
- `human_line_changes`
- `branch`, `category`, `cursorpos`, `dependencies`, `is_write`, `language`,
  `lineno`, `lines`, `project`, `project_root_count`

Unknown extra fields: stored verbatim inside `raw`, never parsed into
columns, never echoed to logs. Hackatime's ingest accepts a similar
(overlapping, not identical) set; byline forwards the original bytes
untouched, so whatever arrives keeps flowing.

`agent_heartbeats.raw` holds the same shape, composed by the daemon from
hook events (FEAT-002), always with `category: "ai coding"`.

Stored alongside the payload: `received_at` (byline's clock), `auth_method`
(`basic` or `query`), `api_key` (needed to forward), `forwarded_at`
(null = queued).
