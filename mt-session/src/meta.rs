use mt_theory::{Degree, Key, Mode, PitchClass};
use serde::{Deserialize, Serialize};

/// `drill.type` for the scale-degree drill.
pub const DRILL_TYPE_DEGREE: &str = "degree";

/// `drill.type` for an untargeted take (recorded outside a drill).
pub const DRILL_TYPE_FREE: &str = "free";

/// The metadata half of a sample upload: everything about a take the client
/// knows and the server does not.
///
/// This is a **wire type**, shaped to deserialize into the archive's
/// `SampleRow` on the far side. The server owns `id`, `recorded_at`,
/// `schema_version`, `audio.path`, and `audio.bit_depth` (single writer,
/// single id-minter — the house rule from lp2025's uid incident), and
/// derives `audio.channels`/`audio.duration_ms` from the PCM part it is
/// posted alongside. None of those appear here, on purpose: a field a
/// client can invent is a field two clients can disagree about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewSampleMeta {
    pub drill: DrillMeta,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub self_rating: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
    pub audio: AudioMeta,
    pub capture: CaptureMeta,
}

/// What was being drilled. `r#type` is free-form (`"degree"` and `"free"`
/// in v0.1); the rest describe a degree drill and are omitted entirely —
/// not nulled — for takes that had no target.
///
/// `key`/`mode`/`degree` carry mt-theory's own types, which serialize to
/// exactly the strings and integers the archive schema expects (`"D"`,
/// `"major"`, `5`), so there is no stringly-typed conversion layer to keep
/// in sync.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrillMeta {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub key: Option<PitchClass>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mode: Option<Mode>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub degree: Option<Degree>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_hz: Option<f64>,
}

impl DrillMeta {
    /// Metadata for a degree-drill take.
    pub fn degree(key: Key, degree: Degree, target_hz: f64) -> Self {
        DrillMeta {
            r#type: DRILL_TYPE_DEGREE.to_string(),
            key: Some(key.tonic),
            mode: Some(key.mode),
            degree: Some(degree),
            target_hz: Some(target_hz),
        }
    }

    /// Metadata for a take with no target.
    pub fn free() -> Self {
        DrillMeta {
            r#type: DRILL_TYPE_FREE.to_string(),
            key: None,
            mode: None,
            degree: None,
            target_hz: None,
        }
    }
}

/// How to interpret the PCM posted with this metadata. The rate is the
/// device's **actual** capture rate, never an assumed 48 kHz.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioMeta {
    pub sample_rate: u32,
}

/// Best-effort provenance for the capture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureMeta {
    pub device_label: String,
    pub user_agent: String,
    pub app_git_sha: String,
}

/// The facts about *this machine, this session* that the adapter knows and
/// the state machine does not: which mic is open, at what rate, and which
/// build is running. Handed to [`crate::DrillSession::new`] once, then
/// stamped onto every take it produces.
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureContext {
    pub sample_rate: u32,
    pub device_label: String,
    pub user_agent: String,
    pub app_git_sha: String,
}

impl CaptureContext {
    fn audio(&self) -> AudioMeta {
        AudioMeta {
            sample_rate: self.sample_rate,
        }
    }

    fn capture(&self) -> CaptureMeta {
        CaptureMeta {
            device_label: self.device_label.clone(),
            user_agent: self.user_agent.clone(),
            app_git_sha: self.app_git_sha.clone(),
        }
    }

    /// Builds the upload metadata for a finished take.
    pub(crate) fn new_sample_meta(
        &self,
        drill: DrillMeta,
        self_rating: Option<u8>,
        note: Option<String>,
    ) -> NewSampleMeta {
        NewSampleMeta {
            drill,
            self_rating,
            note,
            audio: self.audio(),
            capture: self.capture(),
        }
    }
}
