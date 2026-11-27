// byline live status — colored, timestamped one-liners on the serve console.
// FEAT-006: prints after each drain pass with activity. No payloads, ever.

use crate::store::Store;

// ANSI colors (no dep — raw escape sequences)
const RESET: &str = "\x1b[0m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (now / 3600) % 24;
    let m = (now / 60) % 60;
    let s = now % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

/// Query integer config key, defaulting to 0.
fn cfg_int(s: &Store, key: &str) -> i64 {
    s.conn
        .query_row("SELECT CAST(value AS INTEGER) FROM config WHERE key = ?", [key], |r| r.get(0))
        .ok()
        .unwrap_or(0)
}

/// Print a live status line after a drain pass.
/// `hb` = editor rows forwarded, `ag` = agent rows forwarded this pass.
/// Prints if anything was forwarded OR if there are failures/backoff/hook_errors.
pub fn after_drain(store: &Store, hb: usize, ag: usize) {
    let total = hb + ag;

    let unforwarded: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM heartbeats WHERE forwarded_at IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let agent_unforwarded: i64 = store
        .conn
        .query_row(
            "SELECT count(*) FROM agent_heartbeats WHERE forwarded_at IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let queued = unforwarded + agent_unforwarded;

    let failures = cfg_int(store, "forward_failures") as u32;
    let hook_errors = cfg_int(store, "hook_errors");

    // Only print if there was activity OR if failures/backoff/hook_errors exist.
    if total == 0 && failures == 0 && hook_errors == 0 {
        return;
    }

    let ts = timestamp();
    let mut line = format!("{DIM}[{ts}]{RESET} ");

    if total > 0 {
        line.push_str(&format!("{GREEN}✓{RESET} {BOLD}{total}{RESET} forwarded"));
        if hb > 0 || ag > 0 {
            line.push_str(&format!(
                " {DIM}(♥{hb} 🤖{ag}){RESET}"
            ));
        }
    }

    if queued > 0 {
        line.push_str(&format!(" {YELLOW}· {queued} queued{RESET}"));
    }

    if failures > 0 {
        // Show backoff estimate from the drain logic (15 * 2^n, capped 3600)
        let backoff = 15u64.saturating_mul(1u64 << failures.min(8)).min(3600);
        line.push_str(&format!(" {RED}✗ upstream failed{RESET} {DIM}(failures={failures}, retry in {backoff}s){RESET}"));
    }

    if hook_errors > 0 {
        line.push_str(&format!(" {DIM}· hook_err={hook_errors}{RESET}"));
    }

    println!("{line}");
}