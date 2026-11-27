# Claude Code hook payload shape (PostToolUse)

Pinned by FEAT-002 task 1 so `hooks::claude::parse` codes against a fixture
instead of memory. Source: Claude Code hooks docs (hooks reference,
`PostToolUse` event).

Fields Claude Code actually sends on PostToolUse:

- `session_id`, `transcript_path`, `cwd`, `hook_event_name` (`"PostToolUse"`)
- `tool_name` (string)
- `tool_input` (object, tool-specific; for Edit contains `file_path`,
  `old_string`, `new_string` — the last two are file contents)
- `tool_response` (object, tool-specific; for Edit contains `filePath`,
  `structuredPatch`, ...)

No top-level timestamp and no unique event id exist in the payload, so byline
derives both:

- `event_id`: the hook script (later task) may add `"event_id"`; when absent,
  byline uses FNV-1a-64 of the raw payload text (derivable from the event,
  stable across restarts).
- `timestamp`: optional `"timestamp"` top-level field (unix seconds) if the
  hook script adds one; otherwise the store uses its own receive time.

Byline extension fields read when present (the hook script may enrich the
payload before posting): `ai_model`, `ai_input_tokens`, `ai_output_tokens`,
`ai_line_changes` at top level. All optional; absent in the vanilla payload.

Privacy: `tool_input` and `tool_response` are dropped at parse time. Only
`file_path` (as a path), `tool_name`, the `ai_*` counts/names, and `event_id`
survive into storage.
