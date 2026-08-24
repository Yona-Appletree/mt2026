//! The handler-facing error type. Two buckets only, per the phase spec:
//! malformed client input (400) and archive/IO failure (500).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    /// Bad request body: missing/unparseable multipart part, malformed
    /// `meta` JSON, or `pcm` bytes that aren't a whole number of f32s.
    BadRequest(String),
    /// The archive write itself failed (filesystem/WAV I/O) or the
    /// `spawn_blocking` task backing it panicked.
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            AppError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
