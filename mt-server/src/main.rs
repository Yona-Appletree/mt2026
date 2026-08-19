//! `mt-server` binary: read config, mint the `Archive`, build the router,
//! bind, serve. No behavior lives here — see `lib.rs` for the router and
//! `config.rs` for env parsing, so both are testable without a socket.

use std::net::SocketAddr;
use std::sync::Arc;

use mt_archive::Archive;
use mt_server::config::Config;
use mt_server::router;

#[tokio::main]
async fn main() {
    let config = Config::from_env();
    println!(
        "mt-server: data_root={} static_dir={} port={}",
        config.data_root.display(),
        config.static_dir.display(),
        config.port
    );

    let archive = Arc::new(Archive::new(config.data_root.clone()));
    let app = router(archive, config.static_dir.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|err| panic!("failed to bind {addr}: {err}"));
    println!("mt-server listening on http://{addr}");
    axum::serve(listener, app).await.expect("server error");
}
