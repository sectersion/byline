-- docs/schema.sql — frozen after FEAT-001 merges. Any change is a new
-- numbered migration; never edit this file in place again.
CREATE TABLE heartbeats (
  id INTEGER PRIMARY KEY,
  raw TEXT NOT NULL,              -- payload object exactly as received
  received_at INTEGER NOT NULL,   -- unix seconds, byline's clock
  forwarded_at INTEGER,           -- null = queued for upstream
  auth_method TEXT NOT NULL,      -- 'basic' | 'query'
  api_key TEXT NOT NULL           -- the key it arrived with (needed to forward)
);
CREATE TABLE agent_heartbeats (
  id INTEGER PRIMARY KEY,
  raw TEXT NOT NULL,              -- composed agent heartbeat as sent upstream
  received_at INTEGER NOT NULL,
  forwarded_at INTEGER,
  hook_source TEXT NOT NULL       -- e.g. 'claude-code'
);
CREATE TABLE unknown_paths (
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL,
  method TEXT NOT NULL,
  hit_at INTEGER NOT NULL
);
CREATE TABLE config (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE sessions_cache (
  -- placeholder: FORBIDDEN to use for derived numbers; kept empty until a
  -- genuine non-derivable need appears, else dropped at next migration
  id INTEGER PRIMARY KEY
);
CREATE TABLE schema_meta (
  version INTEGER NOT NULL
);
