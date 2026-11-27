// byline setup: repoint ~/.wakatime.cfg at the local daemon

use crate::cli::serve::home_dir;
use std::path::PathBuf;

pub const BYLINE_URL: &str = "http://127.0.0.1:8787/api/v1";

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
        println!("upstream: {}", url.trim());
    }

    if current == BYLINE_URL {
        // idempotent: refresh backup only if the cfg changed since backup
        if backed_up.as_deref() != Some(original.as_str()) {
            std::fs::write(&backup, &original).map_err(|e| e.to_string())?;
            println!("refreshed backup {}", backup.display());
        }
        println!("already repointed at {BYLINE_URL}");
        super::setup_plugin::run(skip_plugins)?;
        return Ok(());
    }

    let (mut new, found) = set_ini(&original, "api_url", BYLINE_URL);
    if !found {
        new = format!("{original}\napi_url = {BYLINE_URL}\n");
    }

    std::fs::write(&backup, &original).map_err(|e| e.to_string())?;
    let orig_dir = home_dir().join(".byline");
    std::fs::create_dir_all(&orig_dir).map_err(|e| e.to_string())?;
    std::fs::write(orig_dir.join("original_api_url"), format!("{current}\n")).map_err(|e| e.to_string())?;
    std::fs::write(&path, &new).map_err(|e| e.to_string())?;

    println!("backed up {} -> {}", path.display(), backup.display());
    println!("api_url: {current} -> {BYLINE_URL}");
    println!("WARNING: re-running hackatime-setup overwrites this file and undoes byline.");
    if let Some(prev) = byline_original.as_deref() {
        if !prev.trim().is_empty() && prev.trim() != current.trim() && current != BYLINE_URL && !current.is_empty() {
            println!("note: previous api_url was {current}, byline first-saw {}", prev.trim());
        }
    }
    // re-claim after foreign rewrite: current was neither byline's nor empty
    if !current.is_empty() && current != BYLINE_URL && Some(current.as_str()) != byline_original.as_deref().map(str::trim) {
        println!("re-claim: api_url had been changed to {current} (hackatime-setup?); repointing again.");
    }
    super::setup_plugin::run(skip_plugins)?;
    Ok(())
}

pub fn unsetup() -> Result<(), String> {
    let path = wakatime_cfg();
    let backup = path.with_extension("cfg.byline-bak");
    let data = std::fs::read_to_string(&backup).map_err(|e| format!("no backup at {}: {e}", backup.display()))?;
    std::fs::write(&path, data).map_err(|e| e.to_string())?;
    println!("restored {}", path.display());
    Ok(())
}

