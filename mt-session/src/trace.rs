use mt_pitch::PitchEstimate;
use mt_theory::cents_between;

/// How far above and below the target line the trace can reach, in cents.
/// A quarter-tone is 50¢, a semitone 100¢, so ±200¢ shows a whole tone of
/// error either way — past that the singer is on a different note and the
/// exact distance stops being interesting.
pub const TRACE_RANGE_CENTS: f64 = 200.0;

/// Clarity a point needs before it is drawn at all.
///
/// Stricter than [`mt_pitch::CLARITY_FLOOR`] on purpose: the tracker's job
/// is to answer "was there a pitch here", the trace's job is to draw a line
/// the singer will believe. A wobbling line through half-voiced consonants
/// reads as bad singing rather than as absent data, so the marginal middle
/// becomes a gap instead.
pub const TRACE_CLARITY_FLOOR: f64 = 0.7;

/// The rectangle the trace is drawn into, in whatever units the caller
/// draws in (CSS pixels for a canvas). Y grows downward, canvas-style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    pub fn new(width: f32, height: f32) -> Self {
        Viewport { width, height }
    }
}

/// One unbroken run of the pitch line. Consecutive points inside a segment
/// are connected; separate segments are not — that is what a gap *is*.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceSegment {
    pub points: Vec<(f32, f32)>,
}

/// Ready-to-draw geometry: the pure output of [`trace_view`]. The canvas
/// code that consumes this does nothing but stroke polylines (vision D12 —
/// the view stays humble, the geometry stays testable).
#[derive(Debug, Clone, PartialEq)]
pub struct TraceView {
    /// Polylines to stroke, in time order.
    pub segments: Vec<TraceSegment>,
    /// Y of the target line — where a perfectly on-pitch note sits.
    pub target_y: f32,
    /// Time at the left edge of the viewport, in tracker milliseconds.
    pub start_ms: f64,
    /// Time at the right edge.
    pub end_ms: f64,
}

/// Maps a pitch track onto a viewport, against a target frequency.
///
/// - **Y** is cents from `target_hz`, clamped to ±[`TRACE_RANGE_CENTS`],
///   with the target line at the vertical centre and sharp pitches above
///   flat ones.
/// - **X** is time across a scrolling `window_ms`-wide window ending at the
///   newest estimate (or at `window_ms` itself, so a take shorter than the
///   window grows from the left rather than sliding). Estimates older than
///   the window are dropped.
/// - **Gaps**: an unvoiced estimate, or one below [`TRACE_CLARITY_FLOOR`],
///   ends the current segment. Nothing is interpolated across it — a line
///   drawn through silence is a claim the singer never made.
///
/// Pure and allocation-only: no clock is read, so the same call renders the
/// live trace at 60 fps and the review screen's full-take trace.
pub fn trace_view(
    track: &[PitchEstimate],
    target_hz: f64,
    viewport: Viewport,
    window_ms: f64,
) -> TraceView {
    let target_y = viewport.height / 2.0;

    // No sane mapping exists for a zero-width time window or a nonsense
    // target; better an empty trace than a canvas full of NaN.
    if !window_ms.is_finite() || window_ms <= 0.0 || !target_hz.is_finite() || target_hz <= 0.0 {
        return TraceView {
            segments: Vec::new(),
            target_y,
            start_ms: 0.0,
            end_ms: 0.0,
        };
    }

    let end_ms = track
        .last()
        .map(|estimate| estimate.t_ms)
        .unwrap_or(window_ms)
        .max(window_ms);
    let start_ms = end_ms - window_ms;

    let mut view = TraceView {
        segments: Vec::new(),
        target_y,
        start_ms,
        end_ms,
    };

    let mut current: Vec<(f32, f32)> = Vec::new();
    for estimate in track {
        if estimate.t_ms < start_ms {
            continue;
        }
        let drawable = estimate
            .hz
            .filter(|hz| *hz > 0.0 && estimate.clarity >= TRACE_CLARITY_FLOOR);
        match drawable {
            Some(hz) => {
                let cents =
                    cents_between(hz, target_hz).clamp(-TRACE_RANGE_CENTS, TRACE_RANGE_CENTS);
                let x = ((estimate.t_ms - start_ms) / window_ms) as f32 * viewport.width;
                let y = target_y - (cents / TRACE_RANGE_CENTS) as f32 * target_y;
                current.push((x, y));
            }
            None => flush(&mut current, &mut view.segments),
        }
    }
    flush(&mut current, &mut view.segments);
    view
}

/// Ends the run in progress, if it has anything in it.
fn flush(current: &mut Vec<(f32, f32)>, segments: &mut Vec<TraceSegment>) {
    if !current.is_empty() {
        segments.push(TraceSegment {
            points: std::mem::take(current),
        });
    }
}
