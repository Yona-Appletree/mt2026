# mt2026

A music practice companion: record a practice sample, tag it, and run a
live scale-degree drill (hear do–mi–sol–do in a chosen key, sing the
prompted degree, watch your pitch trace against the target line in real
time). Every sample and its metadata land in a durable, plain-files
archive that outlives the app.

This is v0.1, a walking skeleton — local-first, single user, no cloud
deployment. The next direction (continuous "tape" view, freeform-first) is
being planned; the drill flow here is the first proof of the loop, not the
final interaction.

## Quickstart

```bash
just serve       # backend on http://localhost:4826, serving the built bundle
just build-web   # (re)build the web bundle it serves
```

or for frontend iteration with hot reload:

```bash
just dev     # backend + dx serve (:8080, proxying /api to :4826) together
```

Open the app in a real browser (mic capture needs one). Keyboard, on the
drill screen: **Space** starts a drill / stops the take, **1–5** rates,
**Enter** saves, **H** replays the target. On the record screen: **Space**
or **R** toggles recording.

Known cosmetic quirk: a *debug* bundle served by `mt-server` logs endless
`ws://…/_dioxus` reconnect errors in the browser console — that's the dx
hot-reload socket with no `dx serve` behind it. Harmless; absent in
release bundles and under `just dev`.

## Data

Samples land under `~/Dropbox/Music/2026-08-19-music-practice` (override
with `MT_DATA_ROOT`) as self-contained day directories:

```text
<root>/2026-08-19/log.jsonl        # append-only metadata, schema v1
<root>/2026-08-19/samples/*.wav    # 16-bit mono at the device's real rate
```

The format is the project's most change-averse artifact — see
[docs/adr/2026-08-19-archive-format.md](docs/adr/2026-08-19-archive-format.md).

## Working conventions

See [AGENTS.md](AGENTS.md) for crate boundaries, dependency rules, and
validation commands (`just test` / `just clippy` / `just fmt-check` /
`just build-web`).
