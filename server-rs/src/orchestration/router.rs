//! Route definitions — the only place routes are registered.

use std::sync::Arc;
use axum::{Router, routing::{get, post}};
use tower_http::cors::{Any, CorsLayer};

use crate::features::broadcast::domain::Sessions;
use crate::features::broadcast::data::ws_handler::BroadcastDeps;
use crate::features::broadcast::data::voice_api::VoiceApiDeps;
use crate::core::config::MAX_BODY_SIZE;
use super::di::AppContext;

pub fn build_router(sessions: Sessions, app_ctx: Arc<AppContext>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let broadcast_deps = Arc::new(BroadcastDeps {
        stt_api_key: app_ctx.config.stt_api_key.clone(),
        translate_api_key: app_ctx.config.translate_api_key.clone(),
        tts_api_key: app_ctx.config.tts_api_key.clone(),
        default_voice: app_ctx.config.default_voice.clone(),
        http_client: app_ctx.http_client.clone(),
    });

    let voice_deps = Arc::new(VoiceApiDeps {
        http_client: app_ctx.http_client.clone(),
        tts_api_key: app_ctx.config.tts_api_key.clone(),
    });

    Router::new()
        .route("/", get(|| async { "Brivva Desktop" }))
        .route("/ws", get(crate::features::broadcast::data::ws_handler::ws_handler))
        .route("/api/voice/clone", post(crate::features::broadcast::data::voice_api::voice_clone_handler))
        .route("/api/voice", get(crate::features::broadcast::data::voice_api::voice_status_handler).delete(crate::features::broadcast::data::voice_api::voice_delete_handler))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_SIZE))
        .layer(cors)
        .layer(axum::Extension(broadcast_deps))
        .layer(axum::Extension(voice_deps))
        .with_state(sessions)
}
