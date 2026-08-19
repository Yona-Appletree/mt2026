//! `mt-pitch`: pure streaming pitch detection over `&[f32]` audio buffers,
//! shared by the live trace (data plane) and batch/offline analysis. No I/O,
//! no web-sys, no archive types, and no in-workspace dependencies — samples
//! in, [`PitchEstimate`]s out.
//!
//! One type does the work: [`Tracker`]. Construct it with the capture rate,
//! push blocks of mono `f32` samples at it with [`Tracker::feed`], and read
//! back one estimate per analysis hop. The same tracker serves the 60 fps
//! live trace (fed `AnalyserNode` blocks) and the review screen (fed an
//! entire recorded take), which is what keeps offline analysis honest about
//! matching what the singer saw (vision D6).
//!
//! ```
//! use mt_pitch::Tracker;
//!
//! let sample_rate = 48_000;
//! let mut tracker = Tracker::new(sample_rate);
//! let a4: Vec<f32> = (0..sample_rate)
//!     .map(|i| {
//!         let t = i as f64 / sample_rate as f64;
//!         (0.4 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32
//!     })
//!     .collect();
//!
//! let estimates = tracker.feed(&a4);
//! let hz = estimates.last().unwrap().hz.unwrap();
//! assert!((hz - 440.0).abs() < 1.0);
//! ```
//!
//! Window, hop, cadence, latency, and the silence/noise thresholds are
//! documented on [`Tracker`]; the fixture suite in `tests/fixtures.rs` is
//! the oracle those numbers were tuned against.

mod tracker;

pub use tracker::{CLARITY_FLOOR, PitchEstimate, Tracker};
