pub mod core;
pub mod streaming;
pub mod stt;
pub mod translation;
pub mod tts;
pub mod voice_clone;
mod pipeline;
mod ws;
mod api;

use axum::{Router, routing::{get, post}};
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use tower_http::cors::{Any, CorsLayer};

use core::config::{MAX_BODY_SIZE, SERVER_ADDR};
use core::types::Sessions;

pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .build()
        .expect("failed to build HTTP client")
});

pub async fn run_server() {
    load_env();
    streaming::kill_orphan_ffmpeg();

    let sessions: Sessions = Arc::new(DashMap::new());
    let app = build_router(sessions);

    let listener = tokio::net::TcpListener::bind(SERVER_ADDR).await.unwrap();
    tracing::info!("Brivva server on http://{}", SERVER_ADDR);
    axum::serve(listener, app).await.unwrap();
}

fn load_env() {
    if dotenvy::from_filename(".env.local").is_err()
        && let Err(e) = dotenvy::dotenv() {
            tracing::warn!(".env not loaded ({e}). Using existing environment variables.");
        }
}

fn build_router(sessions: Sessions) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(|| async { "Brivva Desktop" }))
        .route("/ws", get(ws::ws_handler))
        .route("/api/voice/clone", post(api::voice::voice_clone_handler))
        .route("/api/voice", get(api::voice::voice_status_handler).delete(api::voice::voice_delete_handler))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_SIZE))
        .layer(cors)
        .with_state(sessions)
}
