mod common;
// cli::setup + setup_plugin + serve::load_expected — extracted from src
// (CHORE-004). One binary: they share the env-mutating TempHome harness.

use byline::cli::setup::{run, set_ini, unsetup, BYLINE_URL};
use common::{read_cfg, write_cfg, CFG, HOME_LOCK, TempHome};
use std::path::PathBuf;

// ---- setup ----

#[test]
fn backup_created_api_url_repointed() {
    let _g = HOME_LOCK.lock().unwrap();
    let (_h, home) = TempHome::new("basic");
    write_cfg(&home, CFG);
    run(false).unwrap();
    let out = read_cfg(&home);
    assert!(out.contains(&format!("api_url = {BYLINE_URL}")), "out={out}");
    assert!(out.contains("api_key = k-k-k"), "key preserved");
    let mut bak = home.clone();
    bak.push(".wakatime.cfg.byline-bak");
    assert_eq!(std::fs::read_to_string(&bak).unwrap(), CFG, "backup byte-identical");
    // FIX-007: config table seeded for the drain
    let mut dbp = home.clone(); dbp.push(".byline/byline.db");
    let store = byline::store::Store::open(&dbp).unwrap();
    let get = |k: &str| -> String { store.conn.query_row("SELECT value FROM config WHERE key = ?", [k], |r| r.get(0)).unwrap() };
    assert_eq!(get("upstream_api_url"), "https://hackatime.hackclub.com/api/hackatime/v1"); assert_eq!(get("upstream_api_key"), "k-k-k");
}

#[test]
fn rerun_is_noop() {
    let _g = HOME_LOCK.lock().unwrap();
    let (_h, home) = TempHome::new("noop");
    write_cfg(&home, CFG);
    run(false).unwrap();
    let after1 = read_cfg(&home);
    run(false).unwrap();
    assert_eq!(read_cfg(&home), after1, "second run changes nothing");
}

#[test]
fn reclaim_after_foreign_rewrite() {
    let _g = HOME_LOCK.lock().unwrap();
    let (_h, home) = TempHome::new("reclaim");
    write_cfg(&home, CFG);
    run(false).unwrap();
    // hackatime-setup clobbers the file back to Hackatime
    write_cfg(&home, CFG);
    run(false).unwrap();
    let out = read_cfg(&home);
    assert!(out.contains(&format!("api_url = {BYLINE_URL}")), "re-claimed: {out}");
    assert!(out.contains("api_key = k-k-k"));
}

#[test]
fn unsetup_restores() {
    let _g = HOME_LOCK.lock().unwrap();
    let (_h, home) = TempHome::new("unsetup");
    write_cfg(&home, CFG);
    run(false).unwrap();
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

// ---- setup_plugin ----

/// A dir with a fake `claude` binary; set on PATH so find_claude hits.
fn fake_claude(name: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("byline-plugin-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("claude"), "").unwrap();
    dir
}

fn settings() -> PathBuf {
    byline::cli::serve::home_dir().join(".claude").join("settings.json")
}

fn hook_cmd() -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(settings()).unwrap()).unwrap();
    v["hooks"]["PostToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|m| m["hooks"].as_array().unwrap().iter())
        .find(|h| h["command"].as_str().map_or(false, |c| c.contains("claude-hook.mjs")))
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
    byline::cli::setup_plugin::run(false).unwrap();
    let script = home.join(".byline").join("claude-hook.mjs");
    let hook_script = include_str!("../plugin/claude-code/claude-hook.mjs");
    assert_eq!(std::fs::read_to_string(&script).unwrap(), hook_script, "hook script written");
    assert!(hook_cmd()["command"].as_str().unwrap().contains(&script.display().to_string()));
}

// (b) idempotent: second run changes nothing
#[test]
fn plugin_rerun_is_noop() {
    let _g = HOME_LOCK.lock().unwrap();
    let (_h, _home) = TempHome::new("rerun");
    std::env::set_var("PATH", fake_claude("rerun"));
    byline::cli::setup_plugin::run(false).unwrap();
    let before = std::fs::read_to_string(settings()).unwrap();
    byline::cli::setup_plugin::run(false).unwrap();
    assert_eq!(std::fs::read_to_string(settings()).unwrap(), before, "second run rewrote settings");
    let v: serde_json::Value = serde_json::from_str(&before).unwrap();
    assert_eq!(v["hooks"]["PostToolUse"].as_array().unwrap().len(), 1, "no duplicate entry");
}

// (c) --no-plugins skips; so does claude-not-found
#[test]
fn skips_when_flag_or_no_claude() {
    let _g = HOME_LOCK.lock().unwrap();
    let (_h, home) = TempHome::new("skip");
    std::env::set_var("PATH", fake_claude("skip"));
    byline::cli::setup_plugin::run(true).unwrap();
    assert!(!settings().exists(), "--no-plugins must not touch settings");
    // claude absent from PATH and no ~/.claude -> skip without error
    std::env::set_var("PATH", home.join("nowhere"));
    byline::cli::setup_plugin::run(false).unwrap();
    assert!(!settings().exists(), "absent claude must not touch settings");
}

// ---- serve::load_expected ----

#[test]
fn empty_or_absent_key_means_accept_any() {
    let mut dir = std::env::temp_dir();
    dir.push(format!("byline-serve-{}-key", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(".wakatime.cfg");
    assert_eq!(byline::cli::serve::load_expected(&p), None, "absent cfg");
    std::fs::write(&p, "[settings]\napi_key = \napi_url = x\n").unwrap();
    assert_eq!(byline::cli::serve::load_expected(&p), None, "empty value must not 401 everything");
    std::fs::write(&p, "[settings]\napi_key = real-key\n").unwrap();
    assert_eq!(byline::cli::serve::load_expected(&p), Some("real-key".to_string()));
}
