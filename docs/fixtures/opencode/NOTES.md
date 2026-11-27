# OpenCode hook payload shape (tool.execute.after)

Pinned by FEAT-008 so `hooks::opencode::parse` codes against a fixture
instead of memory. Source: OpenCode plugin API (`tool.execute.after` event).

Fields OpenCode actually sends on `tool.execute.after`:

- `tool` (string, e.g. `"edit"`, `"write"`, `"read"`)
- `sessionID` (string, OpenCode session identifier)
- `callID` (string, unique per tool call within a session)
- `args` (object, tool-specific; for edit contains `file_path`,
  `old_string`, `new_string` — the last two are file contents)
- `output` (object, tool-specific; for edit contains `title`, `output`,
  `metadata` with `file.path`, `diff`, ...)

No top-level timestamp and no unique event id exist in the payload, so byline
derives both:

- `event_id`: the plugin adds one — `sessionID|callID` hashed (stable).
  When absent from the posted body, byline falls back to FNV-1a-64 of the
  raw payload text (stable across restarts).
- `timestamp`: optional `"timestamp"` top-level field (unix seconds) if the
  plugin adds one; otherwise the store uses its own receive time.

Byline extension fields read when present (the plugin may enrich the
payload before posting): `ai_model`, `ai_input_tokens`, `ai_output_tokens`,
`ai_line_changes` at top level. All optional; absent in the vanilla payload
because OpenCode does not expose model/token counts per-tool.

Privacy: `args` and `output` are dropped at parse time. Only
`file_path` (as a path), `tool_name`, the `ai_*` counts/names, and `event_id`
survive into storage.