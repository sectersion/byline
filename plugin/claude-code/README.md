# byline plugin for Claude Code

A PostToolUse hook that reports Claude Code tool activity to your local
byline daemon, so your agent sessions show up as `ai coding` minutes in
Hackatime alongside your editor time.

**Privacy:** the hook whitelists what it sends: event_id, timestamp, tool
name, file path, and byline's optional `ai_*` counts. `tool_input` and
`tool_response` (which carry file contents and prompt text) are dropped in
the hook before anything is POSTed; the daemon also drops them at parse
time. Nothing is stored outside your machine's sqlite.

## Install

Recommended — one command:

```
byline setup
```

Setup detects Claude Code, asks, then installs `~/.byline/claude-hook.mjs`
and adds the PostToolUse entry to `~/.claude/settings.json`. It prints every
file it touches and is idempotent. `byline setup --no-plugins` skips this.

Manual (if you prefer not to run setup): copy this directory into
`~/.claude/plugins/byline/`, or add the entry from `hooks/hooks.json` to
your `~/.claude/settings.json` `hooks` section, pointing the command at the
copy of `claude-hook.mjs` you kept.

## Verify

1. Start the daemon: `byline serve`
2. Work a Claude Code session that uses at least one tool (an Edit is enough).
3. `byline status` — the unforwarded count rises, then the drain posts it.
4. `byline report` — minutes show as `ai coding`.
5. Hackatime shows the same minutes after the drain (the ledger).

The hook script targets `http://127.0.0.1:8787` by default; set
`BYLINE_HOOK_URL` in the hook command to override.
