// byline setup: repoint ~/.wakatime.cfg at the local daemon

use crate::cli::serve::home_dir;
use std::path::PathBuf;

pub const BYLINE_URL: &str = "http://127.0.0.1:8787/api/v1";

pub fn wakatime_cfg() -> PathBuf {
    if let Some(w) = std::env::var_os("WAKATIME_HOME") {
        let mut p = PathBuf::from(w);
        p.push(".wakatime.cfg");
        if p.exists() {
            return p;
        }
        return p; // WAKATIME_HOME set: write there
    }
    let mut p = home_dir();
    p.push(".wakatime.cfg");
    p
}

/// Sets the given key's value in the INI file (line replacement, no crate).
/// Returns None only on an internal error; found==false means the key was
/// absent and will be appended by the caller's fallback.
fn set_ini(content: &str, key: &str, value: &str) -> (String, bool) {
    let mut found = false;
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.trim_start().starts_with(&format!("{key} ")) || line.trim_start().starts_with(&format!("{key}=")) {
            out.push(format!("{key} = {value}"));
            found = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !found {
        // append under [settings] if present, else at end
        let mut with_key: Vec<String> = Vec::new();
        let mut inserted = false;
        for l in &out {
            if l.trim() == "[settings]" {
                with_key.push(l.clone());
                with_key.push(format!("{key} = {value}"));
                inserted = true;
            } else {
                with_key.push(l.clone());
            }
        }
        if !inserted {
            with_key.push(format!("{key} = {value}"));
        }
        return (with_key.join("\n"), false);
    }
    (out.join("\n"), true)
}

fn get_ini(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|l| {
        let t = l.trim_start();
        let rest = t.strip_prefix(key)?.strip_prefix(['=', ' '])?.trim();
        Some(rest.to_string())
    })
}

pub fn run() -> Result<(), String> {
    let path = wakatime_cfg();
    let original = std::fs::read_to_string(&path).unwrap_or_default();
    if original.is_empty() && !path.exists() {
        return Err(format!("{} not found — install Hackatime first (hackatime-setup)", path.display()));
    }
    let current = get_ini(&original, "api_url").unwrap_or_default();

    let backup = path.with_extension("cfg.byline-bak");
    let backed_up = std::fs::read_to_string(&backup).ok();

    // Which api_url did the user have before byline ever touched this file?
    let byline_original = std::fs::read_to_string(home_dir().join(".byline/original_api_url")).ok();

    if current == BYLINE_URL {
        // idempotent: refresh backup only if the cfg changed since backup
        if backed_up.as_deref() != Some(original.as_str()) {
            std::fs::write(&backup, &original).map_err(|e| e.to_string())?;
            println!("refreshed backup {}", backup.display());
        }
        println!("already repointed at {BYLINE_URL}");
        return Ok(());
    }

    let (mut new, found) = set_ini(&original, "api_url", BYLINE_URL);
    if !found {
        // ensure api_url line exists even if set_ini matched nothing
        new = format!("{original}\napi_url = {BYLINE_URL}\n");
    }

    // back up the pre-byline state
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

#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome {
        dir: PathBuf,
    }
    impl TempHome {
        fn new(name: &str) -> (TempHome, PathBuf) {
            let mut dir = std::env::temp_dir();
            dir.push(format!("byline-setup-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let dir = dir.canonicalize().unwrap();
            std::env::set_var("USERPROFILE", &dir);
            std::env::set_var("HOME", &dir);
            (TempHome { dir: dir.clone() }, dir)
        }
    }
    impl Drop for TempHome {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    fn write_cfg(home: &PathBuf, content: &str) {
        let mut p = home.clone();
        p.push(".wakatime.cfg");
        std::fs::write(p, content).unwrap();
    }

    fn read_cfg(home: &PathBuf) -> String {
        let mut p = home.clone();
        p.push(".wakatime.cfg");
        std::fs::read_to_string(p).unwrap()
    }

    const CFG: &str = "[settings]\napi_key = k-k-k\napi_url = https://hackatime.hackclub.com/api/hackatime/v1\nheartbeat_rate_limit_seconds = 30\n";

    #[test]
    fn backup_created_api_url_repointed() {
        let (_h, home) = TempHome::new("basic");
        write_cfg(&home, CFG);
        run().unwrap();
        let out = read_cfg(&home);
        assert!(out.contains(&format!("api_url = {BYLINE_URL}")), "out={out}");
        assert!(out.contains("api_key = k-k-k"), "key preserved");
        let mut bak = home.clone();
        bak.push(".wakatime.cfg.byline-bak");
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), CFG, "backup byte-identical");
    }

    #[test]
    fn rerun_is_noop() {
        let (_h, home) = TempHome::new("noop");
        write_cfg(&home, CFG);
        run().unwrap();
        let after1 = read_cfg(&home);
        run().unwrap();
        assert_eq!(read_cfg(&home), after1, "second run changes nothing");
    }

    #[test]
    fn reclaim_after_foreign_rewrite() {
        let (_h, home) = TempHome::new("reclaim");
        write_cfg(&home, CFG);
        run().unwrap();
        // hackatime-setup clobbers the file back to Hackatime
        write_cfg(&home, CFG);
        run().unwrap();
        let out = read_cfg(&home);
        assert!(out.contains(&format!("api_url = {BYLINE_URL}")), "re-claimed: {out}");
        assert!(out.contains("api_key = k-k-k"));
    }

    #[test]
    fn unsetup_restores() {
        let (_h, home) = TempHome::new("unsetup");
        write_cfg(&home, CFG);
        run().unwrap();
        unsetup().unwrap();
        assert_eq!(read_cfg(&home), CFG, "restored byte-identical");
    }

    #[test]
    fn set_ini_replaces_in_place() {
        let (out, _) = set_ini("[settings]\napi_key = a\napi_url = old\n", "api_url", "new");
        assert!(out.starts_with("[settings]\napi_key = a\napi_url = new"), "out={out}");
        let (added, _) = set_ini("[settings]\napi_key = a\n", "api_url", "new");
        assert!(added.contains("api_url = new"));
        let (bare, _) = set_ini("nothing relevant\n", "api_url", "new");
        assert!(bare.contains("api_url = new"));
        // already-correct value must not be double-appended
        let (same, found) = set_ini("[settings]\napi_url = new\n", "api_url", "new");
        assert!(found && !same.contains("api_url = new\napi_url"), "no duplicate: {same}");
    }
}
