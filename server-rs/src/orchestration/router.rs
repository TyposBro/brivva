//! Route definitions — the only place routes are registered.

use axum::{Router, routing::{get, post}};
use tower_http::cors::{Any, CorsLayer};

use crate::features::broadcast::domain::Sessions;
use crate::core::config::MAX_BODY_SIZE;

pub fn build_router(sessions: Sessions) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/", get(|| async { "Brivva Desktop" }))
        .route("/ws", get(crate::ws::ws_handler))
        .route("/api/voice/clone", post(crate::api::voice::voice_clone_handler))
        .route("/api/voice", get(crate::api::voice::voice_status_handler).delete(crate::api::voice::voice_delete_handler))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_SIZE))
        .layer(cors)
        .with_state(sessions)
}
