//! `mt-archive`: the plain-files practice archive — day-dir WAV + append-only
//! JSONL log (schema v1). Owns the data contract and its (de)serialization;
//! depends on nothing else in this workspace so the format can be read back
//! by tooling that doesn't want the rest of the stack.
//!
//! [`Archive`] is the only entry point: [`Archive::append_sample`] writes a
//! take's WAV and its `log.jsonl` row (WAV fully flushed first, so a row
//! never references a missing file), and [`Archive::read_day`] reads a
//! day's rows back. There is deliberately no mutation or deletion API — the
//! log is append-only by construction (see `docs/adr/2026-08-19-archive-format.md`).
//! No async here; `mt-server` wraps calls to this crate in `spawn_blocking`.

mod archive;
mod error;
mod schema;

pub use archive::{Archive, NewSample, SavedSample};
pub use error::{ArchiveError, Result};
pub use schema::{AudioMeta, CaptureMeta, DrillMeta, KIND_SAMPLE, SCHEMA_VERSION, SampleRow};
