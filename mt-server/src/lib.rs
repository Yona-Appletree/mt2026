//! `mt-server`: the axum backend. The only crate in this workspace allowed
//! to depend on axum/tokio (see AGENTS.md) — a humble adapter over
//! `mt-archive` that mints sample ids/paths via
//! [`mt_archive::Archive::append_sample`] and serves the built
//! `mt-app-web` bundle. `main.rs` is config-plus-bind only; [`router`] is
//! exposed here so tests can drive the app with `tower::ServiceExt::oneshot`
//! without binding a socket.

pub mod config;
mod error;
mod routes;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use mt_archive::Archive;
use tower_http::services::{ServeDir, ServeFile};

/// Body limit generous enough for several minutes of 48kHz f32 mono audio
/// (~11 MB/min), with headroom.
const MAX_BODY_BYTES: usize = 256 * 1024 * 1024;

/// Builds the full app: `/api/health`, `/api/samples`, then static serving
/// of `static_dir` (with `index.html` fallback for unmatched paths) mounted
/// after the API routes so it never shadows them.
///
/// Uses `ServeDir::fallback` (not `not_found_service`) so unmatched paths
/// serve `index.html` as a normal `200` — required for the SPA's
/// client-side routing to take over. `not_found_service` would force the
/// response status to `404` regardless of the fallback's own status, which
/// is meant for "custom 404 page" use, not an SPA index fallback.
pub fn router(archive: Arc<Archive>, static_dir: impl Into<PathBuf>) -> Router {
    let static_dir = static_dir.into();
    let index_html = static_dir.join("index.html");
    let static_service = ServeDir::new(&static_dir).fallback(ServeFile::new(index_html));

    Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/samples", post(routes::create_sample))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(archive)
        .fallback_service(static_service)
}
