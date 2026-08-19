# ADR: plain-files practice archive (schema v1)

Status: accepted
Date: 2026-08-19

## Context

mt2026 is a local-first practice recorder (vision D1: zero deployment,
single user, Dropbox as the sync layer — D8 puts the data root at
`~/Dropbox/Music/2026-08-19-music-practice`). Every take is a WAV file plus
a handful of tags (which drill, self-rating, a note). This is the artifact
that outlives every rewrite of the app around it: the UI, the pitch
tracker, even the server can all be replaced, but the recordings and their
tags need to stay readable by whatever comes next, including tools that
never link against this codebase.

## Decision

### Layout: self-contained day dirs

```text
<root>/2026-08-19/log.jsonl
<root>/2026-08-19/samples/113211-a1b2.wav
```

Each calendar day (local time) gets its own directory containing that
day's append-only log and its WAVs. Rationale (planning Q11): the data
root syncs through Dropbox, and Dropbox's conflict resolution is
per-file — two devices writing near-simultaneously to a single growing
`log.jsonl` for the whole archive risks a sync conflict that clobbers or
forks the entire history. Sharding by day shrinks the blast radius of any
conflict to one day's rows, and in practice this is a single-writer app
(the server mints ids — see below) so conflicts should be rare regardless.
Day dirs are also just convenient: `ls` shows practice history at a
glance, and every day's data (log + audio) travels together.

### Schema v1

One JSON object per line in `log.jsonl`, newline-delimited, append-only.
Example row:

```json
{
  "schema_version": 1,
  "id": "smp-20260819-113211-a1b2",
  "recorded_at": "2026-08-19T11:32:11-07:00",
  "kind": "sample",
  "drill": {
    "type": "degree",
    "key": "D",
    "mode": "major",
    "degree": 5,
    "target_hz": 440.0
  },
  "self_rating": 4,
  "note": "flat on the approach",
  "audio": {
    "path": "samples/113211-a1b2.wav",
    "sample_rate": 48000,
    "channels": 1,
    "bit_depth": 16,
    "duration_ms": 3210
  },
  "capture": {
    "device_label": "MacBook Pro Microphone",
    "user_agent": "…",
    "app_git_sha": "abc1234"
  }
}
```

`drill.type` is a free-form string (`"degree"` and `"free"` in v0.1);
`drill.key`/`mode`/`degree`/`target_hz`, `self_rating`, and `note` are
optional and omitted (not nulled) when absent. `audio.path` is relative to
the day dir. `audio.sample_rate` is whatever the capturing device actually
reported — never assumed (Q12: browsers hand back varying rates depending
on hardware and OS, and baking in an assumed 48kHz would silently corrupt
playback speed/pitch on any device that captures at 44.1kHz or 96kHz).
Timestamps are RFC3339 with local offset.

`mt-archive` (this repo, `mt-archive/src/schema.rs`) is the canonical Rust
encoding of this shape — `SampleRow`, `DrillMeta`, `AudioMeta`,
`CaptureMeta`, `SCHEMA_VERSION`. Treat the JSON shape above as the spec;
the Rust types are a transcription of it, checked by a test that
round-trips this exact example.

### Raw / metadata / derived strata, append-only

The log holds raw capture facts and user-entered tags only. Anything
computed from the audio later — pitch traces, accuracy scores, practice
statistics — belongs in separate, regenerable files, never mixed into
`log.jsonl`. This keeps the log small, keeps it human-diffable, and means
a bug in an analysis pass can never corrupt the source of truth: worst
case, delete the derived file and recompute it.

Rows are never rewritten or deleted in place. `mt-archive` exposes no
mutation or deletion API by construction — only `append_sample` (write)
and `read_day` (read). A log that only ever grows is trivially safe to
sync, easy to reason about, and preserves practice history even for takes
a later UI might let a user "delete" (which would be a tombstone row or a
UI-level filter, not a rewrite of this file — not designed yet, v0.1 has
no such feature).

### 16-bit mono, device-rate WAV

Audio is downmixed to mono and encoded as 16-bit PCM WAV at the capture
device's actual sample rate (Q12, see above). 16-bit mono keeps files
small (this is meant to accumulate thousands of short takes under
Dropbox) while comfortably exceeding the resolution needed to judge pitch
accuracy by ear or by `mt-pitch`. The WAV is fully written and flushed
(`hound::WavWriter::finalize`) *before* its `log.jsonl` line is appended —
a row must never reference a file that doesn't exist yet, so a reader
walking the log can always trust `audio.path`.

### Server mints ids and paths — single writer

`id` (`smp-YYYYMMDD-HHMMSS-<4 hex>`) and the WAV's relative path are
minted by `mt-archive` at write time, not supplied by the caller. This
carries forward a lesson from lp2025: letting more than one process (or a
client) assign ids invites collisions and split-brain history. `mt-server`
is the only process that calls into `mt-archive`, and `mt-archive` itself
is the single point that mints — so there is exactly one id-minter for the
whole archive, matching AGENTS.md's "the server mints sample ids and
paths" rule.

## Consequences

- Adding a field to a row is backwards compatible (old readers ignore
  unknown fields via serde's default `deny_unknown_fields`-off behavior).
  Removing or renaming a field, or changing what `schema_version` means,
  requires bumping `SCHEMA_VERSION` and documenting the migration here.
- Any future "delete a take" feature needs new design — it cannot be a
  rewrite of `log.jsonl` without breaking the append-only guarantee this
  ADR relies on.
- Day-dir sharding means "all rows across all time" isn't a single read;
  a future cross-day reader (out of scope for v0.1 — `mt-archive` only
  ships `read_day`) will need to walk the root's day directories.
