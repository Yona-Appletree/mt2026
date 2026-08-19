# Agent conventions for mt2026

A music practice companion: record → tag → save, plus a live-graded
scale-degree drill. v0.1 is a local-first walking skeleton — see
`agent-context.toml` for the planning-repo pointer.

## Crate boundaries

```text
mt-app-web ─┬─→ mt-session ─┬─→ mt-theory
(Dioxus)    │               └─→ mt-pitch
            ├─→ mt-pitch    (live trace data plane)
            └─→ mt-theory
mt-server ──→ mt-archive
(axum)          (mt-archive depends on nothing in-workspace)
```

Dependency arrows point high-churn → low-churn only. Rules:

- **`axum`/`tokio` live in `mt-server` and nowhere else.** The server is a
  humble adapter over `mt-archive`; no other crate takes a web or async
  runtime dependency.
- **`mt-pitch` and `mt-theory` are pure.** `&[f32]`/values in, values out.
  No I/O, no web-sys, no archive types. `mt-pitch` stays zero-dependency
  beyond `pitch-detection` so it can be shared later without dragging in
  the rest of the stack.
- **`mt-session` is the humble-view core.** Events in, state + commands
  out; side effects live behind port traits (`AudioOut`, `Capture`,
  `Store`) so the state machine is unit-testable with fakes, no browser
  required.
- **Control plane vs. data plane.** The 60fps live pitch trace (analyser →
  `mt-pitch` → ring buffer → canvas) is owned by the web adapter and never
  routes per-frame events through `mt-session`. Trace geometry is a pure
  function returning polylines; only user-level events (start drill,
  submit take, retry) cross into `mt-session`.
- **The server mints sample ids and paths.** Single writer, single
  id-minter — never let a client or a second process assign an id.
- **The archive log is append-only.** `log.jsonl` rows are never rewritten
  or deleted in place; derived/analysis data never lives in that file —
  it goes in separate, regenerable files.
- **Never run `cargo build --workspace` for the web crate.** `mt-app-web`
  is excluded from `default-members` specifically so plain `cargo
  build`/`cargo test` stay host-only. Build/serve it with `just build-web`
  / `just dev`, which shell out to `dx`.

## Validation commands

```bash
just test        # cargo test over default-members
just clippy       # cargo clippy -- -D warnings
just fmt-check    # cargo fmt --check
just build-web    # dx build in mt-app-web (the WASM compile gate)
```

Run all four before calling a change done. Do not suppress warnings or
disable tests to make these pass.
