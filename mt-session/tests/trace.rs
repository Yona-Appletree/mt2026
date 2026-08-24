//! Trace geometry: the pure mapping from a pitch track to polylines. These
//! are the assertions the canvas code gets to not make.

use mt_pitch::PitchEstimate;
use mt_session::{TRACE_CLARITY_FLOOR, TRACE_RANGE_CENTS, TraceView, Viewport, trace_view};

const TARGET: f64 = 440.0;
/// 400x400 so the target line lands on 200 and ±200¢ on 0 and 400 — every
/// expected coordinate in here is exact.
const VIEWPORT: Viewport = Viewport {
    width: 400.0,
    height: 400.0,
};
const WINDOW_MS: f64 = 1000.0;

/// A pitch `cents` away from the target.
fn at(t_ms: f64, cents: f64) -> PitchEstimate {
    PitchEstimate {
        t_ms,
        hz: Some(TARGET * 2f64.powf(cents / 1200.0)),
        clarity: 1.0,
    }
}

fn unvoiced(t_ms: f64) -> PitchEstimate {
    PitchEstimate {
        t_ms,
        hz: None,
        clarity: 0.0,
    }
}

fn murky(t_ms: f64, cents: f64) -> PitchEstimate {
    PitchEstimate {
        clarity: TRACE_CLARITY_FLOOR - 0.05,
        ..at(t_ms, cents)
    }
}

fn view(track: &[PitchEstimate]) -> TraceView {
    trace_view(track, TARGET, VIEWPORT, WINDOW_MS)
}

/// Every point, ignoring where the segment boundaries fall.
fn points(view: &TraceView) -> Vec<(f32, f32)> {
    view.segments
        .iter()
        .flat_map(|segment| segment.points.iter().copied())
        .collect()
}

// -------------------------------------------------------------- y mapping

#[test]
fn an_on_target_pitch_sits_exactly_on_the_target_line() {
    let view = view(&[at(500.0, 0.0)]);
    assert_eq!(view.target_y, 200.0);
    assert_eq!(points(&view), vec![(200.0, 200.0)], "t=500ms is mid-window");
}

#[test]
fn cents_map_linearly_onto_the_viewport_with_sharp_above_flat() {
    // 400 px tall, ±200¢ across it: 1 cent = 1 px, sharp upward.
    let cases = [
        (200.0, 0.0), // top edge
        (100.0, 100.0),
        (50.0, 150.0),
        (0.0, 200.0), // the target line
        (-50.0, 250.0),
        (-100.0, 300.0),
        (-200.0, 400.0), // bottom edge
    ];
    for (cents, expected_y) in cases {
        let view = view(&[at(500.0, cents)]);
        let (_, y) = points(&view)[0];
        assert!(
            (y - expected_y).abs() < 0.01,
            "{cents:+} cents should map to y={expected_y}, got {y}"
        );
    }
}

#[test]
fn wild_pitches_are_clamped_to_the_edge_rather_than_dropped() {
    // An octave out is still *something the singer did*; the trace pins it
    // to the edge instead of silently losing the line.
    for (cents, expected_y) in [(1200.0, 0.0), (-1200.0, 400.0), (201.0, 0.0)] {
        let view = view(&[at(500.0, cents)]);
        assert_eq!(points(&view), vec![(200.0, expected_y)], "{cents:+} cents");
    }
    assert_eq!(TRACE_RANGE_CENTS, 200.0);
}

#[test]
fn the_target_line_follows_the_viewport_height() {
    let view = trace_view(&[], TARGET, Viewport::new(800.0, 300.0), WINDOW_MS);
    assert_eq!(view.target_y, 150.0);
}

// -------------------------------------------------------------- x mapping

#[test]
fn time_maps_across_the_window_left_to_right() {
    // A take shorter than the window starts at 0 and grows rightward.
    let view = view(&[
        at(0.0, 0.0),
        at(250.0, 0.0),
        at(500.0, 0.0),
        at(1000.0, 0.0),
    ]);
    assert_eq!(view.start_ms, 0.0);
    assert_eq!(view.end_ms, 1000.0);
    let xs: Vec<f32> = points(&view).iter().map(|(x, _)| *x).collect();
    assert_eq!(xs, vec![0.0, 100.0, 200.0, 400.0]);
}

#[test]
fn the_window_scrolls_once_the_take_outgrows_it() {
    let track: Vec<PitchEstimate> = (0..=30).map(|i| at(i as f64 * 100.0, 0.0)).collect();
    let view = view(&track);

    // Newest estimate is at 3000 ms, so the window shows 2000..3000.
    assert_eq!(view.start_ms, 2000.0);
    assert_eq!(view.end_ms, 3000.0);

    let drawn = points(&view);
    assert_eq!(drawn.len(), 11, "only the last second is drawn");
    assert_eq!(drawn.first().unwrap().0, 0.0, "t=2000ms is the left edge");
    assert_eq!(drawn.last().unwrap().0, 400.0, "t=3000ms is the right edge");
}

// ------------------------------------------------------------------- gaps

#[test]
fn unvoiced_estimates_break_the_line_instead_of_being_interpolated_across() {
    let view = view(&[
        at(0.0, 0.0),
        at(100.0, 0.0),
        unvoiced(200.0),
        unvoiced(300.0),
        at(400.0, 100.0),
        at(500.0, 100.0),
    ]);

    assert_eq!(view.segments.len(), 2, "the silence must split the line");
    assert_eq!(
        view.segments[0].points,
        vec![(0.0, 200.0), (40.0, 200.0)],
        "first run ends where the voice does"
    );
    assert_eq!(
        view.segments[1].points,
        vec![(160.0, 100.0), (200.0, 100.0)],
        "second run starts where it comes back"
    );
    // Nothing was invented in between: no point sits in the gap's x range.
    assert!(
        !points(&view).iter().any(|(x, _)| *x > 40.0 && *x < 160.0),
        "a point was drawn inside the gap"
    );
}

#[test]
fn low_clarity_estimates_are_gaps_too() {
    let view = view(&[at(0.0, 0.0), murky(100.0, 0.0), at(200.0, 0.0)]);
    assert_eq!(view.segments.len(), 2);
    assert_eq!(points(&view).len(), 2);
}

#[test]
fn clarity_exactly_at_the_floor_is_drawn() {
    let point = PitchEstimate {
        clarity: TRACE_CLARITY_FLOOR,
        ..at(500.0, 0.0)
    };
    assert_eq!(view(&[point]).segments.len(), 1);
}

#[test]
fn a_fully_unvoiced_track_draws_nothing_at_all() {
    let track: Vec<PitchEstimate> = (0..10).map(|i| unvoiced(i as f64 * 50.0)).collect();
    let view = view(&track);
    assert!(view.segments.is_empty());
    assert_eq!(view.target_y, 200.0, "but the target line still exists");
}

#[test]
fn leading_and_trailing_gaps_do_not_produce_empty_segments() {
    let view = view(&[
        unvoiced(0.0),
        unvoiced(100.0),
        at(200.0, 0.0),
        unvoiced(300.0),
    ]);
    assert_eq!(view.segments.len(), 1);
    assert_eq!(view.segments[0].points.len(), 1);
}

// ------------------------------------------------------------ degenerate

#[test]
fn an_empty_track_yields_an_empty_window_not_a_crash() {
    let view = view(&[]);
    assert!(view.segments.is_empty());
    assert_eq!(view.target_y, 200.0);
    assert_eq!(view.start_ms, 0.0);
    assert_eq!(view.end_ms, 1000.0);
}

#[test]
fn nonsense_inputs_produce_an_empty_trace_rather_than_nan() {
    let track = [at(500.0, 0.0)];
    for (target, window) in [
        (0.0, WINDOW_MS),
        (-440.0, WINDOW_MS),
        (f64::NAN, WINDOW_MS),
        (TARGET, 0.0),
        (TARGET, -1000.0),
        (TARGET, f64::NAN),
        (TARGET, f64::INFINITY),
    ] {
        let view = trace_view(&track, target, VIEWPORT, window);
        assert!(
            view.segments.is_empty(),
            "target={target} window={window} should draw nothing"
        );
        assert!(view.target_y.is_finite());
        assert!(view.start_ms.is_finite() && view.end_ms.is_finite());
    }
}

#[test]
fn a_track_of_one_point_is_a_one_point_segment() {
    // The canvas can draw it as a dot; dropping it would make a held note
    // flicker at the start of every take.
    let view = view(&[at(500.0, 25.0)]);
    assert_eq!(view.segments.len(), 1);
    assert_eq!(view.segments[0].points.len(), 1);
}

// -------------------------------------------------- live and review agree

#[test]
fn the_review_window_shows_a_whole_take_the_live_window_scrolls_it() {
    // Same track, two window widths: this is the only difference between
    // the 60 fps live trace and the review screen's full-take render.
    let track: Vec<PitchEstimate> = (0..=60).map(|i| at(i as f64 * 100.0, 0.0)).collect();

    let live = trace_view(&track, TARGET, VIEWPORT, 2_000.0);
    assert_eq!(live.start_ms, 4_000.0);
    assert_eq!(points(&live).len(), 21);

    let review = trace_view(&track, TARGET, VIEWPORT, 6_000.0);
    assert_eq!(review.start_ms, 0.0);
    assert_eq!(points(&review).len(), 61);
}
