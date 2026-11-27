// OpenCode plugin: byline hook for tool.execute.after events.
// Reads the event from the plugin API, whitelists fields, POSTs ONLY the
// path/tool/ai_* counts to the local byline daemon, then passes the original
// event through. A failure here must never block the agent.

import { createHash } from 'node:crypto';

const HOOK_URL = process.env.BYLINE_HOOK_URL ?? 'http://127.0.0.1:8787/api/v1/hooks/opencode';

async function fetchHook(event: any) {
  // OpenCode tool.execute.after gives: { tool, sessionID, callID, args?, output? }
  const { tool, sessionID, callID } = event;
  // Stable id from session+call, matching the byline convention
  let base = `${String(sessionID)}${String(callID)}`;
  let event_id = createHash('sha256').update(base).digest('hex').slice(0, 16);

  // Whitelist: only send normalized fields. metadata.file and metadata.diff
  // are the file-path and line-changes sources; fall back to args.file_path.
  const out: Record<string, any> = {
    event_id,
    timestamp: event.timestamp ?? Date.now() / 1000,
    tool_name: tool,
  };

  // file_path: from metadata.file (edit/write tools) or args.file_path
  const fp = event.metadata?.file?.path ?? event.args?.file_path;
  if (fp !== undefined) out.file_path = fp;

  // ai_*: optional — OpenCode does not expose model/token counts per-tool
  for (const k of ['ai_model', 'ai_input_tokens', 'ai_output_tokens', 'ai_line_changes']) {
    if (event[k] !== undefined) out[k] = event[k];
  }

  try {
    await fetch(HOOK_URL, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(out), signal: AbortSignal.timeout(5000) });
  } catch { /* daemon down: never block the agent */ }
}

export default async ({ client, project, directory, worktree }: any) => ({
  "tool.execute.after": async (input: any, output: any) => {
    try {
      await fetchHook({
        tool: input.tool,
        sessionID: input.sessionID,
        callID: input.callID,
        args: input.args,
        output: output,
      });
    } catch { /* daemon down: never block the agent */ }
  },
});