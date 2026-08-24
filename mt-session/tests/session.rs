//! Table-driven coverage of the drill state machine: every event sequence
//! that matters, asserted on both the resulting state and the exact
//! commands emitted.

use mt_pitch::PitchEstimate;
use mt_session::{
    CaptureContext, Command, DRILL_TYPE_DEGREE, DrillSession, Event, NewSampleMeta, SessionState,
};
use mt_theory::{Degree, Key, Mode, PitchClass};

// ---------------------------------------------------------------- fixtures

fn context() -> CaptureContext {
    CaptureContext {
        sample_rate: 48_000,
        device_label: "MacBook Pro Microphone".to_string(),
        user_agent: "Mozilla/5.0 (test)".to_string(),
        app_git_sha: "abc1234".to_string(),
    }
}

fn session() -> DrillSession {
    DrillSession::new(context())
}

fn d_major() -> Key {
    Key::new(PitchClass::D, Mode::Major)
}

fn sol() -> Degree {
    Degree::new(5).unwrap()
}

/// Start the drill on sol in D major with the tonic at D4 — so the target
/// is A4 = 440 Hz, the one frequency every assertion in here can recognise
/// on sight.
fn start() -> Event {
    Event::Start {
        key: d_major(),
        degree: sol(),
        tonic_octave: 4,
    }
}

/// A stand-in for the tracker output the adapter hands back.
fn fake_track() -> Vec<PitchEstimate> {
    (0..10)
        .map(|i| PitchEstimate {
            t_ms: i as f64 * 10.0,
            hz: Some(438.0 + i as f64),
            clarity: 0.95,
        })
        .collect()
}

/// Runs a sequence of events, returning the commands from the last one.
fn run(session: &mut DrillSession, events: Vec<Event>) -> Vec<Command> {
    let mut last = Vec::new();
    for event in events {
        last = session.handle(event);
    }
    last
}

/// Drives a session to the point where the take has been sung and the
/// review screen is up.
fn in_review() -> DrillSession {
    let mut session = session();
    run(
        &mut session,
        vec![
            start(),
            Event::ReferenceDone,
            Event::Stop,
            Event::TrackReady(fake_track()),
        ],
    );
    session
}

// ------------------------------------------------------------- happy path

#[test]
fn the_happy_path_emits_its_commands_in_order() {
    let mut session = session();

    // Every step: the event, the commands it must produce, and a name for
    // the failure message.
    let steps: Vec<(&str, Event, Vec<Command>)> = vec![
        (
            "Start sounds the establishing arpeggio",
            start(),
            vec![Command::PlayArpeggio([
                293.6647679174076, // do  = D4
                369.9944227116344, // mi  = F#4
                440.0,             // sol = A4
                587.3295358348151, // do  = D5
            ])],
        ),
        (
            "the arpeggio ending opens the mic",
            Event::ReferenceDone,
            vec![Command::StartCapture],
        ),
        (
            "stopping closes the mic",
            Event::Stop,
            vec![Command::StopCapture],
        ),
        (
            "the recorded track moves us to review, silently",
            Event::TrackReady(fake_track()),
            vec![],
        ),
        ("rating is a pure state change", Event::Rate(4), vec![]),
        (
            "so is the note",
            Event::NoteEdited("flat on the approach".to_string()),
            vec![],
        ),
    ];

    for (why, event, expected) in steps {
        assert_eq!(session.handle(event), expected, "{why}");
    }

    // The prompt was by name only: no tone sounded before the singing.
    let commands = session.handle(Event::Save);
    let [Command::Persist(meta)] = commands.as_slice() else {
        panic!("Save should persist exactly once, got {commands:?}");
    };
    assert_eq!(meta.drill.r#type, DRILL_TYPE_DEGREE);
    assert_eq!(meta.drill.key, Some(PitchClass::D));
    assert_eq!(meta.drill.mode, Some(Mode::Major));
    assert_eq!(meta.drill.degree, Some(sol()));
    assert_eq!(meta.drill.target_hz, Some(440.0));
    assert_eq!(meta.self_rating, Some(4));
    assert_eq!(meta.note.as_deref(), Some("flat on the approach"));
    assert_eq!(meta.audio.sample_rate, 48_000);
    assert_eq!(meta.capture.device_label, "MacBook Pro Microphone");
    assert_eq!(meta.capture.app_git_sha, "abc1234");

    assert!(matches!(session.state(), SessionState::Saving { .. }));
    assert_eq!(
        session.handle(Event::SaveOk {
            id: "smp-20260819-113211-a1b2".to_string()
        }),
        vec![]
    );
    assert_eq!(
        session.state(),
        &SessionState::Saved {
            id: "smp-20260819-113211-a1b2".to_string()
        }
    );
}

#[test]
fn the_target_is_the_prompted_degree_in_the_chosen_key() {
    // Same degree, three keys and octaves: the target must come from
    // mt-theory every time, not from anything the adapter passes in later.
    let cases = [
        (PitchClass::D, Mode::Major, 4, 440.0), // sol of D4 major = A4
        (PitchClass::C, Mode::Major, 4, 392.0), // sol of C4 major = G4
        (PitchClass::A, Mode::Minor, 3, 220.0), // do of A3 minor = A3
    ];
    for (tonic, mode, octave, expected) in cases {
        let degree = if expected == 220.0 {
            Degree::new(1).unwrap()
        } else {
            sol()
        };
        let mut session = session();
        session.handle(Event::Start {
            key: Key::new(tonic, mode),
            degree,
            tonic_octave: octave,
        });
        let target = session.target_hz().unwrap();
        assert!(
            (target - expected).abs() < 0.01,
            "{tonic:?} {mode:?} octave {octave}: expected {expected}, got {target}"
        );
    }
}

// -------------------------------------------------------- rejection rules

#[test]
fn save_without_a_rating_is_rejected() {
    let mut session = in_review();
    let before = session.state().clone();

    assert!(!session.can_save());
    assert_eq!(session.handle(Event::Save), vec![], "nothing may be sent");
    assert_eq!(session.state(), &before, "and nothing may change");

    // A rating unlocks it.
    session.handle(Event::Rate(3));
    assert!(session.can_save());
    assert!(matches!(
        session.handle(Event::Save).as_slice(),
        [Command::Persist(_)]
    ));
}

#[test]
fn ratings_outside_one_to_five_are_dropped() {
    for bad in [0u8, 6, 9, 255] {
        let mut session = in_review();
        assert_eq!(session.handle(Event::Rate(bad)), vec![]);
        assert!(
            !session.can_save(),
            "rating {bad} should not have been stored"
        );
    }
    for good in [1u8, 2, 3, 4, 5] {
        let mut session = in_review();
        session.handle(Event::Rate(good));
        assert!(session.can_save(), "rating {good} should have been stored");
    }
}

#[test]
fn a_bad_rating_does_not_clobber_a_good_one() {
    let mut session = in_review();
    session.handle(Event::Rate(4));
    session.handle(Event::Rate(99));

    let commands = session.handle(Event::Save);
    let [Command::Persist(meta)] = commands.as_slice() else {
        panic!("expected a save");
    };
    assert_eq!(meta.self_rating, Some(4));
}

#[test]
fn stop_during_the_reference_cancels_cleanly() {
    let mut session = session();
    session.handle(start());
    assert!(matches!(
        session.state(),
        SessionState::PlayingReference { .. }
    ));

    assert_eq!(
        session.handle(Event::Stop),
        vec![],
        "nothing was captured, so nothing may be stopped"
    );
    assert_eq!(session.state(), &SessionState::Idle);
    assert_eq!(session.track(), &[], "and no take was invented");
    assert_eq!(session.target_hz(), None);

    // The session is genuinely reusable afterwards.
    assert!(matches!(
        session.handle(start()).as_slice(),
        [Command::PlayArpeggio(_)]
    ));
}

#[test]
fn a_second_stop_does_not_close_the_mic_twice() {
    // The auto-stop timer and the spacebar can both fire; the adapter must
    // be told once.
    let mut session = session();
    run(&mut session, vec![start(), Event::ReferenceDone]);

    assert_eq!(session.handle(Event::Stop), vec![Command::StopCapture]);
    assert_eq!(session.handle(Event::Stop), vec![]);
    assert_eq!(session.handle(Event::Stop), vec![]);
}

#[test]
fn events_out_of_order_are_ignored() {
    // Each row: how to get into a state, the event that makes no sense
    // there, and a label.
    let cases: Vec<(&str, Vec<Event>, Event)> = vec![
        ("ReferenceDone before Start", vec![], Event::ReferenceDone),
        ("Stop while idle", vec![], Event::Stop),
        ("Save while idle", vec![], Event::Save),
        ("Rate while idle", vec![], Event::Rate(3)),
        ("HearTarget while idle", vec![], Event::HearTarget),
        (
            "TrackReady during the arpeggio",
            vec![start()],
            Event::TrackReady(fake_track()),
        ),
        (
            "Rate while still listening",
            vec![start(), Event::ReferenceDone],
            Event::Rate(4),
        ),
        (
            "HearTarget before the singing",
            vec![start(), Event::ReferenceDone],
            Event::HearTarget,
        ),
        (
            "Save while still listening",
            vec![start(), Event::ReferenceDone],
            Event::Save,
        ),
        (
            "ReferenceDone twice",
            vec![start(), Event::ReferenceDone],
            Event::ReferenceDone,
        ),
        (
            "SaveOk without a save in flight",
            vec![start(), Event::ReferenceDone, Event::Stop],
            Event::SaveOk {
                id: "nope".to_string(),
            },
        ),
    ];

    for (label, setup, bad_event) in cases {
        let mut session = session();
        run(&mut session, setup);
        let before = session.state().clone();

        assert_eq!(session.handle(bad_event), vec![], "{label}: no commands");
        assert_eq!(session.state(), &before, "{label}: no state change");
    }
}

// ------------------------------------------------------------ review screen

#[test]
fn hear_target_plays_the_target_only_after_the_singing() {
    let mut session = in_review();
    assert_eq!(
        session.handle(Event::HearTarget),
        vec![Command::PlayTone(440.0)]
    );
    // ...and does not disturb the review in progress.
    assert!(matches!(session.state(), SessionState::Review { .. }));
    assert_eq!(session.track().len(), 10);
}

#[test]
fn the_review_holds_the_track_the_adapter_handed_back() {
    let session = in_review();
    assert_eq!(session.track(), fake_track().as_slice());
    assert_eq!(session.target_hz(), Some(440.0));
}

#[test]
fn an_emptied_note_is_omitted_rather_than_sent_blank() {
    let mut session = in_review();
    run(
        &mut session,
        vec![
            Event::Rate(5),
            Event::NoteEdited("typo".to_string()),
            Event::NoteEdited(String::new()),
        ],
    );
    let commands = session.handle(Event::Save);
    let [Command::Persist(meta)] = commands.as_slice() else {
        panic!("expected a save");
    };
    assert_eq!(meta.note, None);
}

// ------------------------------------------------------------ save failure

#[test]
fn a_failed_save_returns_to_review_with_everything_intact() {
    let mut session = in_review();
    run(
        &mut session,
        vec![
            Event::Rate(4),
            Event::NoteEdited("flat on the approach".to_string()),
            Event::Save,
        ],
    );

    assert_eq!(
        session.handle(Event::SaveErr("503 from the server".to_string())),
        vec![]
    );
    assert!(matches!(session.state(), SessionState::Review { .. }));
    assert_eq!(session.last_error(), Some("503 from the server"));
    assert_eq!(session.track(), fake_track().as_slice());
    assert!(session.can_save(), "the rating survived the failure");

    // Retrying sends exactly what it sent before, and clears the error.
    let commands = session.handle(Event::Save);
    let [Command::Persist(meta)] = commands.as_slice() else {
        panic!("expected a retry");
    };
    assert_eq!(meta.self_rating, Some(4));
    assert_eq!(meta.note.as_deref(), Some("flat on the approach"));
    assert_eq!(session.last_error(), None);
}

// ------------------------------------------------------------------ reset

#[test]
fn reset_returns_to_idle_from_anywhere() {
    let setups: Vec<(&str, Vec<Event>, Vec<Command>)> = vec![
        ("from idle", vec![], vec![]),
        ("from the arpeggio", vec![start()], vec![]),
        (
            "from listening, closing the mic on the way out",
            vec![start(), Event::ReferenceDone],
            vec![Command::StopCapture],
        ),
        (
            "from listening after a stop, without closing it twice",
            vec![start(), Event::ReferenceDone, Event::Stop],
            vec![],
        ),
        (
            "from review",
            vec![
                start(),
                Event::ReferenceDone,
                Event::Stop,
                Event::TrackReady(fake_track()),
            ],
            vec![],
        ),
        (
            "from saved",
            vec![
                start(),
                Event::ReferenceDone,
                Event::Stop,
                Event::TrackReady(fake_track()),
                Event::Rate(5),
                Event::Save,
                Event::SaveOk {
                    id: "smp-1".to_string(),
                },
            ],
            vec![],
        ),
    ];

    for (label, setup, expected) in setups {
        let mut session = session();
        run(&mut session, setup);
        assert_eq!(session.handle(Event::Reset), expected, "{label}");
        assert_eq!(session.state(), &SessionState::Idle, "{label}");
        assert_eq!(session.track(), &[], "{label}");
    }
}

#[test]
fn reset_clears_a_previous_save_failure() {
    let mut session = in_review();
    run(
        &mut session,
        vec![
            Event::Rate(4),
            Event::Save,
            Event::SaveErr("boom".to_string()),
        ],
    );
    assert_eq!(session.last_error(), Some("boom"));
    session.handle(Event::Reset);
    assert_eq!(session.last_error(), None);
}

// ------------------------------------------------------------- wire format

#[test]
fn the_upload_metadata_serializes_to_what_the_server_accepts() {
    let mut session = in_review();
    run(
        &mut session,
        vec![
            Event::Rate(4),
            Event::NoteEdited("flat on the approach".to_string()),
        ],
    );
    let commands = session.handle(Event::Save);
    let [Command::Persist(meta)] = commands.as_slice() else {
        panic!("expected a save");
    };

    // The archive row minus everything the server owns: no id, no
    // recorded_at, no schema_version, no audio.path, no audio.bit_depth.
    let expected: serde_json::Value = serde_json::from_str(
        r#"{
          "drill": {
            "type": "degree",
            "key": "D",
            "mode": "major",
            "degree": 5,
            "target_hz": 440.0
          },
          "self_rating": 4,
          "note": "flat on the approach",
          "audio": { "sample_rate": 48000 },
          "capture": {
            "device_label": "MacBook Pro Microphone",
            "user_agent": "Mozilla/5.0 (test)",
            "app_git_sha": "abc1234"
          }
        }"#,
    )
    .unwrap();

    let actual: serde_json::Value =
        serde_json::from_str(&serde_json::to_string(meta).unwrap()).unwrap();
    assert_eq!(actual, expected);

    // And it round-trips, so the server's deserializer and ours agree.
    let back: NewSampleMeta = serde_json::from_value(actual).unwrap();
    assert_eq!(&back, meta);
}

#[test]
fn absent_tags_are_omitted_not_nulled() {
    let mut session = in_review();
    session.handle(Event::Rate(2));
    let commands = session.handle(Event::Save);
    let [Command::Persist(meta)] = commands.as_slice() else {
        panic!("expected a save");
    };

    let json = serde_json::to_string(meta).unwrap();
    assert!(!json.contains("note"), "{json}");
    assert!(!json.contains("null"), "{json}");
}

#[test]
fn free_takes_carry_no_drill_target() {
    // Not produced by the degree drill, but the wire type has to express
    // the archive's other v0.1 drill type without nulls.
    let meta = NewSampleMeta {
        drill: mt_session::DrillMeta::free(),
        self_rating: None,
        note: None,
        audio: mt_session::AudioMeta {
            sample_rate: 44_100,
        },
        capture: mt_session::CaptureMeta {
            device_label: "d".to_string(),
            user_agent: "u".to_string(),
            app_git_sha: "s".to_string(),
        },
    };
    let json = serde_json::to_string(&meta).unwrap();
    assert_eq!(
        json,
        r#"{"drill":{"type":"free"},"audio":{"sample_rate":44100},"capture":{"device_label":"d","user_agent":"u","app_git_sha":"s"}}"#
    );
}
