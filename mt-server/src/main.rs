//! `mt-server`: the axum backend. The only crate in this workspace allowed
//! to depend on axum/tokio — a humble adapter over `mt-archive` that mints
//! sample ids/paths and serves the built `mt-app-web` bundle. v0.1 stub:
//! just the health check.

use axum::{Json, Router, routing::get};
use serde_json::{Value, json};
use std::net::SocketAddr;

const DEFAULT_PORT: u16 = 4826;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("MT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_PORT);

    let app = Router::new().route("/api/health", get(health));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));
    println!("mt-server listening on http://{addr}");
    axum::serve(listener, app).await.expect("server error");
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}
