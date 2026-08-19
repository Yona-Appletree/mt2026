# mt2026

A music practice companion: record a practice sample, tag it, and run a
live-graded scale-degree drill (sing along, see your pitch trace against the
target in real time). Every sample and its metadata land in a durable,
plain-files archive.

This is v0.1, a walking skeleton — local-first, single user, no cloud
deployment yet.

## Quickstart

```bash
just dev
```

This runs the `mt-server` backend and the `mt-app-web` frontend dev server
side by side (see the `justfile` for the exact recipes; `just` alone lists
everything available).

## Working conventions

See [AGENTS.md](AGENTS.md) for crate boundaries, dependency rules, and
validation commands.
