# SEC-001 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The ingest rejects keys that don't match the user's real key and stores only a last-4 fingerprint, never plaintext.

**Architecture:** serve loads the expected key once from `~/.wakatime.cfg` (the file setup already manages) and threads `expected_key: Option<String>` through `server::run`/`handle`; a mismatched key gets 401 with zero rows and no logging. `handle_body`/`handle_single` pass a fingerprint instead of the raw key to `heartbeats::insert`.

**Tech Stack:** Rust, rusqlite, tiny_http (all existing deps; no additions).

**Spec:** `docs/superpowers/specs/2026-11-27-sec-001-ingest-key-design.md`

## Global Constraints

- No new dependency, no schema change, no edit to `docs/schema.sql`.
- No file over 400 lines; none over 300 without stopping; budget_test must pass.
- Commit subject and PR title start with `SEC-001`; branch `sec/ingest-key-001`; PR closes #14.
- Never log or write raw payloads or keys anywhere, including errors.

## Review Focus

- Request with missing/bad auth while a key is expected → 401, not 500, nothing stored. Test: `wrong_key_is_401_no_rows` (Task 1).
- Key ≤ 4 chars → fingerprint stored empty, never partial plaintext. Test: `fingerprint_caps_short_keys` (Task 2).
- Multibyte `?api_key=` value → fingerprint slicing must not panic mid-char. Test: same test asserts `fingerprint("héllo")` doesn't panic and never splits a char (Task 2).
- No cfg readable → accept-any fallback (server tests already exercise `None`; e2e exercises the loaded path).
- e2e posts `e2e-key` (its own seeded cfg key) → accepted and forwarded. Test: `full_cycle` still passes (Task 1).

---

### Task 1: expected-key check — 401 on mismatch

**Files:**
- Modify: `src/ingest/server.rs` (run, handle, tests' `start`)
- Modify: `src/cli/serve.rs` (load key, pass through)
- Modify: `src/cli/setup.rs:57` (`get_ini` → `pub(crate)`)
- Modify: `tests/e2e.rs` (`post_bulk` auth header)

**Interfaces:**
- Produces: `server::run(port: u16, store: Arc<Mutex<Store>>, expected_key: Option<String>) -> !`; `handle(req, store, expected_key: &Option<String>)`. `cli::serve::run` loads the key and calls `run` with `Some`/`None`.
- Consumes: `setup::wakatime_cfg() -> PathBuf` (already pub), `get_ini(content: &str, key: &str) -> Option<String>` (made pub(crate) in this task).

- [ ] **Step 1: Write the failing test** in `src/ingest/server.rs` tests — a wrong key is refused with no row:

```rust
const WRONG: &str = "Authorization: Basic c3Bvb2Yta2V5\r\n"; // base64("spoof-key")

#[test]
fn wrong_key_is_401_no_rows() {
    let (store, _db) = test_store("wrongkey");
    start(19877, store.clone(), Some("key-uuid".to_string()));
    let body = r#"[{"time":1.0,"entity":"e","type":"file","user_agent":"u"}]"#;
    let raw = format!("POST {BULK_PATH} HTTP/1.1\r\nHost: x\r\n{WRONG}Content-Length: {}\r\n\r\n{body}", body.len());
    let (code, resp) = send(19877, &raw);
    assert_eq!(code, 401, "code={code}");
    assert!(resp.contains("invalid api_key"), "resp={resp}");
    assert_eq!(heartbeats::unforwarded(&store.lock().unwrap(), 10).unwrap().len(), 0);
}
```

- [ ] **Step 2: Run it — expect compile error** (signature mismatch), then after threading the param through, red→green on behavior. Run: `cargo test wrong_key_is_401_no_rows` → first FAIL to compile, then pass once Step 3 lands.

- [ ] **Step 3: Implement**

- `server.rs`: add `expected_key: Option<String>` param to `run` and `handle`; before the route match, if `expected_key` is `Some(e)` and `auth.key() != Some(e)` return `(401, r#"{"error":"invalid api_key"}"#)` without storing anything (and without `write_debug`). Existing tests' `start` passes `None`.
- `cli/serve.rs`: in `run()`, load the key:
  ```rust
  let expected = std::fs::read_to_string(crate::cli::setup::wakatime_cfg()).ok()
      .and_then(|c| crate::cli::setup::get_ini(&c, "api_key"));
  crate::ingest::server::run(port, Arc::new(Mutex::new(store)), expected);
  ```
- `cli/setup.rs`: change `fn get_ini` to `pub(crate) fn get_ini`.
- `tests/e2e.rs:117`: replace the auth header's base64 with `ZTJlLWtleQ==` (base64 of `e2e-key`, which e2e seeds at line 175).

- [ ] **Step 4: Verify** — `cargo test` all green (unit + e2e).

- [ ] **Step 5: Commit** — `SEC-001: 401 when the key is not the user's`

### Task 2: fingerprint — sqlite never sees the plaintext key

**Files:**
- Modify: `src/ingest/mod.rs` (fingerprint + both call sites)

**Interfaces:**
- Produces: `fn fingerprint(key: &str) -> &str` in `src/ingest/mod.rs` — last 4 bytes when the key is longer than 4 bytes and ends on a char boundary, else `""`.
- Consumes: `heartbeats::insert(&Store, raw, AuthMethod, api_key)` unchanged — callers now pass the fingerprint.

- [ ] **Step 1: Write the failing test** in a new `#[cfg(test)] mod tests` in `src/ingest/mod.rs`:

```rust
#[test]
fn fingerprint_caps_short_keys() {
    assert_eq!(fingerprint("abcdefgh"), "efgh");
    assert_eq!(fingerprint("abc"), "");       // short: nothing, never partial plaintext
    assert_eq!(fingerprint(""), "");          // empty auth present in code today
    assert_eq!(fingerprint("héllo"), "");     // multibyte boundary: no panic, no split char
}
```

- [ ] **Step 2: Run** `cargo test fingerprint_caps_short_keys` → FAIL (function not defined).

- [ ] **Step 3: Implement** — use `key.get(key.len().saturating_sub(4)..).unwrap_or("")` but return `""` when `key.len() <= 4`; both call sites (`handle_body` line ~47, `handle_single` line ~63) pass `fingerprint(key)` to `insert`.

- [ ] **Step 4: Run** `cargo test` — all green (also confirms `store::heartbeats` tests unaffected).

- [ ] **Step 5: Commit** — `SEC-001: store only a last-4 fingerprint, never the plaintext key`

### Task 3: docs + PR

**Files:**
- Modify: `CURRENT.md` (data flow changed: auth check + fingerprint; note the frozen schema comment's staleness)
- Create PR

- [ ] **Step 1:** Update `CURRENT.md` — one line each: ingest 401s non-user keys (key from `~/.wakatime.cfg`), rows carry a last-4 fingerprint, `schema.sql`'s `(needed to forward)` comment is stale (forwarding reads the config table).
- [ ] **Step 2:** `cargo test` green, then push branch `sec/ingest-key-001`, open PR `SEC-001: validate ingest key, stop storing it plaintext` with `Closes SEC-001`.
- [ ] **Step 3:** Append the design doc to the commit (or a docs commit) so the spec ships with the branch.
