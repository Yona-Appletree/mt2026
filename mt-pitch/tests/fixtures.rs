//! Fixture suite for the pitch tracker — the oracle the window/hop/threshold
//! choices in `Tracker` were tuned against.
//!
//! Every signal is synthesized inline (no audio files in the repo) and every
//! assertion is in cents, because cents are what the drill actually cares
//! about. **Do not loosen a tolerance to make a test pass**: if a clean sine
//! stops landing within ±5 cents, the tracker is wrong, not the fixture.

use mt_pitch::{PitchEstimate, Tracker};
use std::f64::consts::PI;

// ---------------------------------------------------------------- signals

/// A constant-frequency sine at `amp` peak amplitude.
fn sine(hz: f64, secs: f64, sample_rate: u32, amp: f64) -> Vec<f32> {
    frequency_curve(secs, sample_rate, amp, |_| hz)
}

/// An exponential glide from `from_hz` to `to_hz` (exponential so the sweep
/// is linear in pitch, which is what "monotonic in cents" means).
fn exp_sweep(from_hz: f64, to_hz: f64, secs: f64, sample_rate: u32, amp: f64) -> Vec<f32> {
    frequency_curve(secs, sample_rate, amp, |t| {
        from_hz * (to_hz / from_hz).powf(t / secs)
    })
}

/// A `rate_hz` sinusoidal vibrato of ±`depth_cents` around `centre_hz`.
fn vibrato(
    centre_hz: f64,
    rate_hz: f64,
    depth_cents: f64,
    secs: f64,
    sample_rate: u32,
    amp: f64,
) -> Vec<f32> {
    frequency_curve(secs, sample_rate, amp, |t| {
        let cents = depth_cents * (2.0 * PI * rate_hz * t).sin();
        centre_hz * 2f64.powf(cents / 1200.0)
    })
}

/// Phase-accumulating oscillator: samples a frequency curve `hz_at(t)` so
/// frequency can vary without phase discontinuities.
fn frequency_curve(secs: f64, sample_rate: u32, amp: f64, hz_at: impl Fn(f64) -> f64) -> Vec<f32> {
    let n = (secs * sample_rate as f64) as usize;
    let dt = 1.0 / sample_rate as f64;
    let mut phase: f64 = 0.0;
    (0..n)
        .map(|i| {
            let sample = (amp * phase.sin()) as f32;
            phase += 2.0 * PI * hz_at(i as f64 * dt) * dt;
            sample
        })
        .collect()
}

fn silence(secs: f64, sample_rate: u32) -> Vec<f32> {
    vec![0.0; (secs * sample_rate as f64) as usize]
}

/// Deterministic white noise (xorshift, so the suite never flakes).
fn white_noise(secs: f64, sample_rate: u32, amp: f64, seed: u64) -> Vec<f32> {
    let n = (secs * sample_rate as f64) as usize;
    let mut state = seed | 1;
    (0..n)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let unit = (state >> 11) as f64 / (1u64 << 53) as f64; // 0..1
            (amp * (2.0 * unit - 1.0)) as f32
        })
        .collect()
}

// ---------------------------------------------------------------- helpers

fn cents(a_hz: f64, b_hz: f64) -> f64 {
    1200.0 * (a_hz / b_hz).log2()
}

/// Estimates from feeding a whole signal in realistic 128-sample blocks.
fn track(signal: &[f32], sample_rate: u32) -> Vec<PitchEstimate> {
    let mut tracker = Tracker::new(sample_rate);
    signal
        .chunks(128)
        .flat_map(|block| tracker.feed(block))
        .collect()
}

/// The voiced (`hz.is_some()`) estimates only.
fn voiced(estimates: &[PitchEstimate]) -> Vec<(f64, f64, f64)> {
    estimates
        .iter()
        .filter_map(|e| e.hz.map(|hz| (e.t_ms, hz, e.clarity)))
        .collect()
}

/// Asserts every estimate is voiced and within `tolerance_cents` of `hz`.
fn assert_steady(estimates: &[PitchEstimate], expected_hz: f64, tolerance_cents: f64) {
    assert!(!estimates.is_empty(), "no estimates produced at all");
    for estimate in estimates {
        let hz = estimate
            .hz
            .unwrap_or_else(|| panic!("unvoiced estimate at t={} ms", estimate.t_ms));
        let error = cents(hz, expected_hz);
        assert!(
            error.abs() <= tolerance_cents,
            "t={:.1}ms: {hz:.3} Hz is {error:+.2} cents from {expected_hz} Hz \
             (tolerance ±{tolerance_cents}); clarity {:.3}",
            estimate.t_ms,
            estimate.clarity
        );
    }
}

// ------------------------------------------------------- steady-tone accuracy

#[test]
fn a3_220hz_sine_is_within_5_cents() {
    assert_steady(&track(&sine(220.0, 1.0, 48_000, 0.4), 48_000), 220.0, 5.0);
}

#[test]
fn a4_440hz_sine_is_within_5_cents() {
    assert_steady(&track(&sine(440.0, 1.0, 48_000, 0.4), 48_000), 440.0, 5.0);
}

#[test]
fn accuracy_holds_at_44100_hz() {
    assert_steady(&track(&sine(220.0, 1.0, 44_100, 0.4), 44_100), 220.0, 5.0);
    assert_steady(&track(&sine(440.0, 1.0, 44_100, 0.4), 44_100), 440.0, 5.0);
}

#[test]
fn accuracy_holds_across_the_singable_range() {
    // D2 (a low male voice) through D5 — the widest span v0.1's degree drill
    // can ask for. Same ±5 cents everywhere.
    for hz in [73.42, 146.83, 293.66, 587.33] {
        assert_steady(&track(&sine(hz, 1.0, 48_000, 0.4), 48_000), hz, 5.0);
    }
}

#[test]
fn quiet_but_audible_tones_are_still_tracked() {
    // -34 dBFS: soft singing, well above the silence floor.
    assert_steady(
        &track(&sine(293.66, 1.0, 48_000, 0.02), 48_000),
        293.66,
        5.0,
    );
}

#[test]
fn clean_tones_report_high_clarity() {
    for estimate in track(&sine(440.0, 0.5, 48_000, 0.4), 48_000) {
        assert!(
            estimate.clarity > 0.99,
            "clean sine should be unambiguous, got clarity {:.3}",
            estimate.clarity
        );
        assert!(estimate.clarity <= 1.0, "clarity must stay in 0..=1");
    }
}

// -------------------------------------------------------------------- sweep

#[test]
fn a_220_to_440_sweep_is_tracked_monotonically() {
    let estimates = track(&exp_sweep(220.0, 440.0, 2.0, 48_000, 0.4), 48_000);
    let voiced = voiced(&estimates);
    assert!(
        voiced.len() > 150,
        "expected the whole 2s sweep voiced, got {} estimates",
        voiced.len()
    );

    // Monotonic: no estimate may be lower than the one before it. A pure
    // exponential glide has no reason to dip, so the only slack allowed is
    // the ±5 cents of measurement error the steady-tone tests already bound.
    for pair in voiced.windows(2) {
        let (t0, hz0, _) = pair[0];
        let (t1, hz1, _) = pair[1];
        let step = cents(hz1, hz0);
        assert!(
            step > -5.0,
            "sweep went backwards {step:.2} cents between t={t0:.1}ms \
             ({hz0:.2} Hz) and t={t1:.1}ms ({hz1:.2} Hz)"
        );
    }

    // ...and it actually traverses the octave it was asked to. The first
    // and last estimates are stamped half a window inside the take, so the
    // expected endpoints are the glide's value *there*, not 220 and 440.
    let (_, first_hz, _) = voiced[0];
    let (_, last_hz, _) = voiced[voiced.len() - 1];
    assert!(
        cents(first_hz, 220.0) > 0.0 && cents(first_hz, 220.0) < 30.0,
        "sweep should start just above 220 Hz, got {first_hz:.2}"
    );
    assert!(
        cents(last_hz, 440.0) < 0.0 && cents(last_hz, 440.0) > -40.0,
        "sweep should end just below 440 Hz, got {last_hz:.2}"
    );
}

#[test]
fn each_estimate_matches_the_pitch_at_its_own_timestamp() {
    // The load-bearing timing test: `t_ms` claims to be the instant an
    // estimate describes, so on a known glide every point must match the
    // glide's instantaneous frequency at exactly that instant. A window
    // whose analysis is off-centre — measuring the front of the buffer
    // while carrying a centre timestamp — fails this even though it passes
    // every steady-tone test.
    let secs = 2.0;
    let estimates = track(&exp_sweep(220.0, 440.0, secs, 48_000, 0.4), 48_000);
    for estimate in &estimates {
        let hz = estimate.hz.expect("the whole glide is voiced");
        let expected = 220.0 * 2f64.powf(estimate.t_ms / 1000.0 / secs);
        let error = cents(hz, expected);
        assert!(
            error.abs() <= 5.0,
            "t={:.1}ms: read {hz:.2} Hz, glide was at {expected:.2} Hz ({error:+.2} cents)",
            estimate.t_ms
        );
    }
}

// ------------------------------------------------------------ silence/noise

#[test]
fn silence_produces_no_pitch() {
    let estimates = track(&silence(0.5, 48_000), 48_000);
    assert!(!estimates.is_empty(), "silence must still tick the cadence");
    for estimate in &estimates {
        assert_eq!(
            estimate.hz, None,
            "silence reported {:?} Hz at t={:.1}ms",
            estimate.hz, estimate.t_ms
        );
        assert_eq!(estimate.clarity, 0.0);
    }
}

#[test]
fn white_noise_produces_no_pitch() {
    for seed in [1, 7, 12345, 987_654_321] {
        let estimates = track(&white_noise(0.5, 48_000, 0.4, seed), 48_000);
        assert!(!estimates.is_empty());
        for estimate in &estimates {
            assert_eq!(
                estimate.hz, None,
                "noise (seed {seed}) reported {:?} Hz at t={:.1}ms with clarity {:.3}",
                estimate.hz, estimate.t_ms, estimate.clarity
            );
        }
    }
}

#[test]
fn a_gap_of_silence_inside_a_take_breaks_the_track() {
    let mut signal = sine(440.0, 0.4, 48_000, 0.4);
    signal.extend(silence(0.4, 48_000));
    signal.extend(sine(440.0, 0.4, 48_000, 0.4));

    let estimates = track(&signal, 48_000);
    let unvoiced = estimates.iter().filter(|e| e.hz.is_none()).count();
    assert!(
        unvoiced > 20,
        "the silent stretch should read as unvoiced, got {unvoiced} gaps"
    );
    // Both tone stretches are found, and nothing between them is invented.
    let voiced = voiced(&estimates);
    assert!(voiced.len() > 40);
    for (t_ms, hz, _) in voiced {
        assert!(
            cents(hz, 440.0).abs() <= 5.0,
            "t={t_ms:.1}ms drifted to {hz:.2} Hz"
        );
    }
}

#[test]
fn the_clarity_floor_is_what_decides_voiced_from_unvoiced() {
    // A tone buried in equal-amplitude noise: some windows survive, some
    // don't. Whichever way each one goes, the published rule must hold —
    // `hz` is `Some` exactly when clarity reached the floor — because the
    // trace geometry keys its gaps off that same number.
    let tone = sine(293.66, 1.0, 48_000, 0.4);
    let noise = white_noise(1.0, 48_000, 0.4, 3);
    let mixed: Vec<f32> = tone.iter().zip(&noise).map(|(a, b)| a + b).collect();

    let estimates = track(&mixed, 48_000);
    let mut voiced = 0;
    let mut unvoiced = 0;
    for estimate in &estimates {
        assert_eq!(
            estimate.hz.is_some(),
            estimate.clarity >= mt_pitch::CLARITY_FLOOR,
            "t={:.1}ms: hz={:?} but clarity={:.3}",
            estimate.t_ms,
            estimate.hz,
            estimate.clarity
        );
        if estimate.hz.is_some() {
            voiced += 1
        } else {
            unvoiced += 1
        }
    }
    assert!(
        voiced > 0 && unvoiced > 0,
        "expected this mix to straddle the floor, got {voiced} voiced / {unvoiced} unvoiced"
    );
}

// ------------------------------------------------------------------ vibrato

#[test]
fn vibrato_is_tracked_not_flattened() {
    // 5 Hz, ±50 cents around D4. What sets the temporal resolution is the
    // period refinement's comparison span (~25 ms at D4), not the 42.7 ms
    // analysis window, so averaging costs only sinc(0.124) ≈ 0.975 of the
    // excursion. The assertions bracket that: they fail if the tracker
    // smears the vibrato flat *and* if it overshoots.
    let signal = vibrato(293.66, 5.0, 50.0, 2.0, 48_000, 0.4);
    let voiced = voiced(&track(&signal, 48_000));
    assert!(voiced.len() > 150, "expected a fully voiced 2s take");

    let offsets: Vec<f64> = voiced.iter().map(|(_, hz, _)| cents(*hz, 293.66)).collect();
    let max = offsets.iter().cloned().fold(f64::MIN, f64::max);
    let min = offsets.iter().cloned().fold(f64::MAX, f64::min);
    let mean = offsets.iter().sum::<f64>() / offsets.len() as f64;

    // Never wanders outside the true excursion by more than measurement error.
    assert!(
        max <= 55.0 && min >= -55.0,
        "vibrato excursion overshot: {min:.1}..{max:.1} cents"
    );
    // ...and is not averaged into a straight line: at least 0.9 of the true
    // depth on both sides (measured: ±49.0).
    assert!(
        max >= 45.0 && min <= -45.0,
        "vibrato was flattened: only {min:.1}..{max:.1} cents of ±50"
    );
    // The centre of the vibrato is still the note that was sung.
    assert!(
        mean.abs() <= 5.0,
        "vibrato centre drifted {mean:+.2} cents off D4"
    );
}

// ------------------------------------------------------- streaming behaviour

#[test]
fn block_size_does_not_change_the_estimates() {
    // P6 feeds whatever an AnalyserNode poll hands over; the review screen
    // feeds a whole take at once. Both must produce identical tracks.
    let signal = exp_sweep(220.0, 330.0, 0.5, 48_000, 0.4);

    let mut whole = Tracker::new(48_000);
    let in_one_go = whole.feed(&signal);

    for block in [1usize, 37, 128, 512, 4096] {
        let mut tracker = Tracker::new(48_000);
        let streamed: Vec<PitchEstimate> = signal
            .chunks(block)
            .flat_map(|chunk| tracker.feed(chunk))
            .collect();
        assert_eq!(
            streamed, in_one_go,
            "block size {block} changed the estimates"
        );
    }
}

#[test]
fn estimates_arrive_on_a_steady_cadence_starting_half_a_window_in() {
    let tracker = Tracker::new(48_000);
    let cadence = tracker.cadence_ms();
    let latency = tracker.latency_ms();
    drop(tracker);

    let estimates = track(&sine(440.0, 0.5, 48_000, 0.4), 48_000);
    assert!((estimates[0].t_ms - latency).abs() < 1e-9);
    for pair in estimates.windows(2) {
        assert!((pair[1].t_ms - pair[0].t_ms - cadence).abs() < 1e-9);
    }
}

#[test]
fn reset_restarts_the_clock_and_drops_buffered_audio() {
    let mut tracker = Tracker::new(48_000);
    let first = tracker.feed(&sine(440.0, 0.5, 48_000, 0.4));
    assert!(first.last().unwrap().t_ms > 400.0);

    tracker.reset();
    let second = tracker.feed(&sine(220.0, 0.5, 48_000, 0.4));
    assert!(
        (second[0].t_ms - first[0].t_ms).abs() < 1e-9,
        "clock restarted"
    );
    // No trace of the 440 Hz audio that was in the ring buffer.
    assert_steady(&second, 220.0, 5.0);
}

#[test]
fn a_partial_window_yields_nothing_yet() {
    let mut tracker = Tracker::new(48_000);
    let window = tracker.window_samples();
    let signal = sine(440.0, 1.0, 48_000, 0.4);
    assert!(tracker.feed(&signal[..window - 1]).is_empty());
    assert_eq!(tracker.feed(&signal[window - 1..window]).len(), 1);
}
