// byline setup — install the claude-code hook: writes ~/.byline/claude-hook.mjs
// and appends the PostToolUse entry to ~/.claude/settings.json.
// FEAT-008: also installs the OpenCode plugin to ~/.config/opencode/plugins/byline.ts.

use super::serve::home_dir;
use std::io::IsTerminal;

const HOOK_SCRIPT: &str = include_str!("../../plugin/claude-code/claude-hook.mjs");
const SCRIPT_NAME: &str = "claude-hook.mjs";
const OPENCODE_PLUGIN: &str = include_str!("../../plugin/opencode/byline.ts");

pub fn run(skip: bool) -> Result<(), String> {
    if skip {
        return Ok(());
    }
    if !claude_installed() && !opencode_installed() {
        println!("neither claude code nor opencode found — skipping hook install");
        return Ok(());
    }

    // --- Claude Code hook install (same as before) ---
    if claude_installed() {
        // prompt only on a real terminal; non-tty (scripts, tests) proceeds
        if std::io::stdin().is_terminal() && !confirmed_claude() {
            println!("skipped claude hook");
            // still fall through to opencode below
        } else {
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
                // still fall through to opencode below
            } else {
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
            }
        }
    }

    // --- OpenCode plugin install ---
    if opencode_installed() {
        if std::io::stdin().is_terminal() && !confirmed_openencode() {
            println!("skipped opencode plugin");
            // still return Ok(()) below
        } else {
            let opencode_dir = home_dir().join(".config").join("opencode");
            let plugins_dir = opencode_dir.join("plugins");
            std::fs::create_dir_all(&plugins_dir).map_err(|e| e.to_string())?;
            let plugin_path = plugins_dir.join("byline.ts");
            if plugin_path.exists() {
                println!("opencode plugin already installed ({})", plugin_path.display());
            } else {
                std::fs::write(&plugin_path, OPENCODE_PLUGIN).map_err(|e| e.to_string())?;
                println!("installed {} (opencode plugin -> byline hook)", plugin_path.display());
            }
        }
    }

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

/// Detects whether OpenCode is installed (by checking ~/.config/opencode directory).
fn opencode_installed() -> bool {
    home_dir()
        .join(".config")
        .join("opencode")
        .is_dir()
    || std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p).any(|d| {
                d.join("opencode")
                    .join("opencode.exe")
                    .exists()
                    || d.join("opencode").join("opencode").exists()
            })
        })
        .unwrap_or(false)
}

fn confirmed_claude() -> bool {
    print!("install byline hook for claude code? [y/N] ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut a = String::new();
    std::io::stdin().read_line(&mut a).map_or(false, |_| a.trim().eq_ignore_ascii_case("y"))
}

fn confirmed_openencode() -> bool {
    print!("install byline plugin for OpenCode? [y/N] ");
    use std::io::Write;
    std::io::stdout().flush().ok();
    let mut a = String::new();
    std::io::stdin().read_line(&mut a).map_or(false, |_| a.trim().eq_ignore_ascii_case("y"))
}