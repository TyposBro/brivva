//! REST handlers for voice clone API.

use std::sync::Arc;
use axum::{Json, body::Bytes, http::StatusCode};
use serde::Serialize;

use crate::core::config::{BYTES_PER_SEC, VOICE_CLONE_FILE};
use crate::orchestration::di::AppContext;
use crate::shared::voice_clone;

#[derive(Serialize)]
pub struct VoiceStatus {
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    voice_id: Option<String>,
}

pub async fn voice_status_handler() -> Json<VoiceStatus> {
    let voice_id = voice_clone::load_persisted_voice();
    Json(VoiceStatus { active: voice_id.is_some(), voice_id })
}

pub async fn voice_clone_handler(
    axum::Extension(app_ctx): axum::Extension<Arc<AppContext>>,
    body: Bytes,
) -> Result<Json<VoiceStatus>, (StatusCode, String)> {
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Empty audio body".to_string()));
    }
    tracing::info!(
        "[API] voice clone request: {}B PCM ({:.1}s audio)",
        body.len(), body.len() as f64 / BYTES_PER_SEC,
    );
    let voice_id = voice_clone::clone_voice_standalone(
        &app_ctx.http_client, &app_ctx.config.tts_api_key, body.to_vec(),
    ).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(VoiceStatus { active: true, voice_id: Some(voice_id) }))
}

pub async fn voice_delete_handler(
    axum::Extension(app_ctx): axum::Extension<Arc<AppContext>>,
) -> StatusCode {
    if let Some(voice_id) = voice_clone::load_persisted_voice() {
        voice_clone::delete_cloned_voice(&app_ctx.http_client, &app_ctx.config.tts_api_key, &voice_id).await;
        let _ = std::fs::remove_file(VOICE_CLONE_FILE);
        tracing::info!("[API] voice clone deleted: {}", voice_id);
    }
    StatusCode::NO_CONTENT
}
