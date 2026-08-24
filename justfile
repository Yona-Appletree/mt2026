# mt2026 dev tasks

default:
    @just --list

# Run all native (default-member) tests.
test:
    cargo test

# Deny warnings so lint drift gets caught here, not in review.
clippy:
    cargo clippy -- -D warnings

fmt:
    cargo fmt

fmt-check:
    cargo fmt --check

# Host build of every default-member crate (excludes mt-app-web; see its
# own default-members comment in the workspace Cargo.toml).
build:
    cargo build

# WASM compile gate for the Dioxus frontend. `cargo build` never reaches
# this crate on purpose, so this is the only thing that exercises it.
#
# Run from the workspace root with `-p`, not `cd mt-app-web && dx build`:
# `dx` resolves its "main package" against the workspace's default-members,
# and mt-app-web is deliberately excluded from those (see root Cargo.toml),
# so invoking it from inside the crate dir panics trying to find it. Output
# lands in `target/dx/mt-app-web/<profile>/web/public/` — `dx` 0.7.9 ignores
# Dioxus.toml's `out_dir`, matching lp2025 (see its justfile).
build-web:
    dx build --web -p mt-app-web

# Run the backend alone, e.g. to hit /api/health by hand.
serve:
    cargo run -p mt-server

# Run backend + frontend dev servers together. This just backgrounds
# `mt-server` and runs `dx serve` in the foreground, killing the server
# when you Ctrl-C out — kept deliberately dumb. Run them in two terminals
# instead (`just serve` / `dx serve --web -p mt-app-web`) if you want
# independent logs.
dev:
    cargo run -p mt-server & \
    trap 'kill %1' EXIT; \
    dx serve --web -p mt-app-web
