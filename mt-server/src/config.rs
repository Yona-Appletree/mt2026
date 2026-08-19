//! Startup configuration, read from environment variables with defaults.
//! Kept separate from `main.rs` so it's trivially unit-testable and so
//! `main.rs` stays config-plus-bind only (see the module doc on `lib.rs`).

use std::path::PathBuf;

const DEFAULT_PORT: u16 = 4826;
const DEFAULT_DATA_ROOT: &str = "~/Dropbox/Music/2026-08-19-music-practice";

/// Where the built `mt-app-web` bundle actually lands. `dx` 0.7.9 ignores
/// `Dioxus.toml`'s `out_dir` (confirmed against a real `just build-web`
/// run) — the real output directory is `target/dx/<crate>/<profile>/web/public`.
/// This default is relative to the workspace root, matching how `just
/// serve` / `cargo run -p mt-server` are invoked (cwd = workspace root) and
/// the `[[web.proxy]]` entry in `mt-app-web/Dioxus.toml`.
const DEFAULT_STATIC_DIR: &str = "target/dx/mt-app-web/debug/web/public";

/// Resolved server configuration, minted once at startup from `MT_DATA_ROOT`
/// / `MT_PORT` / `MT_STATIC_DIR`.
#[derive(Debug, Clone)]
pub struct Config {
    pub data_root: PathBuf,
    pub port: u16,
    pub static_dir: PathBuf,
}

impl Config {
    /// Reads `MT_DATA_ROOT`, `MT_PORT`, and `MT_STATIC_DIR`, falling back to
    /// their documented defaults. `~`-prefixed paths (env-supplied or
    /// default) are expanded against `$HOME`.
    pub fn from_env() -> Self {
        let data_root_raw =
            std::env::var("MT_DATA_ROOT").unwrap_or_else(|_| DEFAULT_DATA_ROOT.to_string());
        let static_dir_raw =
            std::env::var("MT_STATIC_DIR").unwrap_or_else(|_| DEFAULT_STATIC_DIR.to_string());
        let port = std::env::var("MT_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PORT);

        Config {
            data_root: expand_tilde(&data_root_raw),
            port,
            static_dir: expand_tilde(&static_dir_raw),
        }
    }
}

/// Expands a leading `~` or `~/...` against `$HOME`. Anything else (relative
/// or absolute paths) passes through unchanged — no `dirs`/`shellexpand`
/// dependency needed for this narrow case.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    } else if path == "~"
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home);
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_joins_home() {
        // SAFETY: test-only env mutation, single-threaded within this test.
        unsafe {
            std::env::set_var("HOME", "/home/testuser");
        }
        assert_eq!(
            expand_tilde("~/Dropbox/Music"),
            PathBuf::from("/home/testuser/Dropbox/Music")
        );
        assert_eq!(expand_tilde("~"), PathBuf::from("/home/testuser"));
    }

    #[test]
    fn expand_tilde_leaves_non_tilde_paths_alone() {
        assert_eq!(
            expand_tilde("target/dx/mt-app-web/debug/web/public"),
            PathBuf::from("target/dx/mt-app-web/debug/web/public")
        );
        assert_eq!(
            expand_tilde("/absolute/path"),
            PathBuf::from("/absolute/path")
        );
    }
}
