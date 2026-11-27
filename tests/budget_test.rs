//! Enforces the budgets from CLAUDE.md. Do not edit this test to make it pass.
//! (Lives in tests/ because cargo test only auto-discovers tests/*.rs —
//! a root-level budget_test.rs would never run.)

use std::path::{Path, PathBuf};

// Dependencies allowed in Cargo.toml. Add a dep to ALLOWED in the same
// commit that adds it to Cargo.toml. Max 1 new dependency per PR.
const ALLOWED: &[&str] = &["rusqlite", "ureq", "tiny_http", "serde_json", "chrono"];

const MAX_LINES_PER_FILE: usize = 400;
const WARN_LINES_PER_FILE: usize = 300;
const MAX_SRC_TOTAL: usize = 2500;

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().map_or(false, |e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn file_line_caps() {
    let mut files = Vec::new();
    walk(Path::new("src"), &mut files);
    let mut total = 0usize;
    for f in files {
        let n = std::fs::read_to_string(&f).expect("read").lines().count();
        total += n;
        assert!(n <= MAX_LINES_PER_FILE, "{} has {} lines (cap {})", f.display(), n, MAX_LINES_PER_FILE);
        if n > WARN_LINES_PER_FILE {
            eprintln!("WARN: {} has {} lines (warn at {})", f.display(), n, WARN_LINES_PER_FILE);
        }
    }
    assert!(total <= MAX_SRC_TOTAL, "src/ total {} lines (cap {})", total, MAX_SRC_TOTAL);
}

#[test]
fn deps_in_allowed() {
    let toml = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml");
    let mut in_deps = false;
    for line in toml.lines() {
        if line.starts_with('[') {
            in_deps = line.starts_with("[dependencies]") || line.starts_with("[dev-dependencies]");
            continue;
        }
        if in_deps {
            let name = line.split('=').next().unwrap_or("").trim();
            if name.is_empty() {
                continue;
            }
            assert!(
                ALLOWED.contains(&name),
                "dependency `{}` not in ALLOWED — add it in the same commit (max 1 new dep per PR)",
                name
            );
        }
    }
}
