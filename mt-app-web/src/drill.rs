//! The drill screen: the adapter around `mt_session::DrillSession`.
//!
//! Split exactly along vision D12's control/data-plane line:
//!
//! - **Control plane** — UI gestures become [`Event`]s, fed to the state
//!   machine in the control coroutine; the [`Command`]s it returns are
//!   executed here (oscillators, mic, upload). Take-level only.
//! - **Data plane** — every 128-frame worklet block is tapped into an
//!   `mt_pitch::Tracker` and the estimates appended to a shared buffer; a
//!   `requestAnimationFrame` loop draws that buffer through the *pure*
//!   `mt_session::trace_view` geometry. No per-frame event ever touches the
//!   state machine, and the canvas code does nothing but stroke polylines.
//!
//! The recorded take is the same sample stream the live tracker saw, and the
//! review trace is recomputed from the recorded PCM through a fresh tracker
//! — the exact code path future offline analysis will use (vision D6).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use dioxus::prelude::*;
use futures_util::StreamExt;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen::closure::Closure;
use wasm_bindgen_futures::spawn_local;
use web_sys::{
    AudioContext, CanvasRenderingContext2d, GainNode, HtmlCanvasElement, KeyboardEvent,
    OscillatorType,
};

use mt_pitch::{PitchEstimate, Tracker};
use mt_session::{
    CaptureContext, Command, DrillSession, Event, SessionState, Viewport, trace_view,
};
use mt_theory::{Degree, Key, Mode, PitchClass};

use crate::api;
use crate::recorder::{Recorder, Recording};

/// The one canvas. Looked up by id — it lives in this screen's rsx and the
/// screen is never unmounted (only hidden), so the element is stable.
const CANVAS_ID: &str = "drill-trace";

/// How much take the scrolling live trace shows.
const LIVE_WINDOW_MS: f64 = 4_000.0;

/// Reference arpeggio pacing: each note sounds for NOTE_MS inside a
/// NOTE_SLOT_MS slot, plus a short tail before the prompt flips to "sing".
const NOTE_MS: f64 = 500.0;
const NOTE_SLOT_MS: f64 = 550.0;
const REFERENCE_TAIL_MS: u32 = 150;

/// A take ends after this long if the singer doesn't stop it first.
const AUTO_STOP_MS: u32 = 6_000;

/// Reference/target tone level. Comfortable over laptop speakers while the
/// mic is (or is about to be) open.
const TONE_GAIN: f32 = 0.22;

/// Messages into the drill's control coroutine.
pub enum DrillMsg {
    /// A UI gesture, already translated into the session's vocabulary.
    Ui(Event),
    /// The arpeggio task finished sounding (generation-tagged: a stale one
    /// from a cancelled or superseded drill is dropped here, and the state
    /// machine would reject it anyway).
    ReferenceDone(u64),
    /// The auto-stop timer fired.
    AutoStop(u64),
}

/// The drill screen. `screen` says which screen is visible — it gates the
/// keyboard shortcuts, not the rendering (this screen stays mounted while
/// hidden so a live mic can never be orphaned by a screen switch).
#[component]
pub fn DrillScreen(screen: Signal<crate::Screen>) -> Element {
    // Drill setup the singer picks between takes. Major-only for v0.1:
    // mt-theory's minor-mode solfège is a documented gap, so offering the
    // toggle would prompt names the theory crate can't say yet.
    let key_tonic = use_signal(|| PitchClass::D);
    let tonic_octave = use_signal(|| 3_i32);
    let degree = use_signal(|| Degree::new(5).expect("5 is in 1..=7"));

    // The view's mirror of the state machine, refreshed after every event.
    // The session itself lives in the coroutine; this signal exists so rsx
    // can render from it.
    let state = use_signal(|| SessionState::Idle);

    let control = use_coroutine(move |rx: UnboundedReceiver<DrillMsg>| drill_loop(rx, state));

    // Keyboard: Space drives the loop phase-appropriately; digits rate and
    // Enter saves on the review screen. Guarded so nothing fires while the
    // other screen is up or while typing in the note field.
    use_hook(|| {
        let handler = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            if *screen.peek() != crate::Screen::Drill
                || event.repeat()
                || event.meta_key()
                || event.ctrl_key()
                || event.alt_key()
                || is_typing_target(&event)
            {
                return;
            }
            let key = event.key();
            let msg = match (&*state.peek(), key.as_str()) {
                (SessionState::Idle | SessionState::Saved { .. }, " ") => Some(Event::Start {
                    key: Key::new(*key_tonic.peek(), Mode::Major),
                    degree: *degree.peek(),
                    tonic_octave: *tonic_octave.peek(),
                }),
                (SessionState::PlayingReference { .. } | SessionState::Listening { .. }, " ") => {
                    Some(Event::Stop)
                }
                (SessionState::Review { .. }, digit @ ("1" | "2" | "3" | "4" | "5")) => {
                    Some(Event::Rate(digit.as_bytes()[0] - b'0'))
                }
                (SessionState::Review { .. }, "Enter") => Some(Event::Save),
                (SessionState::Review { .. }, "h") => Some(Event::HearTarget),
                _ => None,
            };
            if let Some(event_out) = msg {
                event.prevent_default();
                control.send(DrillMsg::Ui(event_out));
            }
        });
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            let _ = document
                .add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
        }
        Rc::new(handler)
    });

    let state_now = state();
    let key_now = Key::new(key_tonic(), Mode::Major);
    let degree_now = degree();
    let octave_now = tonic_octave();
    let in_setup = matches!(state_now, SessionState::Idle | SessionState::Saved { .. });

    rsx! {
        section { class: "drill",
            // ---- setup row -------------------------------------------
            div { class: "drill-setup",
                label { r#for: "drill-key", "Key" }
                select {
                    id: "drill-key",
                    disabled: !in_setup,
                    onchange: move |event| {
                        if let Some(pc) = parse_pitch_class(&event.value()) {
                            let mut key_tonic = key_tonic;
                            key_tonic.set(pc);
                        }
                    },
                    for pc in PitchClass::ALL {
                        option {
                            value: pitch_class_label(pc),
                            selected: pc == key_now.tonic,
                            "{pitch_class_label(pc)} major"
                        }
                    }
                }
                label { r#for: "drill-octave", "Do" }
                select {
                    id: "drill-octave",
                    disabled: !in_setup,
                    onchange: move |event| {
                        if let Ok(octave) = event.value().parse::<i32>() {
                            let mut tonic_octave = tonic_octave;
                            tonic_octave.set(octave);
                        }
                    },
                    for octave in 2..=5 {
                        option {
                            value: "{octave}",
                            selected: octave == octave_now,
                            "{pitch_class_label(key_now.tonic)}{octave}"
                        }
                    }
                }
            }

            // ---- degree row ------------------------------------------
            div { class: "drill-degrees",
                for n in 1..=7_u8 {
                    button {
                        key: "{n}",
                        r#type: "button",
                        class: if Degree::new(n) == Some(degree_now) { "degree degree-on" } else { "degree" },
                        disabled: !in_setup,
                        onclick: move |_| {
                            let mut degree = degree;
                            degree.set(Degree::new(n).expect("1..=7"));
                        },
                        "{Degree::new(n).expect(\"1..=7\").solfege_name()}"
                    }
                }
                button {
                    r#type: "button",
                    class: "degree degree-random",
                    disabled: !in_setup,
                    onclick: move |_| {
                        let n = (js_sys::Math::random() * 7.0) as u8 + 1;
                        let mut degree = degree;
                        degree.set(Degree::new(n.min(7)).expect("1..=7"));
                        control.send(DrillMsg::Ui(Event::Start {
                            key: Key::new(*key_tonic.peek(), Mode::Major),
                            degree: *degree.peek(),
                            tonic_octave: *tonic_octave.peek(),
                        }));
                    },
                    "random"
                }
            }

            // ---- the prompt ------------------------------------------
            {drill_prompt(&state_now, key_now, degree_now)}

            // ---- the trace -------------------------------------------
            canvas { id: CANVAS_ID, class: "trace" }

            // ---- phase controls --------------------------------------
            {drill_controls(&state_now, control, key_now, degree_now, octave_now)}
        }
    }
}

/// The big center line: what the singer should be doing right now.
fn drill_prompt(state: &SessionState, key: Key, degree: Degree) -> Element {
    let (class, text) = match state {
        SessionState::Idle => (
            "prompt prompt-dim",
            format!(
                "Space to hear do–mi–sol–do in {} major, then sing {}",
                pitch_class_label(key.tonic),
                degree.solfege_name()
            ),
        ),
        SessionState::PlayingReference { key, .. } => (
            "prompt",
            format!("Listen — {} major", pitch_class_label(key.tonic)),
        ),
        SessionState::Listening { degree, .. } => (
            "prompt prompt-sing",
            format!("Sing: {}", degree.solfege_name()),
        ),
        SessionState::Review { degree, .. } => (
            "prompt prompt-dim",
            format!("How was your {}?", degree.solfege_name()),
        ),
        SessionState::Saving { .. } => ("prompt prompt-dim", "Saving…".to_string()),
        SessionState::Saved { id } => (
            "prompt prompt-dim",
            format!("Saved {id} — Space to go again"),
        ),
    };
    rsx! { p { class, "{text}" } }
}

/// The buttons below the canvas, per phase.
fn drill_controls(
    state: &SessionState,
    control: Coroutine<DrillMsg>,
    key: Key,
    degree: Degree,
    tonic_octave: i32,
) -> Element {
    match state {
        SessionState::Idle | SessionState::Saved { .. } => rsx! {
            div { class: "actions",
                button {
                    class: "primary",
                    r#type: "button",
                    onclick: move |_| control.send(DrillMsg::Ui(Event::Start { key, degree, tonic_octave })),
                    "Start (Space)"
                }
            }
        },
        SessionState::PlayingReference { .. } | SessionState::Listening { .. } => rsx! {
            div { class: "actions",
                button {
                    r#type: "button",
                    onclick: move |_| control.send(DrillMsg::Ui(Event::Stop)),
                    "Stop (Space)"
                }
            }
        },
        SessionState::Review {
            self_rating, note, ..
        } => {
            let rating_now = *self_rating;
            let note_now = note.clone().unwrap_or_default();
            rsx! {
                div { class: "tag",
                    span { class: "field-label", "How did it go? (1–5)" }
                    div { class: "ratings",
                        for score in 1_u8..=5 {
                            button {
                                key: "{score}",
                                r#type: "button",
                                class: if rating_now == Some(score) { "rating rating-on" } else { "rating" },
                                onclick: move |_| control.send(DrillMsg::Ui(Event::Rate(score))),
                                "{score}"
                            }
                        }
                    }
                    label { r#for: "drill-note", "Note (optional)" }
                    textarea {
                        id: "drill-note",
                        rows: "2",
                        value: "{note_now}",
                        oninput: move |event| control.send(DrillMsg::Ui(Event::NoteEdited(event.value()))),
                    }
                    div { class: "actions",
                        button {
                            r#type: "button",
                            onclick: move |_| control.send(DrillMsg::Ui(Event::HearTarget)),
                            "Hear target (H)"
                        }
                        button {
                            class: "primary",
                            r#type: "button",
                            disabled: rating_now.is_none(),
                            onclick: move |_| control.send(DrillMsg::Ui(Event::Save)),
                            "Save (Enter)"
                        }
                        button {
                            r#type: "button",
                            onclick: move |_| control.send(DrillMsg::Ui(Event::Reset)),
                            "Discard"
                        }
                    }
                }
            }
        }
        SessionState::Saving { .. } => rsx! {
            div { class: "actions",
                button { class: "primary", r#type: "button", disabled: true, "Saving…" }
            }
        },
    }
}

// ====================================================================
// The control coroutine: owns the session, the mic, and the playback.
// ====================================================================

async fn drill_loop(rx: UnboundedReceiver<DrillMsg>, mut state_out: Signal<SessionState>) {
    // Timer tasks (arpeggio-done, auto-stop) re-enter through their own
    // sender, merged with the UI inbox into one queue — one message stream,
    // one place events are ordered.
    let (timer_tx, timer_rx) = futures_channel::mpsc::unbounded::<DrillMsg>();
    let mut inbox = futures_util::stream::select(rx, timer_rx);

    // The placeholder rate/label are overwritten with the take's actual
    // facts before anything is persisted — see `Command::Persist` below.
    // The true rate is unknowable before the mic opens, and the session is
    // constructed before any drill starts.
    let mut session = DrillSession::new(CaptureContext {
        sample_rate: 48_000,
        device_label: "unknown (pre-capture)".to_string(),
        user_agent: api::user_agent(),
        app_git_sha: api::APP_GIT_SHA.to_string(),
    });

    // Adapter-owned kit, all take-scoped.
    let mut playback: Option<AudioContext> = None;
    let mut live: Option<Recorder> = None;
    let mut take: Option<Recording> = None;
    let live_track: Rc<RefCell<Vec<PitchEstimate>>> = Rc::new(RefCell::new(Vec::new()));
    let mut raf: Option<RafLoop> = None;
    // Bumped whenever a timer-backed step (re)starts; stale timer messages
    // carry the old value and are dropped.
    let mut generation: u64 = 0;

    while let Some(msg) = inbox.next().await {
        let event = match msg {
            DrillMsg::Ui(event) => event,
            DrillMsg::ReferenceDone(tag) if tag == generation => Event::ReferenceDone,
            DrillMsg::AutoStop(tag) if tag == generation => Event::Stop,
            _ => continue, // stale timer from a cancelled/superseded step
        };

        // A user-initiated Stop or Reset during the arpeggio must actually
        // silence it; the state machine (correctly) has no command for
        // sound it never asked to keep.
        if matches!(event, Event::Stop | Event::Reset)
            && matches!(session.state(), SessionState::PlayingReference { .. })
        {
            close_playback(&mut playback);
        }

        // Feed the event, then run every command — commands can produce
        // follow-on events (TrackReady, SaveOk…), hence the queue.
        let mut events = std::collections::VecDeque::from([event]);
        while let Some(event) = events.pop_front() {
            for command in session.handle(event) {
                match command {
                    Command::PlayArpeggio(freqs) => {
                        close_playback(&mut playback);
                        generation += 1;
                        match play_notes(&freqs.map(|hz| (hz, NOTE_MS))) {
                            Ok(ctx) => {
                                playback = Some(ctx);
                                schedule(
                                    &timer_tx,
                                    freqs.len() as u32 * NOTE_SLOT_MS as u32 + REFERENCE_TAIL_MS,
                                    DrillMsg::ReferenceDone(generation),
                                );
                            }
                            Err(err) => {
                                web_sys::console::error_1(&JsValue::from_str(&err));
                                events.push_back(Event::Reset);
                            }
                        }
                    }

                    Command::StartCapture => {
                        live_track.borrow_mut().clear();
                        let sink = Rc::clone(&live_track);
                        let target_hz = session.target_hz().unwrap_or(0.0);
                        match Recorder::start_with_tap(move |sample_rate| {
                            let mut tracker = Tracker::new(sample_rate);
                            Box::new(move |block: &[f32]| {
                                sink.borrow_mut().extend(tracker.feed(block));
                            })
                        })
                        .await
                        {
                            Ok(recorder) => {
                                live = Some(recorder);
                                generation += 1;
                                schedule(&timer_tx, AUTO_STOP_MS, DrillMsg::AutoStop(generation));
                                let track = Rc::clone(&live_track);
                                // `replace` drops any previous loop (which
                                // cancels its pending frame) as it installs
                                // the new one.
                                drop(raf.replace(RafLoop::start(move || {
                                    draw_trace(&track.borrow(), target_hz, LIVE_WINDOW_MS);
                                })));
                            }
                            Err(err) => {
                                web_sys::console::error_1(&JsValue::from_str(&err));
                                events.push_back(Event::Reset);
                            }
                        }
                    }

                    Command::StopCapture => {
                        drop(raf.take()); // stops the loop, cancels the pending frame
                        generation += 1; // invalidates the auto-stop timer
                        if let Some(recorder) = live.take() {
                            let recording = recorder.stop().await;
                            // Review trace = the recorded PCM through a
                            // fresh tracker: the batch path, byte-identical
                            // input to what offline analysis will read.
                            let mut tracker = Tracker::new(recording.sample_rate);
                            let track = tracker.feed(&recording.samples);
                            take = Some(recording);
                            events.push_back(Event::TrackReady(track));
                        }
                    }

                    Command::PlayTone(hz) => {
                        close_playback(&mut playback);
                        match play_notes(&[(hz, 1_000.0)]) {
                            Ok(ctx) => playback = Some(ctx),
                            Err(err) => web_sys::console::error_1(&JsValue::from_str(&err)),
                        }
                    }

                    Command::Persist(mut meta) => {
                        match take.as_ref() {
                            Some(recording) => {
                                // The session stamped placeholder capture
                                // facts; the adapter owns the real ones.
                                // The server decodes the PCM at this rate,
                                // so this is correctness, not bookkeeping.
                                meta.audio.sample_rate = recording.sample_rate;
                                meta.capture.device_label = recording.device_label.clone();
                                let result = api::save_meta(&meta, &recording.samples).await;
                                events.push_back(match result {
                                    Ok(saved) => Event::SaveOk { id: saved.id },
                                    Err(err) => Event::SaveErr(err),
                                });
                            }
                            None => {
                                events.push_back(Event::SaveErr(
                                    "no recorded take to save".to_string(),
                                ));
                            }
                        }
                    }
                }
            }

            // Entering Review: draw the whole take once, full-width.
            if let SessionState::Review {
                track, target_hz, ..
            } = session.state()
            {
                let window_ms = track
                    .last()
                    .map(|estimate| estimate.t_ms)
                    .unwrap_or(0.0)
                    .max(1_000.0);
                draw_trace(track, *target_hz, window_ms);
            }
        }

        state_out.set(session.state().clone());
    }
}

/// Sends a message back into the drill inbox after `delay_ms`. If the
/// coroutine is gone by then, the send fails silently — exactly right.
fn schedule(
    timer_tx: &futures_channel::mpsc::UnboundedSender<DrillMsg>,
    delay_ms: u32,
    msg: DrillMsg,
) {
    let timer_tx = timer_tx.clone();
    spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(delay_ms).await;
        let _ = timer_tx.unbounded_send(msg);
    });
}

/// Closes (and thereby silences) the playback context, if any.
fn close_playback(playback: &mut Option<AudioContext>) {
    if let Some(ctx) = playback.take() {
        let _ = ctx.close();
    }
}

/// Plays a sequence of `(hz, duration_ms)` notes back-to-back in
/// [`NOTE_SLOT_MS`] slots on a fresh `AudioContext`, each under a short
/// gain envelope so nothing clicks. Returns the context so the caller can
/// silence it early by closing it.
fn play_notes(notes: &[(f64, f64)]) -> Result<AudioContext, String> {
    let ctx = AudioContext::new().map_err(|err| audio_err("create playback context", &err))?;
    let now = ctx.current_time();

    for (index, (hz, duration_ms)) in notes.iter().enumerate() {
        let start = now + index as f64 * (NOTE_SLOT_MS / 1_000.0);
        let end = start + duration_ms / 1_000.0;

        let osc = ctx
            .create_oscillator()
            .map_err(|err| audio_err("create oscillator", &err))?;
        osc.set_type(OscillatorType::Triangle);
        osc.frequency().set_value(*hz as f32);

        let gain: GainNode = ctx
            .create_gain()
            .map_err(|err| audio_err("create gain", &err))?;
        let level = gain.gain();
        // 20ms attack, 50ms release: inaudible as an envelope, decisive
        // against clicks.
        let _ = level.set_value_at_time(0.0, start);
        let _ = level.linear_ramp_to_value_at_time(TONE_GAIN, start + 0.02);
        let _ = level.set_value_at_time(TONE_GAIN, end - 0.05);
        let _ = level.linear_ramp_to_value_at_time(0.0, end);

        osc.connect_with_audio_node(&gain)
            .map_err(|err| audio_err("connect oscillator", &err))?;
        gain.connect_with_audio_node(&ctx.destination())
            .map_err(|err| audio_err("connect gain", &err))?;
        let _ = osc.start_with_when(start);
        let _ = osc.stop_with_when(end + 0.02);
    }

    Ok(ctx)
}

fn audio_err(context: &str, err: &JsValue) -> String {
    format!("failed to {context}: {err:?}")
}

// ====================================================================
// The data plane: rAF loop + canvas drawing.
// ====================================================================

/// The self-referential cell a rAF loop keeps its own callback in (the
/// callback needs to re-request itself; the loop needs to free it).
type FrameCallback = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

/// A running `requestAnimationFrame` loop. Dropping it stops the loop *and*
/// cancels the in-flight frame, so the callback closure is never invoked
/// after it is freed.
struct RafLoop {
    active: Rc<Cell<bool>>,
    frame_id: Rc<Cell<i32>>,
    _callback: FrameCallback,
}

impl RafLoop {
    fn start(mut draw: impl FnMut() + 'static) -> Self {
        let active = Rc::new(Cell::new(true));
        let frame_id = Rc::new(Cell::new(0));
        let callback: FrameCallback = Rc::new(RefCell::new(None));

        let active_tick = Rc::clone(&active);
        let frame_tick = Rc::clone(&frame_id);
        let callback_tick = Rc::downgrade(&callback);
        *callback.borrow_mut() = Some(Closure::new(move || {
            if !active_tick.get() {
                return;
            }
            draw();
            if let Some(callback) = callback_tick.upgrade()
                && let Some(closure) = callback.borrow().as_ref()
            {
                frame_tick.set(request_frame(closure));
            }
        }));

        if let Some(closure) = callback.borrow().as_ref() {
            frame_id.set(request_frame(closure));
        }

        RafLoop {
            active,
            frame_id,
            _callback: callback,
        }
    }
}

impl Drop for RafLoop {
    fn drop(&mut self) {
        self.active.set(false);
        if let Some(window) = web_sys::window() {
            let _ = window.cancel_animation_frame(self.frame_id.get());
        }
    }
}

fn request_frame(closure: &Closure<dyn FnMut()>) -> i32 {
    web_sys::window()
        .and_then(|window| {
            window
                .request_animation_frame(closure.as_ref().unchecked_ref())
                .ok()
        })
        .unwrap_or(0)
}

/// Strokes the current track onto the canvas. All geometry comes from the
/// pure `trace_view`; this function draws lines and nothing else.
fn draw_trace(track: &[PitchEstimate], target_hz: f64, window_ms: f64) {
    let Some(canvas) = canvas() else { return };
    let Some(ctx) = context_2d(&canvas) else {
        return;
    };

    // Match the backing store to CSS size × devicePixelRatio so lines stay
    // crisp on retina displays; coordinates below are in CSS pixels.
    let dpr = web_sys::window()
        .map(|window| window.device_pixel_ratio())
        .unwrap_or(1.0);
    let css_width = f64::from(canvas.client_width()).max(1.0);
    let css_height = f64::from(canvas.client_height()).max(1.0);
    let device_width = (css_width * dpr) as u32;
    let device_height = (css_height * dpr) as u32;
    if canvas.width() != device_width || canvas.height() != device_height {
        canvas.set_width(device_width);
        canvas.set_height(device_height);
    }
    let _ = ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    ctx.clear_rect(0.0, 0.0, css_width, css_height);

    let viewport = Viewport::new(css_width as f32, css_height as f32);
    let view = trace_view(track, target_hz, viewport, window_ms);

    // Gridlines at ±100¢ (the semitones either side of the target).
    ctx.set_line_width(1.0);
    ctx.set_stroke_style_str("#2c323c");
    for fraction in [0.25, 0.75] {
        let y = css_height * fraction;
        ctx.begin_path();
        ctx.move_to(0.0, y);
        ctx.line_to(css_width, y);
        ctx.stroke();
    }

    // The target line.
    ctx.set_stroke_style_str("#6ea8ff");
    ctx.set_line_width(2.0);
    ctx.begin_path();
    ctx.move_to(0.0, f64::from(view.target_y));
    ctx.line_to(css_width, f64::from(view.target_y));
    ctx.stroke();

    // The voice.
    ctx.set_stroke_style_str("#ffd166");
    ctx.set_line_width(3.0);
    for segment in &view.segments {
        let mut points = segment.points.iter();
        let Some((x, y)) = points.next() else {
            continue;
        };
        ctx.begin_path();
        ctx.move_to(f64::from(*x), f64::from(*y));
        for (x, y) in points {
            ctx.line_to(f64::from(*x), f64::from(*y));
        }
        ctx.stroke();
    }
}

fn canvas() -> Option<HtmlCanvasElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id(CANVAS_ID)?
        .dyn_into::<HtmlCanvasElement>()
        .ok()
}

fn context_2d(canvas: &HtmlCanvasElement) -> Option<CanvasRenderingContext2d> {
    canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<CanvasRenderingContext2d>()
        .ok()
}

// ====================================================================
// Small helpers.
// ====================================================================

/// True when the key event is headed for a text field — those keystrokes
/// belong to typing, not to the drill.
fn is_typing_target(event: &KeyboardEvent) -> bool {
    event
        .target()
        .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
        .map(|element| {
            let tag = element.tag_name();
            tag.eq_ignore_ascii_case("input") || tag.eq_ignore_ascii_case("textarea")
        })
        .unwrap_or(false)
}

fn pitch_class_label(pc: PitchClass) -> &'static str {
    match pc {
        PitchClass::C => "C",
        PitchClass::CSharp => "C#",
        PitchClass::D => "D",
        PitchClass::DSharp => "D#",
        PitchClass::E => "E",
        PitchClass::F => "F",
        PitchClass::FSharp => "F#",
        PitchClass::G => "G",
        PitchClass::GSharp => "G#",
        PitchClass::A => "A",
        PitchClass::ASharp => "A#",
        PitchClass::B => "B",
    }
}

fn parse_pitch_class(label: &str) -> Option<PitchClass> {
    PitchClass::ALL
        .into_iter()
        .find(|pc| pitch_class_label(*pc) == label)
}
