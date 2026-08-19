//! Embeds the build's git SHA as `MT_APP_GIT_SHA` so the app can stamp it
//! into every saved row's `capture.app_git_sha` (plan.md's data contract).
//! Deliberately dependency-free — one `git rev-parse`, `"unknown"` if that
//! fails for any reason (no git, no repo, shallow checkout, sandbox).

use std::process::Command;

fn main() {
    // Cargo caches build scripts aggressively; without these the SHA would
    // freeze at whatever it was on the first build of a fresh target dir.
    println!("cargo::rerun-if-changed=../.git/HEAD");
    println!("cargo::rerun-if-changed=../.git/refs");

    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo::rustc-env=MT_APP_GIT_SHA={sha}");
}
