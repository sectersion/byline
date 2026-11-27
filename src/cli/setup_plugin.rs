// byline setup — install the claude-code hook: writes ~/.byline/claude-hook.mjs
// and appends the PostToolUse entry to ~/.claude/settings.json.

use super::serve::home_dir;
use std::io::IsTerminal;

const HOOK_SCRIPT: &str = include_str!("../../plugin/claude-code/claude-hook.mjs");
const SCRIPT_NAME: &str = "claude-hook.mjs";

pub fn run(skip: bool) -> Result<(), String> {
    if skip {
        return Ok(());
    }
    if !claude_installed() {
        println!("claude code not found — skipping hook install");
        return Ok(());
    }
    // prompt only on a real terminal; non-tty (scripts, tests) proceeds
    if std::io::stdin().is_terminal() && !confirmed() {
        println!("skipped claude hook");
        return Ok(());
    }

    let dir = home_dir().join(".byline");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script = dir.join(SCRIPT_NAME);
    if std::fs::read_to_string(&script).ok().as_deref() != Some(HOOK_SCRIPT) {
        std::fs::write(&script, HOOK_SCRIPT).map_err(|e| e.to_string())?;
        println!("installed {}", script.display());
    }

    let settings = home_dir().join(".claude").join("settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).map_err(|e| e.to_string())?;
    let old = std::fs::read_to_string(&settings).unwrap_or_default();
    if old.contains(SCRIPT_NAME) {
        println!("claude hook already installed ({})", settings.display());
        return Ok(());
    }
    let mut v: serde_json::Value = if old.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&old).map_err(|e| format!("{}: {e}", settings.display()))?
    };
    if v["hooks"]["PostToolUse"].is_null() {
        v["hooks"]["PostToolUse"] = serde_json::json!([]);
    }
    v["hooks"]["PostToolUse"]
        .as_array_mut()
        .ok_or_else(|| format!("{}: hooks.PostToolUse is not a list", settings.display()))?
        .push(serde_json::json!({
            "matcher": "*",
            "hooks": [{"type": "command", "command": format!("node \"{}\"", script.display())}]
        }));
    let out = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::write(&settings, out + "\n").map_err(|e| e.to_string())?;
    println!("installed {} (PostToolUse -> byline hook)", settings.display());
    Ok(())
}

/// `claude` on PATH, or a ~/.claude directory.
fn claude_installed() -> bool {
    if home_dir().join(".claude").is_dir() {
        return true;
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).any(|d| ["claude", "claude.exe", "claude.cmd"].iter().any(|n| d.join(n).exists()))
}

fn confirmed() -> bool {
    print!("install byline hook for claude code? [y/N] ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut a = String::new();
    std::io::stdin().read_line(&mut a).map_or(false, |_| a.trim().eq_ignore_ascii_case("y"))
}

#[cfg(test)]
mod tests {
    use super::super::setup::tests::{TempHome, HOME_LOCK};
    use super::*;
    use std::path::PathBuf;

    /// A dir with a fake `claude` binary; set on PATH so find_claude hits.
    fn fake_claude(name: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("byline-plugin-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("claude"), "").unwrap();
        dir
    }

    fn settings() -> PathBuf {
        home_dir().join(".claude").join("settings.json")
    }

    fn hook_cmd() -> serde_json::Value {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(settings()).unwrap()).unwrap();
        v["hooks"]["PostToolUse"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["hooks"].as_array().unwrap().iter())
            .find(|h| h["command"].as_str().map_or(false, |c| c.contains(SCRIPT_NAME)))
            .cloned()
            .expect("byline PostToolUse entry")
    }

    // (a) claude present -> script installed, settings entry added
    #[test]
    fn installs_script_and_settings_entry() {
        let _g = HOME_LOCK.lock().unwrap();
        let (_h, home) = TempHome::new("install");
        let bin = fake_claude("install");
        std::env::set_var("PATH", &bin);
        run(false).unwrap();
        let script = home.join(".byline").join(SCRIPT_NAME);
        assert_eq!(std::fs::read_to_string(&script).unwrap(), HOOK_SCRIPT, "hook script written");
        assert!(hook_cmd()["command"].as_str().unwrap().contains(&script.display().to_string()));
    }

    // (b) idempotent: second run changes nothing
    #[test]
    fn rerun_is_noop() {
        let _g = HOME_LOCK.lock().unwrap();
        let (_h, _home) = TempHome::new("rerun");
        std::env::set_var("PATH", fake_claude("rerun"));
        run(false).unwrap();
        let before = std::fs::read_to_string(settings()).unwrap();
        run(false).unwrap();
        assert_eq!(std::fs::read_to_string(settings()).unwrap(), before, "second run rewrote settings");
        let v: serde_json::Value = serde_json::from_str(&before).unwrap();
        assert_eq!(v["hooks"]["PostToolUse"].as_array().unwrap().len(), 1, "no duplicate entry");
    }

    // (c) --no-plugins skips; so does claude-not-found
    #[test]
    fn skips_when_flag_or_no_claude() {
        let _g = HOME_LOCK.lock().unwrap();
        let (_h, _home) = TempHome::new("skip");
        std::env::set_var("PATH", fake_claude("skip"));
        run(true).unwrap();
        assert!(!settings().exists(), "--no-plugins must not touch settings");
        // claude absent from PATH and no ~/.claude -> skip without error
        std::env::set_var("PATH", home_dir().join("nowhere"));
        run(false).unwrap();
        assert!(!settings().exists(), "absent claude must not touch settings");
    }
}
