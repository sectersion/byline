# byline plugin for OpenCode

A `tool.execute.after` plugin that reports OpenCode tool activity to your local
byline daemon, so your agent sessions show up as `ai coding` minutes in
Hackatime alongside your editor time.

**Privacy:** the plugin whitelists what it sends: `event_id`, `timestamp`,
`tool_name`, `file_path`, and byline's optional `ai_*` counts. `args` and
`output` (which carry file contents and prompt text) are dropped in the hook
before anything is POSTed; the daemon also drops them at parse time. Nothing
is stored outside your machine's sqlite.

## Install

Recommended — one command:

```
byline setup
```

Setup detects OpenCode (checks `~/.config/opencode/`), asks, then installs the
plugin to `~/.config/opencode/plugins/byline.ts`. Idempotent — re-running
does nothing. `byline setup --no-plugins` skips this.

Manual (if you prefer not to run setup):

1. Ensure the `tool.execute.after` plugin API is enabled in OpenCode (it is by default).
2. Place `byline.ts` in the OpenCode plugins directory
   (`~/.config/opencode/plugins/byline.ts`).
3. Add a PostToolUse entry to `~/.claude/settings.json`:

```json
{ "matcher": "*", "hooks": [ { "type": "command", "command": "node ~/.config/opencode/plugins/byline.ts" } ] }
```

(point the command at the copy of `byline.ts` you kept).

## Verify

1. Start the daemon: `byline serve`
2. Work an OpenCode session that uses at least one tool (an Edit is enough).
3. `byline status` — the unforwarded count rises, then the drain posts it.
4. `byline report` — minutes show as `ai coding`.
5. Hackatime shows the same minutes after the drain (the ledger).

The plugin targets `http://127.0.0.1:8787` by default; set
`BYLINE_HOOK_URL` in the hook command to override.