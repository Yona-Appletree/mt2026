use mt_pitch::PitchEstimate;
use mt_theory::{Degree, Key, arpeggio_do_mi_sol_do, degree_to_hz};

use crate::meta::{CaptureContext, DrillMeta, NewSampleMeta};

/// Lowest and highest self-rating the archive schema accepts.
const RATING_RANGE: std::ops::RangeInclusive<u8> = 1..=5;

/// Where a drill has got to. Every field a later stage needs is carried
/// forward here rather than recomputed, so a take can never be saved
/// against a target other than the one the singer was actually shown.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionState {
    /// Nothing in flight; waiting for [`Event::Start`].
    Idle,
    /// The establishing arpeggio is sounding. The prompt ("sing sol") is
    /// on screen, the target tone is deliberately *not* played.
    PlayingReference {
        key: Key,
        degree: Degree,
        tonic_octave: i32,
        target_hz: f64,
    },
    /// The mic is open and the live trace is running.
    Listening {
        key: Key,
        degree: Degree,
        tonic_octave: i32,
        target_hz: f64,
        /// A [`Event::Stop`] has been acted on and we are waiting for the
        /// adapter to hand back the recorded track. Stops the second Stop
        /// (auto-stop racing the spacebar) from ordering capture off twice.
        stopping: bool,
    },
    /// The take is over: full-take trace on screen, target tone available
    /// to play, rating and note being entered.
    Review {
        key: Key,
        degree: Degree,
        tonic_octave: i32,
        target_hz: f64,
        track: Vec<PitchEstimate>,
        self_rating: Option<u8>,
        note: Option<String>,
    },
    /// Upload in flight. The take is held intact so a failure can drop
    /// straight back to [`SessionState::Review`] with the rating and note
    /// the user already typed.
    Saving {
        key: Key,
        degree: Degree,
        tonic_octave: i32,
        target_hz: f64,
        track: Vec<PitchEstimate>,
        self_rating: u8,
        note: Option<String>,
    },
    /// Saved, with the id the server minted.
    Saved { id: String },
}

/// Everything that can happen to a drill. Adapters translate UI gestures,
/// audio callbacks, and HTTP results into these; nothing else gets in.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// User started a drill on this degree, with the tonic placed in
    /// `tonic_octave` (scientific pitch notation — 4 puts D major's do at
    /// D4).
    Start {
        key: Key,
        degree: Degree,
        tonic_octave: i32,
    },
    /// The arpeggio finished sounding.
    ReferenceDone,
    /// Stop — spacebar, the auto-stop timer, or cancelling the arpeggio.
    Stop,
    /// The adapter has run the recorded PCM through the tracker.
    TrackReady(Vec<PitchEstimate>),
    /// Self-rating, 1..=5. Anything else is rejected.
    Rate(u8),
    /// The note field changed.
    NoteEdited(String),
    /// Play the target pitch, so the singer can hear what they were aiming
    /// at — *after* the take, never before it (audiation, not matching).
    ///
    /// **Deviation:** this event is not in the phase file's event list,
    /// which nonetheless lists [`Command::PlayTone`] as an output. Some
    /// event has to ask for it; this is that event.
    HearTarget,
    /// Save the take.
    Save,
    /// The server accepted the upload and minted this id.
    SaveOk { id: String },
    /// The upload failed; the message is for the user.
    SaveErr(String),
    /// Throw the take away and go back to [`SessionState::Idle`].
    Reset,
}

/// What the adapter must do. Returned as plain data from
/// [`DrillSession::handle`] — no port traits, no trait objects, so a test
/// asserts on a `Vec<Command>` instead of instrumenting a fake.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sound the do-mi-sol-do establishing arpeggio at these frequencies,
    /// then fire [`Event::ReferenceDone`].
    PlayArpeggio([f64; 4]),
    /// Open the mic and start the live trace.
    StartCapture,
    /// Close the mic; deliver the recorded track as [`Event::TrackReady`].
    StopCapture,
    /// Sound this frequency (the review screen's "hear the target").
    PlayTone(f64),
    /// Upload this metadata together with the PCM the adapter is holding;
    /// report back with [`Event::SaveOk`] or [`Event::SaveErr`].
    Persist(NewSampleMeta),
}

/// The drill's control plane: events in, state and commands out.
///
/// Deliberately ignorant of everything real — no audio, no network, no
/// clock, no PCM. The adapter owns those, and owns the 60 fps live trace
/// entirely (vision D12): per-frame pitch estimates never pass through
/// here, only take-level events.
///
/// ```
/// use mt_session::{CaptureContext, Command, DrillSession, Event, SessionState};
/// use mt_theory::{Degree, Key, Mode, PitchClass};
///
/// let mut session = DrillSession::new(CaptureContext {
///     sample_rate: 48_000,
///     device_label: "MacBook Pro Microphone".into(),
///     user_agent: "test".into(),
///     app_git_sha: "abc1234".into(),
/// });
///
/// let commands = session.handle(Event::Start {
///     key: Key::new(PitchClass::D, Mode::Major),
///     degree: Degree::new(5).unwrap(),
///     tonic_octave: 4,
/// });
/// assert!(matches!(commands.as_slice(), [Command::PlayArpeggio(_)]));
/// assert_eq!(session.target_hz(), Some(440.0));
/// ```
#[derive(Debug, Clone)]
pub struct DrillSession {
    state: SessionState,
    capture: CaptureContext,
    last_error: Option<String>,
}

impl DrillSession {
    /// Starts an idle session that will stamp every take with `capture`.
    pub fn new(capture: CaptureContext) -> Self {
        DrillSession {
            state: SessionState::Idle,
            capture,
            last_error: None,
        }
    }

    /// The current state.
    pub fn state(&self) -> &SessionState {
        &self.state
    }

    /// The frequency the singer is (or was) aiming at, once a drill has
    /// started and until it is saved.
    pub fn target_hz(&self) -> Option<f64> {
        match &self.state {
            SessionState::Idle | SessionState::Saved { .. } => None,
            SessionState::PlayingReference { target_hz, .. }
            | SessionState::Listening { target_hz, .. }
            | SessionState::Review { target_hz, .. }
            | SessionState::Saving { target_hz, .. } => Some(*target_hz),
        }
    }

    /// The recorded track, once there is one.
    pub fn track(&self) -> &[PitchEstimate] {
        match &self.state {
            SessionState::Review { track, .. } | SessionState::Saving { track, .. } => track,
            _ => &[],
        }
    }

    /// Whether [`Event::Save`] would be accepted right now — the same rule
    /// `handle` applies, exposed so a view can disable the button instead
    /// of letting the user press a dead one.
    pub fn can_save(&self) -> bool {
        matches!(
            &self.state,
            SessionState::Review {
                self_rating: Some(_),
                ..
            }
        )
    }

    /// The message from the last failed save, cleared as soon as anything
    /// else succeeds.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Feeds one event through the machine and returns what the adapter
    /// should do about it. Events that make no sense in the current state
    /// (and rejected ones — see the module docs) return no commands and
    /// leave the state untouched.
    pub fn handle(&mut self, event: Event) -> Vec<Command> {
        let state = std::mem::replace(&mut self.state, SessionState::Idle);
        let (next, commands) = self.transition(state, event);
        self.state = next;
        commands
    }

    fn transition(&mut self, state: SessionState, event: Event) -> (SessionState, Vec<Command>) {
        match (state, event) {
            // ---- starting a drill -------------------------------------
            (
                SessionState::Idle,
                Event::Start {
                    key,
                    degree,
                    tonic_octave,
                },
            ) => {
                self.last_error = None;
                let target_hz = degree_to_hz(key, degree, tonic_octave);
                (
                    SessionState::PlayingReference {
                        key,
                        degree,
                        tonic_octave,
                        target_hz,
                    },
                    vec![Command::PlayArpeggio(arpeggio_do_mi_sol_do(
                        key,
                        tonic_octave,
                    ))],
                )
            }

            // ---- reference → listening --------------------------------
            (
                SessionState::PlayingReference {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                },
                Event::ReferenceDone,
            ) => (
                SessionState::Listening {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    stopping: false,
                },
                vec![Command::StartCapture],
            ),

            // Cancelling during the arpeggio: nothing has been captured, so
            // there is nothing to stop and nothing to review. The adapter
            // silences its own oscillator.
            (SessionState::PlayingReference { .. }, Event::Stop) => (SessionState::Idle, vec![]),

            // ---- listening → review -----------------------------------
            (
                SessionState::Listening {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    stopping,
                },
                Event::Stop,
            ) => {
                let commands = if stopping {
                    // Auto-stop and the spacebar both fired; one is enough.
                    vec![]
                } else {
                    vec![Command::StopCapture]
                };
                (
                    SessionState::Listening {
                        key,
                        degree,
                        tonic_octave,
                        target_hz,
                        stopping: true,
                    },
                    commands,
                )
            }

            (
                SessionState::Listening {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    ..
                },
                Event::TrackReady(track),
            ) => (
                SessionState::Review {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    track,
                    self_rating: None,
                    note: None,
                },
                vec![],
            ),

            // ---- tagging the take -------------------------------------
            (
                SessionState::Review {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    track,
                    self_rating,
                    note,
                },
                Event::Rate(rating),
            ) => {
                // Out-of-range ratings are dropped: the archive schema says
                // 1..=5, and a state machine that quietly stores a 9 makes
                // the server the one that has to care.
                let self_rating = if RATING_RANGE.contains(&rating) {
                    Some(rating)
                } else {
                    self_rating
                };
                (
                    SessionState::Review {
                        key,
                        degree,
                        tonic_octave,
                        target_hz,
                        track,
                        self_rating,
                        note,
                    },
                    vec![],
                )
            }

            (
                SessionState::Review {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    track,
                    self_rating,
                    ..
                },
                Event::NoteEdited(text),
            ) => (
                SessionState::Review {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    track,
                    self_rating,
                    note: (!text.is_empty()).then_some(text),
                },
                vec![],
            ),

            (
                SessionState::Review {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    track,
                    self_rating,
                    note,
                },
                Event::HearTarget,
            ) => (
                SessionState::Review {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    track,
                    self_rating,
                    note,
                },
                vec![Command::PlayTone(target_hz)],
            ),

            // ---- saving -----------------------------------------------
            (
                SessionState::Review {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    track,
                    self_rating: Some(rating),
                    note,
                },
                Event::Save,
            ) => {
                self.last_error = None;
                let meta = self.capture.new_sample_meta(
                    DrillMeta::degree(key, degree, target_hz),
                    Some(rating),
                    note.clone(),
                );
                (
                    SessionState::Saving {
                        key,
                        degree,
                        tonic_octave,
                        target_hz,
                        track,
                        self_rating: rating,
                        note,
                    },
                    vec![Command::Persist(meta)],
                )
            }

            (SessionState::Saving { .. }, Event::SaveOk { id }) => {
                self.last_error = None;
                (SessionState::Saved { id }, vec![])
            }

            // A failed upload must not cost the user their take or their
            // tags — drop back into Review with everything intact.
            (
                SessionState::Saving {
                    key,
                    degree,
                    tonic_octave,
                    target_hz,
                    track,
                    self_rating,
                    note,
                },
                Event::SaveErr(message),
            ) => {
                self.last_error = Some(message);
                (
                    SessionState::Review {
                        key,
                        degree,
                        tonic_octave,
                        target_hz,
                        track,
                        self_rating: Some(self_rating),
                        note,
                    },
                    vec![],
                )
            }

            // ---- reset ------------------------------------------------
            // From Listening the mic is still open, so tear it down rather
            // than leaving the adapter to notice.
            (SessionState::Listening { stopping, .. }, Event::Reset) => {
                self.last_error = None;
                let commands = if stopping {
                    vec![]
                } else {
                    vec![Command::StopCapture]
                };
                (SessionState::Idle, commands)
            }
            (_, Event::Reset) => {
                self.last_error = None;
                (SessionState::Idle, vec![])
            }

            // ---- everything else is rejected --------------------------
            (state, _) => (state, vec![]),
        }
    }
}
