// byline setup: repoint ~/.wakatime.cfg at the local daemon

use crate::cli::serve::home_dir;
use std::io::IsTerminal;
use std::path::PathBuf;

pub const BYLINE_URL: &str = "http://127.0.0.1:8787/api/v1";

// FEAT-005: colorized UX helpers — green for success, yellow for warnings,
// plain white otherwise. Detect TTY, degrade to plain on non-tty, respect NO_COLOR.
fn is_tty() -> bool {
    std::io::stdout().is_terminal() && std::env::var("NO_COLOR").is_err()
}

const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";

pub(crate) fn ok(msg: &str) {
    if is_tty() { println!("{}{}{}", GREEN, msg, RESET); }
    else { println!("{}", msg); }
}

pub(crate) fn warn(msg: &str) {
    if is_tty() { println!("{}{}{}", YELLOW, msg, RESET); }
    else { println!("{}", msg); }
}

pub(crate) fn plain(msg: &str) {
    println!("{}", msg);
}

pub fn wakatime_cfg() -> PathBuf {
    if let Some(w) = std::env::var_os("WAKATIME_HOME") {
        let mut p = PathBuf::from(w);
        p.push(".wakatime.cfg");
        return p; // WAKATIME_HOME set: write there even if absent
    }
    let mut p = home_dir();
    p.push(".wakatime.cfg");
    p
}

/// Sets the given key's value in the INI file (line replacement, no crate).
/// found==false means the key was absent and the caller appends a fallback.
pub fn set_ini(content: &str, key: &str, value: &str) -> (String, bool) {
    let mut found = false;
    let out: Vec<String> = content
        .lines()
        .map(|l| {
            if !found && (l.trim_start().starts_with(&format!("{key} ")) || l.trim_start().starts_with(&format!("{key}="))) {
                found = true;
                format!("{key} = {value}")
            } else {
                l.to_string()
            }
        })
        .collect();
    if found {
        return (out.join("\n"), true);
    }
    // key absent: insert under [settings] when present, else append
    let mut res = out;
    match res.iter().position(|l| l.trim() == "[settings]") {
        Some(i) => res.insert(i + 1, format!("{key} = {value}")),
        None => res.push(format!("{key} = {value}")),
    }
    (res.join("\n"), false)
}

pub fn get_ini(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|l| {
        // prefix must land on a key= boundary, not mid-token
        let rest = l.trim_start().strip_prefix(key)?.trim_start_matches(['=', ' ']);
        (rest != l.trim_start()).then(|| rest.trim().to_string())
    })
}

pub fn run(skip_plugins: bool) -> Result<(), String> {
    // FEAT-005: header + sections
    ok("byline setup");
    plain("editor config:");

    let path = wakatime_cfg();
    let original = std::fs::read_to_string(&path).unwrap_or_default();
    if original.is_empty() && !path.exists() {
        return Err(format!("{} not found — install Hackatime first (hackatime-setup)", path.display()));
    }
    let current = get_ini(&original, "api_url").unwrap_or_default();

    let backup = path.with_extension("cfg.byline-bak");
    let backed_up = std::fs::read_to_string(&backup).ok();

    let byline_original = std::fs::read_to_string(home_dir().join(".byline/original_api_url")).ok();

    // FIX-007: the drain reads the config table, so seed upstream_api_url/upstream_api_key
    // from the original cfg (backup wins — the live file carries byline's url after repoint).
    // Empty api_key is not seeded (SEC-001: empty key is unreadable).
    let src_cfg = backed_up.as_deref().unwrap_or(&original);
    if let Some(url) = get_ini(src_cfg, "api_url").filter(|u| !u.trim().is_empty() && u.trim() != BYLINE_URL) {
        let (store, _) = crate::cli::status::open_store()?;
        store.set_config("upstream_api_url", url.trim()).map_err(|e| e.to_string())?;
        if let Some(key) = get_ini(src_cfg, "api_key").filter(|k| !k.trim().is_empty()) {
            store.set_config("upstream_api_key", key.trim()).map_err(|e| e.to_string())?;
        }
        ok(&format!("upstream: {}", url.trim()));
    }

    if current == BYLINE_URL {
        // idempotent: refresh backup only if the cfg changed since backup
        let mut files_touched = 0usize;
        if backed_up.as_deref() != Some(original.as_str()) {
            std::fs::write(&backup, &original).map_err(|e| e.to_string())?;
            ok(&format!("refreshed backup {}", backup.display()));
            files_touched += 1;
        }
        ok(&format!("already repointed at {BYLINE_URL}"));
        plain("agent hook:");
        super::setup_plugin::run(skip_plugins)?;
        // FEAT-005: summary
        ok(&format!("byline setup: {} files touched — run: byline serve", files_touched));
        return Ok(());
    }

    // Not yet repointed: new installation flow
    let mut files_touched = 0usize;

    let (mut new, found) = set_ini(&original, "api_url", BYLINE_URL);
    if !found {
        new = format!("{original}\napi_url = {BYLINE_URL}\n");
    }

    std::fs::write(&backup, &original).map_err(|e| e.to_string())?;
    files_touched += 1;

    let orig_dir = home_dir().join(".byline");
    std::fs::create_dir_all(&orig_dir).map_err(|e| e.to_string())?;
    std::fs::write(orig_dir.join("original_api_url"), format!("{current}\n")).map_err(|e| e.to_string())?;
    files_touched += 1;

    std::fs::write(&path, &new).map_err(|e| e.to_string())?;
    files_touched += 1;

    plain(&format!("backed up {} -> {}", path.display(), backup.display()));
    ok(&format!("api_url: {current} -> {BYLINE_URL}"));

    // WARNING: foreign rewrite re-claimed
    warn("WARNING: re-running hackatime-setup overwrites this file and undoes byline.");

    if let Some(prev) = byline_original.as_deref() {
        if !prev.trim().is_empty() && prev.trim() != current.trim() && current != BYLINE_URL && !current.is_empty() {
            plain(&format!("note: previous api_url was {}, byline first-saw {}", current, prev.trim()));
        }
    }
    // re-claim after foreign rewrite: current was neither byline's nor empty
    if !current.is_empty() && current != BYLINE_URL && Some(current.as_str()) != byline_original.as_deref().map(str::trim) {
        warn(&format!("re-claim: api_url had been changed to {} (hackatime-setup?); repointing again.", current));
    }

    plain("agent hook:");
    super::setup_plugin::run(skip_plugins)?;

    // FEAT-005: summary
    ok(&format!("byline setup: {} files touched — run: byline serve", files_touched));
    Ok(())
}

pub fn unsetup() -> Result<(), String> {
    let path = wakatime_cfg();
    let backup = path.with_extension("cfg.byline-bak");
    let data = std::fs::read_to_string(&backup).map_err(|e| format!("no backup at {}: {e}", backup.display()))?;
    std::fs::write(&path, data).map_err(|e| format!("{}", e))?;
    println!("restored {}", path.display());
    Ok(())
}