use pitch_detection::detector::PitchDetector;
use pitch_detection::detector::mcleod::McLeodDetector;

/// One pitch estimate for one analysis window.
///
/// `clarity` is always populated and `hz` is `Some` exactly when clarity
/// reached [`CLARITY_FLOOR`] — so a view can dim segments it half-trusts
/// and see *why* a gap is a gap (0.0 means the window was too quiet or too
/// aperiodic to yield a candidate at all; 0.5 means a pitch was found and
/// judged too murky to draw).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchEstimate {
    /// Milliseconds since the first sample fed after construction or
    /// [`Tracker::reset`], stamped at the instant the estimate describes —
    /// the **centre** of its analysis window (see [`Tracker`] for what that
    /// means for latency).
    pub t_ms: f64,
    /// Estimated fundamental in Hz, or `None` for silence, noise, and
    /// anything below [`CLARITY_FLOOR`].
    pub hz: Option<f64>,
    /// How periodic the window actually was at the detected period, in
    /// `0.0..=1.0`: the normalized square difference `1 - d(τ)/(P₀ + P_τ)`.
    /// A clean sustained tone measures 1.000; full-scale white noise never
    /// gets far enough to be measured and reports 0.0.
    pub clarity: f64,
}

/// Nominal analysis-window length in milliseconds. See [`Tracker`] for why
/// ~43 ms won the tuning pass.
const TARGET_WINDOW_MS: f64 = 42.7;

/// Windows are powers of two (the detector FFTs `window + padding`), and
/// never shorter than this — below ~500 samples the window stops covering
/// two periods of a low voice at any plausible capture rate.
const MIN_WINDOW: usize = 512;

/// Upper bound on the window, so an unexpectedly high capture rate can't
/// blow up the per-hop FFT cost.
const MAX_WINDOW: usize = 8192;

/// Hop = window / [`HOP_DIVISOR`]: 75% window overlap, one estimate every
/// ~10.7 ms at 48 kHz.
const HOP_DIVISOR: usize = 4;

/// Minimum RMS a window must carry before the detector is even run. 0.003
/// is about -50 dBFS: below it we are looking at a silent or barely-open
/// mic, and any "pitch" found there is noise.
const MIN_RMS: f64 = 0.003;

/// Clarity a McLeod peak needs to be considered a *candidate* period at
/// all. Deliberately loose — McLeod's clarity decays with lag/window, so a
/// strict gate here would reject low notes before they are ever measured.
/// The real accept/reject decision is [`CLARITY_FLOOR`], applied to the
/// unbiased clarity computed at the refined lag.
const CANDIDATE_CLARITY: f64 = 0.3;

/// Clarity at or above which an estimate is reported as a pitch. Below it
/// [`PitchEstimate::hz`] is `None` — the window was noise, a consonant, or
/// a transition. Clean tones measure 1.000 and full-scale white noise never
/// even reaches [`CANDIDATE_CLARITY`], so the floor sits in a wide empty
/// middle: what it actually adjudicates is the murk in between, a tone at
/// roughly 0 dB SNR against room noise.
pub const CLARITY_FLOOR: f64 = 0.6;

/// Streaming pitch tracker: `&[f32]` in, one [`PitchEstimate`] per hop out.
///
/// Feed it whatever block sizes the source produces — an `AnalyserNode`
/// poll, a worklet block, or an entire recorded take — and it emits the
/// same estimates either way: the internal ring buffer decouples the feed
/// size from the analysis cadence, so the live path and the batch/review
/// path go through identical code (vision D6).
///
/// ## Window, hop, cadence, latency
///
/// Chosen from the capture rate at construction: the power-of-two window
/// nearest ~43 ms — 2048 samples at both rates that matter — and a hop of a
/// quarter of that (75% overlap).
///
/// | rate | window | hop | window ms | cadence | latency |
/// |---|---|---|---|---|---|
/// | 48 kHz | 2048 | 512 | 42.7 ms | 10.7 ms | 21.3 ms |
/// | 44.1 kHz | 2048 | 512 | 46.4 ms | 11.6 ms | 23.2 ms |
///
/// *Cadence* is the spacing of successive `t_ms` values — ~94 estimates a
/// second at 48 kHz, comfortably more than a 60 fps trace consumes.
/// *Latency* is how far behind real time an estimate is when it lands: each
/// estimate is stamped at the instant it actually measures (the centre of
/// its analysis window) but can only be computed once the whole window has
/// arrived, so the value describing instant *t* appears half a window after
/// *t*. Startup costs one extra window: the first estimate needs a full
/// buffer, so it is stamped at 21.3 ms and lands at 42.7 ms.
///
/// The plan's starting point was 4096/512 at 48 kHz; the fixture suite
/// moved it to 2048/512. 4096 buys nothing here — the period refinement
/// below, not the window length, sets the accuracy — while 85 ms of window
/// doubles the latency that the G1 "does it lag?" question is about and
/// smears fast vibrato. 2048 is still 9.4 periods of 220 Hz and 3.1 periods
/// of a D2 (73 Hz) low male voice, and lags up to half a window resolve
/// cleanly, so the usable floor is ~47 Hz at 48 kHz — an octave below any
/// voice this drill will meet.
///
/// ## How an estimate is made
///
/// Two stages. `pitch-detection`'s McLeod detector picks the *octave*
/// (which period, out of the ambiguous family, the window is built on),
/// then [`refine_period`] re-measures that period against a fixed-length
/// comparison window and reports an unbiased clarity — see its docs for why
/// the McLeod peak alone misses the ±5-cent bar at low pitches.
///
/// Measured against the fixture suite (release build): steady sines from
/// 73 Hz to 880 Hz land within **0.01 cents** at 48 kHz and 44.1 kHz, an
/// eight-harmonic sawtooth within 0.07 cents, a 5 Hz ±50-cent vibrato
/// recovers ±49 cents of its ±50 (the refinement's comparison span, ~25 ms
/// at D4, sets the temporal resolution rather than the 43 ms window), and a
/// 220→440 Hz glide is strictly monotonic with every point matching the
/// glide's instantaneous frequency at its own timestamp. Tones stay within
/// ~5 cents down to about 18 dB SNR against white noise. One estimate costs
/// ~0.17 ms on an M-series laptop — 1.6% of a core at the 10.7 ms cadence.
///
/// ## Silence and noise
///
/// A window reports `hz: None` when its RMS is below -50 dBFS (about a
/// closed mic), when nothing periodic enough to be a candidate is found, or
/// when the refined clarity is below [`CLARITY_FLOOR`]. White noise at full
/// scale never clears the first two hurdles at these window sizes, so it
/// reads as unvoiced rather than as a wandering pitch. Estimates are still
/// emitted on the same cadence throughout — a gap in pitch, not a gap in
/// time — which is what the trace geometry needs to draw a break instead of
/// interpolating across it.
pub struct Tracker {
    sample_rate: u32,
    window: usize,
    hop: usize,
    /// Ring of the most recent `window` samples.
    ring: Vec<f32>,
    /// Next write position in `ring`; once full, also the oldest sample.
    write: usize,
    /// How much of `ring` holds real samples (< window only at startup).
    filled: usize,
    /// Chronologically ordered copy of `ring`, reused across estimates.
    scratch: Vec<f64>,
    detector: McLeodDetector<f64>,
    /// Samples fed since construction/reset.
    samples_seen: u64,
    /// Value of `samples_seen` at which the next estimate fires.
    next_emit: u64,
    power_threshold: f64,
}

impl Tracker {
    /// Builds a tracker for a capture rate in Hz. Window and hop are
    /// derived from the rate (see the type docs); the buffers are
    /// allocated here and never reallocated during streaming.
    pub fn new(sample_rate: u32) -> Self {
        let window = window_for(sample_rate);
        Self::with_window(sample_rate, window, (window / HOP_DIVISOR).max(1))
    }

    fn with_window(sample_rate: u32, window: usize, hop: usize) -> Self {
        // Padding equal to the window makes the detector's zero-padded
        // autocorrelation exact for every lag it inspects, and keeps its
        // FFT length a power of two.
        let detector = McLeodDetector::new(window, window);
        Tracker {
            sample_rate,
            window,
            hop,
            ring: vec![0.0; window],
            write: 0,
            filled: 0,
            scratch: vec![0.0; window],
            detector,
            samples_seen: 0,
            next_emit: window as u64,
            power_threshold: window as f64 * MIN_RMS * MIN_RMS,
        }
    }

    /// Consumes a block of mono samples and returns the estimates that
    /// completed inside it — zero when the block is shorter than a hop,
    /// several when it is longer. Estimates come back in time order.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<PitchEstimate> {
        let mut out = Vec::with_capacity(samples.len() / self.hop + 1);
        for &sample in samples {
            self.ring[self.write] = sample;
            self.write = (self.write + 1) % self.window;
            if self.filled < self.window {
                self.filled += 1;
            }
            self.samples_seen += 1;
            if self.filled == self.window && self.samples_seen >= self.next_emit {
                out.push(self.estimate());
                self.next_emit = self.samples_seen + self.hop as u64;
            }
        }
        out
    }

    /// Drops all buffered audio and restarts the clock: the next estimate
    /// after a reset is stamped relative to the next sample fed. Call this
    /// between takes so a new recording's timestamps start at zero.
    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.write = 0;
        self.filled = 0;
        self.samples_seen = 0;
        self.next_emit = self.window as u64;
    }

    /// Capture rate this tracker was built for, in Hz.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Analysis window length in samples.
    pub fn window_samples(&self) -> usize {
        self.window
    }

    /// Distance between successive analysis windows, in samples.
    pub fn hop_samples(&self) -> usize {
        self.hop
    }

    /// Spacing between successive estimates' `t_ms`, in milliseconds.
    pub fn cadence_ms(&self) -> f64 {
        self.hop as f64 / self.sample_rate as f64 * 1000.0
    }

    /// How far behind real time an estimate lands, in milliseconds: half
    /// the analysis window (see the type docs).
    pub fn latency_ms(&self) -> f64 {
        self.window as f64 / 2.0 / self.sample_rate as f64 * 1000.0
    }

    fn estimate(&mut self) -> PitchEstimate {
        // When the ring is full, `write` points at the oldest sample.
        let oldest = self.write;
        for (i, slot) in self.scratch.iter_mut().enumerate() {
            let idx = if oldest + i >= self.window {
                oldest + i - self.window
            } else {
                oldest + i
            };
            *slot = self.ring[idx] as f64;
        }

        let centre = self.samples_seen as f64 - self.window as f64 / 2.0;
        let t_ms = centre / self.sample_rate as f64 * 1000.0;

        let candidate = self.detector.get_pitch(
            &self.scratch,
            self.sample_rate as usize,
            self.power_threshold,
            CANDIDATE_CLARITY,
        );
        let Some(candidate) = candidate else {
            // Too quiet, or nothing periodic enough to even try.
            return PitchEstimate {
                t_ms,
                hz: None,
                clarity: 0.0,
            };
        };

        let coarse_tau = self.sample_rate as f64 / candidate.frequency;
        let (tau, clarity) = match refine_period(&self.scratch, coarse_tau) {
            Some(refined) => refined,
            None => (coarse_tau, candidate.clarity.clamp(0.0, 1.0)),
        };

        PitchEstimate {
            t_ms,
            hz: (clarity >= CLARITY_FLOOR).then(|| self.sample_rate as f64 / tau),
            clarity,
        }
    }
}

/// Refines a candidate period (in samples) and measures how periodic the
/// window really is at that period. Returns `(period_samples, clarity)`, or
/// `None` when the candidate is too long to evaluate against half a window.
///
/// **Why this exists.** `pitch-detection`'s McLeod stage finds the right
/// *octave* reliably, but its peak lands systematically sharp: it correlates
/// a zero-padded signal (so the correlation decays like `1 - τ/N`) while its
/// `m(τ)` normalizer only subtracts the leading power term, leaving a
/// residual downward ramp across the peak that pulls the interpolated
/// maximum to a shorter period. The pull grows with `τ²/N` — negligible at
/// 440 Hz, but 9 cents at 220 Hz and worse below that, i.e. squarely outside
/// the ±5-cent oracle.
///
/// So we take only the octave decision from McLeod and re-measure the period
/// with a fixed-length comparison window (YIN's `d(τ)`, which has no
/// `1 - τ/N` envelope at all), searched ±2% around the candidate and
/// parabolically interpolated. The same pass yields an unbiased clarity —
/// the normalized square difference `1 - d(τ)/(P₀ + P_τ)` — which, unlike
/// McLeod's, does not fade as the note gets lower.
fn refine_period(window: &[f64], coarse_tau: f64) -> Option<(f64, f64)> {
    if !coarse_tau.is_finite() || coarse_tau < 2.0 {
        return None;
    }
    let n = window.len();
    // The comparison window is half the analysis window, so lags up to
    // `compare` samples can be evaluated without running off the end.
    let compare = n / 2;
    let max_tau = compare.saturating_sub(1);

    let span = (coarse_tau * 0.02).max(3.0).ceil() as usize;
    let nominal = coarse_tau.round() as usize;
    let lo = nominal.saturating_sub(span).max(2);
    let hi = (nominal + span).min(max_tau);
    if lo + 1 >= hi {
        return None;
    }

    // The comparison spans `compare + τ` samples; start it so that span sits
    // in the middle of the analysis window. Every lag in the search shares
    // this one start, so `d(τ)` stays smooth *and* the measurement is
    // centred on the instant `t_ms` claims — otherwise the refined pitch
    // would describe the front of the window while carrying a centre
    // timestamp, a silent ~9 ms lie in the trace.
    let start = (n.saturating_sub(compare + nominal)) / 2;
    let start = start.min(n.saturating_sub(compare + hi + 1));

    // `(square difference, power of the shifted half)` at one lag.
    let difference = |tau: usize| -> (f64, f64) {
        let mut sum_sq = 0.0;
        let mut shifted_power = 0.0;
        for i in 0..compare {
            let a = window[start + i];
            let b = window[start + i + tau];
            let delta = a - b;
            sum_sq += delta * delta;
            shifted_power += b * b;
        }
        (sum_sq, shifted_power)
    };
    let head_power: f64 = window[start..start + compare].iter().map(|x| x * x).sum();

    // Integer minimum of d(τ) across the search span.
    let mut best = lo;
    let mut best_d = f64::MAX;
    for tau in lo..=hi {
        let (d, _) = difference(tau);
        if d < best_d {
            best_d = d;
            best = tau;
        }
    }

    // Parabolic interpolation through the minimum and its neighbours.
    let (d_prev, _) = difference(best - 1);
    let (d_next, _) = difference(best + 1);
    let curvature = d_prev - 2.0 * best_d + d_next;
    let offset = if curvature > 0.0 {
        (0.5 * (d_prev - d_next) / curvature).clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let d_at_peak = best_d - 0.25 * (d_prev - d_next) * offset;

    let (_, shifted_power) = difference(best);
    let normalizer = head_power + shifted_power;
    let clarity = if normalizer > 0.0 {
        (1.0 - d_at_peak / normalizer).clamp(0.0, 1.0)
    } else {
        0.0
    };

    Some((best as f64 + offset, clarity))
}

/// The power-of-two window closest to [`TARGET_WINDOW_MS`] at this rate,
/// clamped to `[MIN_WINDOW, MAX_WINDOW]`.
fn window_for(sample_rate: u32) -> usize {
    let ideal = sample_rate as f64 * TARGET_WINDOW_MS / 1000.0;
    let exponent = ideal.max(1.0).log2().round().clamp(0.0, 20.0) as u32;
    (1usize << exponent).clamp(MIN_WINDOW, MAX_WINDOW)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_2048_at_both_common_capture_rates() {
        assert_eq!(window_for(48_000), 2048);
        assert_eq!(window_for(44_100), 2048);
    }

    #[test]
    fn window_is_clamped_at_extreme_rates() {
        assert_eq!(window_for(8_000), MIN_WINDOW);
        assert_eq!(window_for(768_000), MAX_WINDOW);
    }

    #[test]
    fn hop_is_a_quarter_window_and_cadence_matches() {
        let tracker = Tracker::new(48_000);
        assert_eq!(tracker.window_samples(), 2048);
        assert_eq!(tracker.hop_samples(), 512);
        assert!((tracker.cadence_ms() - 10.666).abs() < 0.01);
        assert!((tracker.latency_ms() - 21.333).abs() < 0.01);
    }
}
