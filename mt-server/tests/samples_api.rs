//! Integration tests for the router: drive it with
//! `tower::ServiceExt::oneshot` (no socket) against a tempdir archive and a
//! tempdir static dir (empty — static serving isn't exercised here).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use mt_archive::Archive;
use tower::ServiceExt;

/// 0.1s of 440Hz sine at 8kHz, as little-endian f32 bytes — tiny on purpose
/// (per the phase spec) so the test runs fast.
fn synth_pcm_bytes(sample_rate: u32, seconds: f32, freq_hz: f32) -> Vec<u8> {
    let n = (sample_rate as f32 * seconds) as usize;
    let mut bytes = Vec::with_capacity(n * 4);
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let sample = (2.0 * std::f32::consts::PI * freq_hz * t).sin() * 0.5;
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn meta_json(sample_rate: u32) -> String {
    format!(
        r#"{{
            "drill": {{"type": "degree", "key": "D", "mode": "major", "degree": 5, "target_hz": 440.0}},
            "self_rating": 4,
            "note": "test take",
            "audio": {{"sample_rate": {sample_rate}}},
            "capture": {{"device_label": "Test Mic", "user_agent": "test-agent", "app_git_sha": "abc1234"}}
        }}"#
    )
}

/// Hand-builds a `multipart/form-data` body with a `meta` text part and a
/// `pcm` binary part — avoids pulling in a client-side multipart crate just
/// for tests.
fn build_multipart_body(boundary: &str, meta: &str, pcm: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"meta\"\r\n");
    body.extend_from_slice(b"Content-Type: application/json\r\n\r\n");
    body.extend_from_slice(meta.as_bytes());
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"pcm\"\r\n");
    body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body.extend_from_slice(pcm);
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

fn multipart_request(boundary: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/api/samples")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body))
        .unwrap()
}

/// The one subdirectory of `root` (the archive mints a `YYYY-MM-DD` day dir
/// on first write; tests never straddle midnight in practice, so there's
/// exactly one).
fn only_day_dir(root: &std::path::Path) -> std::path::PathBuf {
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(entries.len(), 1, "expected exactly one day dir in {root:?}");
    entries.remove(0)
}

#[tokio::test]
async fn health_reports_ok_and_version() {
    let data_root = tempfile::tempdir().unwrap();
    let static_dir = tempfile::tempdir().unwrap();
    let archive = Arc::new(Archive::new(data_root.path().to_path_buf()));
    let app = mt_server::router(archive, static_dir.path().to_path_buf());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn post_samples_writes_wav_and_log_row_and_returns_matching_id_and_path() {
    let data_root = tempfile::tempdir().unwrap();
    let static_dir = tempfile::tempdir().unwrap();
    let archive = Arc::new(Archive::new(data_root.path().to_path_buf()));
    let app = mt_server::router(archive, static_dir.path().to_path_buf());

    let sample_rate = 8_000;
    let pcm = synth_pcm_bytes(sample_rate, 0.1, 440.0);
    let boundary = "test-boundary-1234";
    let body = build_multipart_body(boundary, &meta_json(sample_rate), &pcm);

    let response = app
        .oneshot(multipart_request(boundary, body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response_body = response.into_body().collect().await.unwrap().to_bytes();
    let response_json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
    let response_id = response_json["id"].as_str().unwrap().to_string();
    let response_path = response_json["path"].as_str().unwrap().to_string();

    // Read the day dir the archive actually wrote.
    let day_dir = only_day_dir(data_root.path());

    // The log row: exactly one line, and it matches the response and the
    // meta we sent.
    let log_contents = std::fs::read_to_string(day_dir.join("log.jsonl")).unwrap();
    let lines: Vec<&str> = log_contents.lines().collect();
    assert_eq!(lines.len(), 1);
    let row: mt_archive::SampleRow = serde_json::from_str(lines[0]).unwrap();

    assert_eq!(row.id, response_id);
    assert_eq!(row.audio.path, response_path);
    assert_eq!(row.drill.r#type, "degree");
    assert_eq!(row.drill.key.as_deref(), Some("D"));
    assert_eq!(row.drill.mode.as_deref(), Some("major"));
    assert_eq!(row.drill.degree, Some(5));
    assert_eq!(row.self_rating, Some(4));
    assert_eq!(row.note.as_deref(), Some("test take"));
    assert_eq!(row.audio.sample_rate, sample_rate);
    assert_eq!(row.audio.channels, 1);
    assert_eq!(row.audio.bit_depth, 16);
    assert_eq!(row.capture.device_label, "Test Mic");
    assert_eq!(row.capture.app_git_sha, "abc1234");
    // Server-owned fields the client never sent.
    assert_eq!(row.schema_version, mt_archive::SCHEMA_VERSION);
    assert!(!row.recorded_at.is_empty());

    // The WAV: parses via hound at the stated rate, with the right sample
    // count (mono, 16-bit).
    let wav_path = day_dir.join(&row.audio.path);
    assert!(wav_path.is_file());
    let mut reader = hound::WavReader::open(&wav_path).unwrap();
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, sample_rate);
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.bits_per_sample, 16);
    let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(samples.len(), pcm.len() / 4);
}

#[tokio::test]
async fn post_samples_with_malformed_meta_json_returns_400() {
    let data_root = tempfile::tempdir().unwrap();
    let static_dir = tempfile::tempdir().unwrap();
    let archive = Arc::new(Archive::new(data_root.path().to_path_buf()));
    let app = mt_server::router(archive, static_dir.path().to_path_buf());

    let pcm = synth_pcm_bytes(8_000, 0.1, 440.0);
    let boundary = "test-boundary-5678";
    let body = build_multipart_body(boundary, "{ this is not valid json", &pcm);

    let response = app
        .oneshot(multipart_request(boundary, body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Nothing should have been written.
    assert!(
        std::fs::read_dir(data_root.path())
            .unwrap()
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn post_samples_with_missing_meta_field_returns_400() {
    let data_root = tempfile::tempdir().unwrap();
    let static_dir = tempfile::tempdir().unwrap();
    let archive = Arc::new(Archive::new(data_root.path().to_path_buf()));
    let app = mt_server::router(archive, static_dir.path().to_path_buf());

    // Valid JSON, but missing required fields (`drill`, `audio`, `capture`).
    let pcm = synth_pcm_bytes(8_000, 0.1, 440.0);
    let boundary = "test-boundary-9012";
    let body = build_multipart_body(boundary, "{}", &pcm);

    let response = app
        .oneshot(multipart_request(boundary, body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_samples_with_non_multiple_of_four_pcm_returns_400() {
    let data_root = tempfile::tempdir().unwrap();
    let static_dir = tempfile::tempdir().unwrap();
    let archive = Arc::new(Archive::new(data_root.path().to_path_buf()));
    let app = mt_server::router(archive, static_dir.path().to_path_buf());

    let mut pcm = synth_pcm_bytes(8_000, 0.1, 440.0);
    pcm.pop(); // break the 4-byte alignment
    let boundary = "test-boundary-3456";
    let body = build_multipart_body(boundary, &meta_json(8_000), &pcm);

    let response = app
        .oneshot(multipart_request(boundary, body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
