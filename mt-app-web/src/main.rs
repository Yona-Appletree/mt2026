//! `mt-app-web`: the Dioxus web frontend. Talks to `mt-server` over `/api`
//! (proxied in dev, same-origin in production).
//!
//! This is the humble layer. The only state it owns is *form* state — which
//! phase of record→tag→save the screen is in, and what's typed in the tag
//! fields. Capture is delegated to [`recorder`], the save to [`api`]. When
//! the drill arrives (P5/P6) its state machine lives in `mt-session`, not
//! here.

mod api;
#[cfg(debug_assertions)]
mod dev;
mod recorder;

use dioxus::prelude::*;
use futures_util::StreamExt;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::KeyboardEvent;

use api::{SaveRequest, SavedSample};
use recorder::{Recorder, Recording};

const STYLE: Asset = asset!("/assets/style.css");

/// `drill.type` for an untargeted take (plan.md's data contract).
const FREE_DRILL: &str = "free";

fn main() {
    dioxus::launch(app);
}

/// Where the single screen is in the record→tag→save loop.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Idle,
    Recording,
    /// A take is captured and the tag form is up.
    Tagging,
    /// The POST is in flight.
    Saving,
}

/// What the last completed save attempt did, for the result banner.
#[derive(Clone, Debug, PartialEq)]
enum Outcome {
    Saved(SavedSample),
    Failed(String),
}

/// Messages into the control coroutine. Everything that needs to `await` —
/// opening the mic, closing it, POSTing — happens there, so the view's
/// event handlers stay one line each.
enum Cmd {
    /// Start recording, or stop and move to tagging.
    Toggle,
    Save,
    Discard,
    #[cfg(debug_assertions)]
    TestTone,
}

#[component]
fn app() -> Element {
    let mut phase = use_signal(|| Phase::Idle);
    let mut outcome = use_signal(|| None::<Outcome>);
    let mut captured = use_signal(|| None::<Recording>);
    let mut started_at_ms = use_signal(|| 0.0_f64);
    let mut elapsed_ms = use_signal(|| 0_u32);

    // Tag form fields.
    let mut drill_type = use_signal(|| FREE_DRILL.to_string());
    let mut target = use_signal(String::new);
    let mut note = use_signal(String::new);
    let mut rating = use_signal(|| None::<u8>);

    let control = use_coroutine(move |mut rx: UnboundedReceiver<Cmd>| async move {
        // The live capture graph lives here, as a plain local: it is owned
        // by exactly one sequential task, so it needs no interior
        // mutability and can't be double-started.
        let mut live: Option<Recorder> = None;

        while let Some(cmd) = rx.next().await {
            let mut save_now = false;

            match cmd {
                Cmd::Toggle => match live.take() {
                    None => {
                        outcome.set(None);
                        captured.set(None);
                        elapsed_ms.set(0);
                        started_at_ms.set(js_sys::Date::now());
                        match Recorder::start().await {
                            Ok(recorder) => {
                                live = Some(recorder);
                                phase.set(Phase::Recording);
                            }
                            Err(err) => {
                                outcome.set(Some(Outcome::Failed(err)));
                                phase.set(Phase::Idle);
                            }
                        }
                    }
                    Some(recorder) => {
                        captured.set(Some(recorder.stop().await));
                        phase.set(Phase::Tagging);
                    }
                },
                Cmd::Save => save_now = true,
                Cmd::Discard => {
                    captured.set(None);
                    outcome.set(None);
                    reset_form(drill_type, target, note, rating);
                    phase.set(Phase::Idle);
                }
                #[cfg(debug_assertions)]
                Cmd::TestTone => {
                    // Deliberately the same path a real take takes: fill
                    // `captured` and the form, then fall into the save arm.
                    captured.set(Some(dev::test_tone()));
                    drill_type.set(FREE_DRILL.to_string());
                    target.set(String::new());
                    note.set("dev test tone".to_string());
                    rating.set(None);
                    save_now = true;
                }
            }

            if !save_now {
                continue;
            }
            let Some(take) = captured.peek().clone() else {
                continue;
            };
            phase.set(Phase::Saving);
            let request = SaveRequest {
                drill_type: non_empty(&drill_type.peek()).unwrap_or_else(|| FREE_DRILL.to_string()),
                self_rating: *rating.peek(),
                note: compose_note(&target.peek(), &note.peek()),
                sample_rate: take.sample_rate,
                device_label: take.device_label.clone(),
            };
            match api::save_sample(request, &take.samples).await {
                Ok(saved) => {
                    outcome.set(Some(Outcome::Saved(saved)));
                    captured.set(None);
                    reset_form(drill_type, target, note, rating);
                    phase.set(Phase::Idle);
                }
                Err(err) => {
                    outcome.set(Some(Outcome::Failed(err)));
                    phase.set(Phase::Tagging);
                }
            }
        }
    });

    // The one-keypress path. A document-level listener rather than an
    // element handler so it works no matter what has focus — guarded on
    // phase so Space and R type normally in the tag form's fields.
    use_hook(|| {
        let handler = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            if event.repeat() || event.meta_key() || event.ctrl_key() || event.alt_key() {
                return;
            }
            if matches!(*phase.peek(), Phase::Tagging | Phase::Saving) {
                return;
            }
            let key = event.key();
            if key == " " || key.eq_ignore_ascii_case("r") {
                // Space would otherwise scroll the page.
                event.prevent_default();
                control.send(Cmd::Toggle);
            }
        });
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            let _ = document
                .add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
        }
        Rc::new(handler)
    });

    // Elapsed-time ticks while recording. 100ms is well under the tenth of
    // a second the readout shows.
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(100).await;
            if *phase.peek() == Phase::Recording {
                let since = js_sys::Date::now() - *started_at_ms.peek();
                elapsed_ms.set(since.max(0.0) as u32);
            }
        }
    });

    let phase_now = phase();
    let recording = phase_now == Phase::Recording;
    let tagging = matches!(phase_now, Phase::Tagging | Phase::Saving);
    let rating_now = rating();

    let take_summary = captured
        .read()
        .as_ref()
        .map(describe_take)
        .unwrap_or_default();
    let (banner_class, banner_text) = match outcome.read().as_ref() {
        Some(Outcome::Saved(saved)) => (
            "banner banner-ok",
            format!("Saved {} → {}", saved.id, saved.path),
        ),
        Some(Outcome::Failed(err)) => ("banner banner-error", err.clone()),
        None => ("banner", String::new()),
    };

    rsx! {
        document::Stylesheet { href: STYLE }

        main { class: "app",
            header {
                h1 { "mt2026" }
                p { class: "tagline", "Record a take, tag it, keep it." }
            }

            section { class: if recording { "deck deck-live" } else { "deck" },
                button {
                    class: "record",
                    disabled: tagging,
                    onclick: move |_| control.send(Cmd::Toggle),
                    if recording { "Stop" } else { "Record" }
                }
                div { class: "timer", "{format_elapsed(elapsed_ms())}" }
                p { class: "hint",
                    if tagging { "Tag the take below." } else { "Press Space or R" }
                }
            }

            if tagging {
                section { class: "tag",
                    p { class: "summary", "{take_summary}" }

                    label { r#for: "drill-type", "Drill type" }
                    input {
                        id: "drill-type",
                        r#type: "text",
                        value: "{drill_type}",
                        placeholder: FREE_DRILL,
                        oninput: move |event| drill_type.set(event.value()),
                    }

                    label { r#for: "target", "Target (optional)" }
                    input {
                        id: "target",
                        r#type: "text",
                        value: "{target}",
                        placeholder: "e.g. D major, 5th",
                        oninput: move |event| target.set(event.value()),
                    }

                    span { class: "field-label", "How did it go?" }
                    div { class: "ratings",
                        for score in 1_u8..=5 {
                            button {
                                key: "{score}",
                                r#type: "button",
                                class: rating_class(rating_now, score),
                                disabled: phase_now == Phase::Saving,
                                onclick: move |_| {
                                    rating.set(if rating_now == Some(score) { None } else { Some(score) })
                                },
                                "{score}"
                            }
                        }
                    }

                    label { r#for: "note", "Note (optional)" }
                    textarea {
                        id: "note",
                        rows: "2",
                        value: "{note}",
                        oninput: move |event| note.set(event.value()),
                    }

                    div { class: "actions",
                        button {
                            class: "primary",
                            r#type: "button",
                            disabled: phase_now == Phase::Saving,
                            onclick: move |_| control.send(Cmd::Save),
                            if phase_now == Phase::Saving { "Saving…" } else { "Save" }
                        }
                        button {
                            r#type: "button",
                            disabled: phase_now == Phase::Saving,
                            onclick: move |_| control.send(Cmd::Discard),
                            "Discard"
                        }
                    }
                }
            }

            if !banner_text.is_empty() {
                p { class: banner_class, "{banner_text}" }
            }

            DevTools { control }
        }
    }
}

/// The end-to-end check hook: synthesizes a tone and pushes it through the
/// real save path. Compiled out of release bundles.
#[component]
fn DevTools(control: Coroutine<Cmd>) -> Element {
    #[cfg(debug_assertions)]
    return rsx! {
        section { class: "devtools",
            button {
                r#type: "button",
                onclick: move |_| control.send(Cmd::TestTone),
                "Save test tone"
            }
            span { "dev only · 1s of 440Hz through the real save path" }
        }
    };

    #[cfg(not(debug_assertions))]
    {
        let _ = control;
        rsx! {}
    }
}

/// `target` and `note` are two fields but one schema slot: v0.1 free-form
/// takes carry no `drill.target_hz`, so a typed target rides along in
/// `note` rather than being dropped.
fn compose_note(target: &str, note: &str) -> Option<String> {
    match (non_empty(target), non_empty(note)) {
        (None, None) => None,
        (None, Some(note)) => Some(note),
        (Some(target), None) => Some(format!("target: {target}")),
        (Some(target), Some(note)) => Some(format!("target: {target} — {note}")),
    }
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn reset_form(
    mut drill_type: Signal<String>,
    mut target: Signal<String>,
    mut note: Signal<String>,
    mut rating: Signal<Option<u8>>,
) {
    drill_type.set(FREE_DRILL.to_string());
    target.set(String::new());
    note.set(String::new());
    rating.set(None);
}

fn describe_take(take: &Recording) -> String {
    let seconds = take.samples.len() as f64 / take.sample_rate.max(1) as f64;
    format!(
        "{seconds:.1}s · {} Hz · {}",
        take.sample_rate, take.device_label
    )
}

fn format_elapsed(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}.{}", seconds / 60, seconds % 60, (ms % 1000) / 100)
}

fn rating_class(selected: Option<u8>, score: u8) -> &'static str {
    if selected == Some(score) {
        "rating rating-on"
    } else {
        "rating"
    }
}
