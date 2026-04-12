//! REST handlers for voice clone API.

use std::sync::Arc;
use axum::{Json, body::Bytes, http::StatusCode};
use serde::{Deserialize, Serialize};

use crate::core::config::BYTES_PER_SEC;
use crate::shared::voice_clone;

#[derive(Deserialize)]
pub struct VoiceCloneQuery {
    pub provider: Option<String>,
}

/// Dependencies injected from orchestration for voice API handlers.
pub struct VoiceApiDeps {
    pub http_client: reqwest::Client,
    pub tts_api_key: String,
    pub dashscope_api_key: String,
}

#[derive(Serialize)]
pub struct VoiceStatus {
    active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    voice_id: Option<String>,
}

pub async fn voice_status_handler(
    axum::extract::Query(params): axum::extract::Query<VoiceCloneQuery>,
) -> Json<VoiceStatus> {
    let provider = params.provider.as_deref().unwrap_or("elevenlabs");
    let voice_id = voice_clone::persistence::load_persisted_voice_for(provider);
    Json(VoiceStatus { active: voice_id.is_some(), voice_id })
}

pub async fn voice_clone_handler(
    axum::Extension(deps): axum::Extension<Arc<VoiceApiDeps>>,
    axum::extract::Query(params): axum::extract::Query<VoiceCloneQuery>,
    body: Bytes,
) -> Result<Json<VoiceStatus>, (StatusCode, String)> {
    if body.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Empty audio body".to_string()));
    }
    let provider = params.provider.as_deref().unwrap_or("elevenlabs");
    tracing::info!(
        "[API] voice clone request ({}): {}B PCM ({:.1}s audio)",
        provider, body.len(), body.len() as f64 / BYTES_PER_SEC,
    );
    let voice_id = match provider {
        "dashscope" => voice_clone::clone_voice_dashscope(
            &deps.http_client, &deps.dashscope_api_key, body.to_vec(),
        ).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?,
        _ => voice_clone::clone_voice_standalone(
            &deps.http_client, &deps.tts_api_key, body.to_vec(),
        ).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?,
    };
    Ok(Json(VoiceStatus { active: true, voice_id: Some(voice_id) }))
}

pub async fn voice_delete_handler(
    axum::Extension(deps): axum::Extension<Arc<VoiceApiDeps>>,
    axum::extract::Query(params): axum::extract::Query<VoiceCloneQuery>,
) -> StatusCode {
    let provider = params.provider.as_deref().unwrap_or("elevenlabs");
    if let Some(voice_id) = voice_clone::persistence::load_persisted_voice_for(provider) {
        match provider {
            "dashscope" => voice_clone::delete_cloned_voice_dashscope(
                &deps.http_client, &deps.dashscope_api_key, &voice_id,
            ).await,
            _ => voice_clone::delete_cloned_voice(
                &deps.http_client, &deps.tts_api_key, &voice_id,
            ).await,
        }
        let file = voice_clone::persistence::file_for_provider_pub(provider);
        let _ = std::fs::remove_file(file);
        tracing::info!("[API] voice clone deleted ({}): {}", provider, voice_id);
    }
    StatusCode::NO_CONTENT
}
