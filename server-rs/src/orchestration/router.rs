//! Route definitions — the only place routes are registered.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::{Router, routing::{get, post}};
use tower_http::cors::{Any, CorsLayer};

use crate::features::broadcast::domain::Sessions;
use crate::features::broadcast::data::ws_handler::BroadcastDeps;
use crate::features::broadcast::data::voice_api::VoiceApiDeps;
use crate::features::dubbing::data::handlers::DubbingDeps;
use crate::features::dubbing::domain::DubbingJobs;
use crate::core::config::MAX_BODY_SIZE;
use super::di::AppContext;

pub fn build_router(sessions: Sessions, app_ctx: Arc<AppContext>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let broadcast_deps = Arc::new(BroadcastDeps {
        stt_api_key: app_ctx.config.soniox_api_key.clone(),
        tts_api_key: app_ctx.config.tts_api_key.clone(),
        dashscope_api_key: app_ctx.config.dashscope_api_key.clone(),
        default_voice: app_ctx.config.default_voice.clone(),
        http_client: app_ctx.http_client.clone(),
    });

    let voice_deps = Arc::new(VoiceApiDeps {
        http_client: app_ctx.http_client.clone(),
        tts_api_key: app_ctx.config.tts_api_key.clone(),
        dashscope_api_key: app_ctx.config.dashscope_api_key.clone(),
    });

    let dubbing_deps = Arc::new(DubbingDeps {
        tts_api_key: app_ctx.config.tts_api_key.clone(),
        http_client: app_ctx.http_client.clone(),
    });

    let dubbing_jobs: DubbingJobs = Arc::new(Mutex::new(HashMap::new()));

    use crate::features::dubbing::data::handlers as dub;

    Router::new()
        .route("/", get(|| async { "Brivva Desktop" }))
        .route("/ws", get(crate::features::broadcast::data::ws_handler::ws_handler))
        .route("/api/voice/clone", post(crate::features::broadcast::data::voice_api::voice_clone_handler))
        .route("/api/voice", get(crate::features::broadcast::data::voice_api::voice_status_handler).delete(crate::features::broadcast::data::voice_api::voice_delete_handler))
        .route("/api/dubbing/start", post(dub::start_handler))
        .route("/api/dubbing/status/{job_id}", get(dub::status_handler))
        .route("/api/dubbing/jobs/{session_id}", get(dub::session_jobs_handler))
        .route("/api/dubbing/download/{job_id}", get(dub::download_handler))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_SIZE))
        .layer(cors)
        .layer(axum::Extension(broadcast_deps))
        .layer(axum::Extension(voice_deps))
        .layer(axum::Extension(dubbing_deps))
        .layer(axum::Extension(dubbing_jobs))
        .with_state(sessions)
}
