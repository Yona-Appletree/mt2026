//! `mt-session`: the humble-view drill core — events in, state and commands
//! out. It owns the rules of a take (what plays when, what may be saved,
//! what a trace looks like); it owns no audio, no network, no clock, and no
//! browser.
//!
//! Three things live here:
//!
//! - [`DrillSession`] — the state machine. [`DrillSession::handle`] takes
//!   one [`Event`] and returns the [`Command`]s the adapter should carry
//!   out. **Commands are returned as data, not dispatched through port
//!   traits**: a test asserts `vec![Command::StartCapture]` instead of
//!   instrumenting a fake, and the whole crate stays free of `dyn`.
//!   (`AGENTS.md` still describes the port-trait shape this replaced.)
//! - [`trace_view`] — the pitch trace as pure geometry. Track and target
//!   in, polylines out; the canvas code only strokes what it is handed
//!   (vision D12). Per-frame pitch estimates never enter the state machine
//!   — the live trace is the adapter's data plane, and only take-level
//!   events cross into the control plane here.
//! - [`NewSampleMeta`] and friends — the serde wire types for a save. They
//!   carry only what a client legitimately knows; ids, timestamps, and file
//!   paths are the server's to mint.
//!
//! ## The shape of a take
//!
//! ```text
//! Idle ──Start──▶ PlayingReference ──ReferenceDone──▶ Listening
//!   ▲                    │  PlayArpeggio                 │  StartCapture
//!   │                    │                               │
//!   │                  Stop (cancels, nothing captured)   │ Stop → StopCapture
//!   │                    │                               │
//!   ├────────────────────┘                        TrackReady
//!   │                                                    ▼
//!   │                          PlayTone ◀──HearTarget── Review ──Save──▶ Saving
//!   └──────────────────────────Reset──────────────────────┘   ▲            │
//!                                                    SaveErr ─┘      SaveOk│
//!                                                                          ▼
//!                                                                       Saved
//! ```
//!
//! The prompt is by solfège name and the target tone is played only on the
//! review screen, after the singing — the drill trains audiation, not
//! matching (vision D9).
//!
//! ## Rules the machine enforces
//!
//! - `Save` without a rating does nothing (see [`DrillSession::can_save`]).
//! - Ratings outside 1..=5 are dropped rather than stored.
//! - `Stop` during the arpeggio cancels cleanly: no capture was started, so
//!   none is stopped and no take is invented.
//! - A second `Stop` while already stopping (the auto-stop timer racing the
//!   spacebar) does not order capture off twice.
//! - `SaveErr` returns to `Review` with the take, rating, and note intact.
//! - `Reset` from `Listening` closes the mic on the way out.
//!
//! Events that make no sense in the current state are ignored: no commands,
//! no state change.

mod meta;
mod session;
mod trace;

pub use meta::{
    AudioMeta, CaptureContext, CaptureMeta, DRILL_TYPE_DEGREE, DRILL_TYPE_FREE, DrillMeta,
    NewSampleMeta,
};
pub use session::{Command, DrillSession, Event, SessionState};
pub use trace::{
    TRACE_CLARITY_FLOOR, TRACE_RANGE_CENTS, TraceSegment, TraceView, Viewport, trace_view,
};
