pub mod constants;
pub mod error;
pub mod ffmpeg;
pub mod stt;
pub mod translation;
pub mod tts;
pub mod voice_clone;
mod pipeline;
mod types;
mod ws;

use axum::{
    Router, Json,
    body::Bytes,
    http::StatusCode,
    routing::{get, post},
};
use dashmap::DashMap;
use serde::Serialize;
use std::sync::Arc;
use std::sync::LazyLock;
use tower_http::cors::{Any, CorsLayer};

use constants::{
    BYTES_PER_SEC, MAX_BODY_SIZE, SERVER_ADDR, VOICE_CLONE_FILE,
};
use types::Sessions;

pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .build()
        .expect("failed to build HTTP client")
});

/// Start the Axum server on localhost:3000.
pub async fn run_server() {
    // Prefer .env.local (localhost config), fall back to .env
    if dotenvy::from_filename(".env.local").is_err() {
        if let Err(e) = dotenvy::dotenv() {
            tracing::warn!(".env not loaded ({e}). Using existing environment variables.");
        }
    }

    // Clean up orphaned FFmpeg processes from previous crashes
    ffmpeg::kill_orphan_ffmpeg();

    let sessions: Sessions = Arc::new(DashMap::new());

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { "Brivva Desktop" }))
        .route("/ws", get(ws::ws_handler))
        .route("/api/voice/clone", post(voice_clone_handler))
        .route("/api/voice", get(voice_status_handler).delete(voice_delete_handler))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY_SIZE))
        .layer(cors)
        .with_state(sessions);

    let listener = tokio::net::TcpListener::bind(SERVER_ADDR)
        .await
        .unwrap();

    tracing::info!("Brivva server on http://{}", SERVER_ADDR);
    axum::serve(listener, app).await.unwrap();
}

// -- Voice Clone REST API -----------------------------------------

#[derive(Serialize)]
struct VoiceStatus {
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    voice_id: Option<String>,
}

/// GET /api/voice -- check if a persisted voice clone exists.
async fn voice_status_handler() -> Json<VoiceStatus> {
    let voice_id = voice_clone::load_persisted_voice();
    Json(VoiceStatus { active: voice_id.is_some(), voice_id })
}

/// POST /api/voice/clone -- accepts raw PCM s16le 44100Hz mono, clones via ElevenLabs.
async fn voice_clone_handler(body: Bytes) -> Result<Json<VoiceStatus>, (StatusCode, String)> {
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Empty audio body".to_string()));
    }
    tracing::info!(
        "[API] voice clone request: {}B PCM ({:.1}s audio)",
        body.len(),
        body.len() as f64 / BYTES_PER_SEC,
    );

    let voice_id = voice_clone::clone_voice_standalone(body.to_vec()).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

    Ok(Json(VoiceStatus { active: true, voice_id: Some(voice_id) }))
}

/// DELETE /api/voice -- delete the persisted voice clone.
async fn voice_delete_handler() -> StatusCode {
    if let Some(voice_id) = voice_clone::load_persisted_voice() {
        voice_clone::delete_cloned_voice(&voice_id).await;
        let _ = std::fs::remove_file(VOICE_CLONE_FILE);
        tracing::info!("[API] voice clone deleted: {}", voice_id);
    }
    StatusCode::NO_CONTENT
}
