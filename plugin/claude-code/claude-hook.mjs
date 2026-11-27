// byline hook for Claude Code (PostToolUse).
// Reads the event JSON from stdin, adds a stable event_id (the daemon's
// dedup key), POSTs ONLY the whitelisted fields to the local byline daemon,
// then passes the original event through on stdout. A failure here must
// never block the agent.
import { createHash } from 'node:crypto';

const HOOK_URL = process.env.BYLINE_HOOK_URL ?? 'http://127.0.0.1:8787/api/v1/hooks/claude';

const chunks = [];
for await (const c of process.stdin) chunks.push(c);
const text = Buffer.concat(chunks).toString('utf8');
let ev = null;
try { ev = JSON.parse(text); } catch { /* not JSON: forward as-is */ }

// Stable id: session + tool call when the payload carries one, else
// session + whole-second timestamp. Rounded so retries in the same second
// dedup instead of double-counting. Number() guards against an ISO-string
// timestamp (NaN would collapse every event onto the same id).
if (ev && !ev.event_id) {
  const ts = Math.round(Number(ev.timestamp) || Date.now() / 1000);
  const base = ev.tool_call_id ? `${ev.session_id}|${ev.tool_call_id}` : `${ev.session_id}|${ts}`;
  ev.event_id = createHash('sha256').update(String(base)).digest('hex').slice(0, 16);
}

// Whitelist: tool_input / tool_response (file contents, prompts) never leave
// this process. Only the fields the daemon parses are sent. file_path lives
// inside tool_input on the wire; lift it out.
if (ev) {
  const out = { event_id: ev.event_id, timestamp: ev.timestamp, tool_name: ev.tool_name };
  const fp = ev.file_path ?? ev.tool_input?.file_path;
  if (fp !== undefined) out.file_path = fp;
  for (const k of ['ai_model', 'ai_input_tokens', 'ai_output_tokens', 'ai_line_changes']) {
    if (ev[k] !== undefined) out[k] = ev[k];
  }
  try {
    await fetch(HOOK_URL, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(out) });
  } catch { /* daemon down: never block the agent */ }
}

console.log(text);
