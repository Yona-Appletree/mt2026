//! Row types for `log.jsonl`, schema v1. Field names, presence, and nesting
//! come from plan.md's "The data contract" — treat this file as a
//! transcription of that spec, not a place for taste. See the ADR at
//! `docs/adr/2026-08-19-archive-format.md` for the rationale.

use serde::{Deserialize, Serialize};

/// The archive schema version this crate reads and writes. Bump only when
/// the on-disk shape changes, and document the migration in the ADR.
pub const SCHEMA_VERSION: u32 = 1;

/// The `kind` value every row written by this crate carries today. The
/// field exists so the log can carry other kinds of entries later without
/// a schema bump.
pub const KIND_SAMPLE: &str = "sample";

/// One line of `log.jsonl`: a single recorded take plus its tags.
///
/// Field order here matches the plan.md example row, so `serde_json`
/// serialization order matches it too (JSON object key order isn't
/// semantically meaningful, but matching it makes diffing against the spec
/// trivial).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SampleRow {
    pub schema_version: u32,
    pub id: String,
    /// RFC3339 timestamp with local offset, e.g. `2026-08-19T11:32:11-07:00`.
    pub recorded_at: String,
    pub kind: String,
    pub drill: DrillMeta,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub self_rating: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
    pub audio: AudioMeta,
    pub capture: CaptureMeta,
}

/// What was being drilled when the take was recorded.
///
/// `r#type` is required and free-form (`"degree"` and `"free"` in v0.1);
/// the rest describe a degree drill and are absent (not merely null) for
/// free-form takes, per plan.md's data contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrillMeta {
    #[serde(rename = "type")]
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub degree: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub target_hz: Option<f64>,
}

/// The WAV file this row points to and how to interpret its bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioMeta {
    /// Day-dir-relative path, e.g. `samples/113211-a1b2.wav`.
    pub path: String,
    /// The device's actual capture rate — never assumed (plan.md Q12).
    pub sample_rate: u32,
    pub channels: u16,
    pub bit_depth: u16,
    pub duration_ms: u64,
}

/// Best-effort provenance for the capture. All fields required by the
/// schema (v0.1 expects the web adapter to always have them at hand).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaptureMeta {
    pub device_label: String,
    pub user_agent: String,
    pub app_git_sha: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact example row from plan.md's "The data contract" section.
    /// This is the contract: field names and nesting must match it, not
    /// the other way around.
    const EXAMPLE_ROW_JSON: &str = r#"{
      "schema_version": 1,
      "id": "smp-20260819-113211-a1b2",
      "recorded_at": "2026-08-19T11:32:11-07:00",
      "kind": "sample",
      "drill": {
        "type": "degree",
        "key": "D",
        "mode": "major",
        "degree": 5,
        "target_hz": 440.0
      },
      "self_rating": 4,
      "note": "flat on the approach",
      "audio": {
        "path": "samples/113211-a1b2.wav",
        "sample_rate": 48000,
        "channels": 1,
        "bit_depth": 16,
        "duration_ms": 3210
      },
      "capture": {
        "device_label": "MacBook Pro Microphone",
        "user_agent": "…",
        "app_git_sha": "abc1234"
      }
    }"#;

    #[test]
    fn example_row_deserializes() {
        let row: SampleRow = serde_json::from_str(EXAMPLE_ROW_JSON).unwrap();
        assert_eq!(row.schema_version, 1);
        assert_eq!(row.id, "smp-20260819-113211-a1b2");
        assert_eq!(row.recorded_at, "2026-08-19T11:32:11-07:00");
        assert_eq!(row.kind, "sample");
        assert_eq!(row.drill.r#type, "degree");
        assert_eq!(row.drill.key.as_deref(), Some("D"));
        assert_eq!(row.drill.mode.as_deref(), Some("major"));
        assert_eq!(row.drill.degree, Some(5));
        assert_eq!(row.drill.target_hz, Some(440.0));
        assert_eq!(row.self_rating, Some(4));
        assert_eq!(row.note.as_deref(), Some("flat on the approach"));
        assert_eq!(row.audio.path, "samples/113211-a1b2.wav");
        assert_eq!(row.audio.sample_rate, 48000);
        assert_eq!(row.audio.channels, 1);
        assert_eq!(row.audio.bit_depth, 16);
        assert_eq!(row.audio.duration_ms, 3210);
        assert_eq!(row.capture.device_label, "MacBook Pro Microphone");
        assert_eq!(row.capture.app_git_sha, "abc1234");
    }

    /// The load-bearing test: serialized field names/shapes must match the
    /// plan.md example exactly (structural equality via `serde_json::Value`,
    /// so key order and whitespace don't matter — only names and nesting).
    #[test]
    fn serialized_field_names_match_plan_md_example() {
        let row: SampleRow = serde_json::from_str(EXAMPLE_ROW_JSON).unwrap();

        let round_tripped: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&row).unwrap()).unwrap();
        let expected: serde_json::Value = serde_json::from_str(EXAMPLE_ROW_JSON).unwrap();

        assert_eq!(round_tripped, expected);
    }

    #[test]
    fn optional_fields_are_omitted_not_nulled_when_absent() {
        let row = SampleRow {
            schema_version: SCHEMA_VERSION,
            id: "smp-20260819-113211-a1b2".to_string(),
            recorded_at: "2026-08-19T11:32:11-07:00".to_string(),
            kind: KIND_SAMPLE.to_string(),
            drill: DrillMeta {
                r#type: "free".to_string(),
                key: None,
                mode: None,
                degree: None,
                target_hz: None,
            },
            self_rating: None,
            note: None,
            audio: AudioMeta {
                path: "samples/113211-a1b2.wav".to_string(),
                sample_rate: 48000,
                channels: 1,
                bit_depth: 16,
                duration_ms: 3210,
            },
            capture: CaptureMeta {
                device_label: "MacBook Pro Microphone".to_string(),
                user_agent: "…".to_string(),
                app_git_sha: "abc1234".to_string(),
            },
        };

        let json = serde_json::to_string(&row).unwrap();
        assert!(!json.contains("self_rating"));
        assert!(!json.contains("\"note\""));
        assert!(!json.contains("\"key\""));
        assert!(!json.contains("\"mode\""));
        assert!(!json.contains("\"degree\""));
        assert!(!json.contains("\"target_hz\""));
    }
}
