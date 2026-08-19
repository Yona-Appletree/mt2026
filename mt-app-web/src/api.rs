//! The one call this app makes: `POST /api/samples`. Serializes the meta
//! part exactly as `mt-server`'s `NewSampleRequest` expects it — the server
//! owns `schema_version`, `id`, `recorded_at`, and every derived
//! `audio.*` field, so none of them appear here.

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use web_sys::{Blob, FormData};

/// Relative on purpose: the same URL works behind `dx serve`'s
/// `[[web.proxy]]` and against `mt-server` serving the built bundle
/// same-origin.
const SAMPLES_URL: &str = "/api/samples";

/// Stamped into every row by `build.rs`; `"unknown"` when git isn't around.
pub const APP_GIT_SHA: &str = env!("MT_APP_GIT_SHA");

/// What the server returns on a successful save.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct SavedSample {
    pub id: String,
    pub path: String,
}

/// The tag form's contents plus the two capture facts, ready to send.
pub struct SaveRequest {
    pub drill_type: String,
    pub self_rating: Option<u8>,
    pub note: Option<String>,
    pub sample_rate: u32,
    pub device_label: String,
}

/// `drill` for a v0.1 take. `key`/`mode`/`degree`/`target_hz` are absent,
/// not null — the degree drill that fills them arrives in P6.
#[derive(Serialize)]
struct DrillPart {
    #[serde(rename = "type")]
    r#type: String,
}

/// The only audio fact the client knows; the archive derives the rest from
/// the WAV it writes.
#[derive(Serialize)]
struct AudioPart {
    sample_rate: u32,
}

#[derive(Serialize)]
struct CapturePart {
    device_label: String,
    user_agent: String,
    app_git_sha: String,
}

#[derive(Serialize)]
struct MetaPart {
    drill: DrillPart,
    #[serde(skip_serializing_if = "Option::is_none")]
    self_rating: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    audio: AudioPart,
    capture: CapturePart,
}

/// Posts one take. `pcm` goes over the wire as raw little-endian f32 mono
/// at `req.sample_rate`, which is what the server decodes it as.
pub async fn save_sample(req: SaveRequest, pcm: &[f32]) -> Result<SavedSample, String> {
    let meta = MetaPart {
        drill: DrillPart {
            r#type: req.drill_type,
        },
        self_rating: req.self_rating,
        note: req.note,
        audio: AudioPart {
            sample_rate: req.sample_rate,
        },
        capture: CapturePart {
            device_label: req.device_label,
            user_agent: user_agent(),
            app_git_sha: APP_GIT_SHA.to_string(),
        },
    };
    let meta_json =
        serde_json::to_string(&meta).map_err(|err| format!("failed to encode meta: {err}"))?;
    post_sample(&meta_json, pcm).await
}

/// Posts a drill take. `mt_session::NewSampleMeta` serializes to exactly the
/// wire shape `mt-server` expects (asserted in mt-session's tests), so it
/// goes over as-is — no re-mapping layer to drift.
pub async fn save_meta(
    meta: &mt_session::NewSampleMeta,
    pcm: &[f32],
) -> Result<SavedSample, String> {
    let meta_json =
        serde_json::to_string(meta).map_err(|err| format!("failed to encode meta: {err}"))?;
    post_sample(&meta_json, pcm).await
}

/// The one wire call, shared by the free-take and drill paths.
async fn post_sample(meta_json: &str, pcm: &[f32]) -> Result<SavedSample, String> {
    let form = FormData::new().map_err(|err| js_msg("build the form body", &err))?;
    form.append_with_str("meta", meta_json)
        .map_err(|err| js_msg("append the meta part", &err))?;
    form.append_with_blob_and_filename("pcm", &pcm_blob(pcm)?, "pcm.f32le")
        .map_err(|err| js_msg("append the pcm part", &err))?;

    // No explicit Content-Type: the browser must set it so the multipart
    // boundary matches the FormData body it generates.
    let response = gloo_net::http::Request::post(SAMPLES_URL)
        .body(form)
        .map_err(|err| format!("failed to build the request: {err}"))?
        .send()
        .await
        .map_err(|err| format!("failed to reach {SAMPLES_URL}: {err}"))?;

    if !response.ok() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("server returned {status}: {body}"));
    }

    response
        .json::<SavedSample>()
        .await
        .map_err(|err| format!("failed to read the server's response: {err}"))
}

/// f32 samples → a Blob of their little-endian bytes.
fn pcm_blob(pcm: &[f32]) -> Result<Blob, String> {
    let mut bytes = Vec::with_capacity(pcm.len() * 4);
    for sample in pcm {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    let parts = js_sys::Array::new();
    parts.push(&js_sys::Uint8Array::from(bytes.as_slice()));
    Blob::new_with_u8_array_sequence(&parts).map_err(|err| js_msg("build the pcm blob", &err))
}

pub fn user_agent() -> String {
    web_sys::window()
        .map(|window| window.navigator().user_agent().unwrap_or_default())
        .unwrap_or_default()
}

fn js_msg(context: &str, err: &JsValue) -> String {
    let detail = err
        .as_string()
        .or_else(|| {
            err.dyn_ref::<js_sys::Error>()
                .map(|error| String::from(error.message()))
        })
        .unwrap_or_else(|| format!("{err:?}"));
    format!("failed to {context}: {detail}")
}
