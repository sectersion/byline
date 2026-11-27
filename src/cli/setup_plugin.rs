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

