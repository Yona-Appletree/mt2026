//! Debug-build-only scaffolding. Not compiled into a release bundle.
//!
//! Mic permission isn't grantable in automated browsing, so the end-to-end
//! record→tag→save path needs a source of samples that isn't the mic. This
//! is that source and nothing more: it produces a `Recording` the UI feeds
//! through the *same* save path a real take uses.

use crate::recorder::Recording;

/// A believable capture rate to synthesize at — real hardware rates get
/// read off the `AudioContext`, never assumed (Q12); this is a fixture.
const TONE_SAMPLE_RATE: u32 = 48_000;
const TONE_HZ: f32 = 440.0;
const TONE_SECONDS: u32 = 1;
/// Well below clipping, and unmistakable in a waveform view.
const TONE_AMPLITUDE: f32 = 0.25;

/// One second of 440Hz sine, labelled so it can't be mistaken for a take.
pub fn test_tone() -> Recording {
    let frames = TONE_SAMPLE_RATE * TONE_SECONDS;
    let step = std::f32::consts::TAU * TONE_HZ / TONE_SAMPLE_RATE as f32;
    let samples = (0..frames)
        .map(|frame| (step * frame as f32).sin() * TONE_AMPLITUDE)
        .collect();

    Recording {
        samples,
        sample_rate: TONE_SAMPLE_RATE,
        device_label: "dev test tone (synthetic)".to_string(),
    }
}
