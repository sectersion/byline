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

No top-level timestamp and no unique event id exist in the payload (`tool_use_id`
is Claude Code's id-like field for the tool call, when it sends one), so byline
derives both:

- `event_id`: the hook script adds one — `session_id|tool_use_id` when
  `tool_use_id` is present, else `session_id|tool_name|file_path|Date.now()`
  (millisecond precision). When absent from the posted body, byline falls back
  to FNV-1a-64 of the raw payload text (stable across restarts). Ceiling:
  crash-recovery re-delivery within the same millisecond can still
  double-count; a stable Claude-Code-provided event id would close it.
- `timestamp`: optional `"timestamp"` top-level field (unix seconds) if the
  hook script adds one; otherwise the store uses its own receive time.

Byline extension fields read when present (the hook script may enrich the
payload before posting): `ai_model`, `ai_input_tokens`, `ai_output_tokens`,
`ai_line_changes` at top level. All optional; absent in the vanilla payload.

Privacy: `tool_input` and `tool_response` are dropped at parse time. Only
`file_path` (as a path), `tool_name`, the `ai_*` counts/names, and `event_id`
survive into storage.
