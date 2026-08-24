//! Route handlers. No domain logic lives here — decode the wire format,
//! hand off to `mt_archive::Archive`, translate its result back to JSON.

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Multipart, State};
use mt_archive::{Archive, CaptureMeta, DrillMeta, NewSample};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::AppError;

/// The client-supplied part of a new sample: the row shape from plan.md's
/// data contract minus every server-owned field. Per mt-archive's
/// `NewSample` doc comment, that's `schema_version`, `id`, `recorded_at`
/// (the archive stamps the time itself), and `audio.path` /
/// `audio.bit_depth` / `audio.channels` / `audio.duration_ms` (all facts
/// about the WAV the archive is about to write, not something a caller
/// restates). What's left — `drill`, `self_rating`, `note`,
/// `audio.sample_rate`, `capture` — maps 1:1 onto `NewSample`'s fields.
#[derive(Debug, Deserialize)]
struct NewSampleRequest {
    drill: DrillMeta,
    #[serde(default)]
    self_rating: Option<u8>,
    #[serde(default)]
    note: Option<String>,
    audio: NewSampleAudioRequest,
    capture: CaptureMeta,
}

/// Only the one client-known audio fact: the device's actual capture rate.
/// Everything else in `AudioMeta` is derived from the WAV the archive
/// writes.
#[derive(Debug, Deserialize)]
struct NewSampleAudioRequest {
    sample_rate: u32,
}

impl From<NewSampleRequest> for NewSample {
    fn from(req: NewSampleRequest) -> Self {
        NewSample {
            drill: req.drill,
            self_rating: req.self_rating,
            note: req.note,
            sample_rate: req.audio.sample_rate,
            capture: req.capture,
        }
    }
}

pub async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

/// `POST /api/samples`: a two-part multipart form (`meta` JSON, `pcm` raw
/// little-endian f32 mono samples at `meta.audio.sample_rate`). Decodes
/// both parts, then does the actual archive write inside
/// `spawn_blocking` (`mt-archive` is sync by design).
pub async fn create_sample(
    State(archive): State<Arc<Archive>>,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let mut meta: Option<NewSampleRequest> = None;
    let mut pcm_bytes: Option<Bytes> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| AppError::BadRequest(format!("malformed multipart body: {err}")))?
    {
        match field.name() {
            Some("meta") => {
                let text = field
                    .text()
                    .await
                    .map_err(|err| AppError::BadRequest(format!("invalid 'meta' part: {err}")))?;
                let parsed: NewSampleRequest = serde_json::from_str(&text)
                    .map_err(|err| AppError::BadRequest(format!("malformed meta JSON: {err}")))?;
                meta = Some(parsed);
            }
            Some("pcm") => {
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|err| AppError::BadRequest(format!("invalid 'pcm' part: {err}")))?;
                pcm_bytes = Some(bytes);
            }
            _ => {
                // Unknown part: ignore rather than reject, in case the
                // client sends extra form fields later.
            }
        }
    }

    let meta = meta.ok_or_else(|| AppError::BadRequest("missing 'meta' part".to_string()))?;
    let pcm_bytes =
        pcm_bytes.ok_or_else(|| AppError::BadRequest("missing 'pcm' part".to_string()))?;

    if pcm_bytes.len() % 4 != 0 {
        return Err(AppError::BadRequest(format!(
            "'pcm' byte length {} is not a multiple of 4 (expected little-endian f32 samples)",
            pcm_bytes.len()
        )));
    }
    let pcm_f32: Vec<f32> = pcm_bytes
        .chunks_exact(4)
        .map(|chunk| {
            f32::from_le_bytes(
                chunk
                    .try_into()
                    .expect("chunks_exact(4) always yields 4 bytes"),
            )
        })
        .collect();

    let new_sample: NewSample = meta.into();

    let saved = tokio::task::spawn_blocking(move || archive.append_sample(new_sample, &pcm_f32))
        .await
        .map_err(|err| AppError::Internal(format!("archive task panicked: {err}")))?
        .map_err(|err| AppError::Internal(format!("archive write failed: {err}")))?;

    Ok(Json(json!({ "id": saved.id, "path": saved.path })))
}
