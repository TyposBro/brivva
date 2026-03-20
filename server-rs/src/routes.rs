//! REST API routes for YouTube OAuth, sessions, voices, and user info.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Redirect},
};
use serde::Deserialize;

use crate::db;
use crate::youtube;
use crate::AppState;

// ── Query params ────────────────────────────────────────

#[derive(Deserialize)]
pub struct UserIdQuery {
    pub user_id: String,
}

#[derive(Deserialize)]
pub struct OAuthCallback {
    pub code: String,
    pub state: String, // user_id
}

#[derive(Deserialize)]
pub struct CreateSessionBody {
    pub user_id: String,
    pub title: String,
    pub source_lang: String,
    pub target_langs: Vec<String>, // ["en","ja","zh"]
    pub voice_id: Option<String>,
    #[serde(default)]
    pub platforms: Vec<PlatformConfig>, // which platforms to stream to
}

#[derive(Deserialize, Clone)]
pub struct PlatformConfig {
    pub platform: String,    // "youtube", "instagram", "coupang", "custom"
    pub rtmp_url: Option<String>,    // required for non-YouTube
    pub stream_key: Option<String>,  // required for non-YouTube
}

#[derive(Deserialize)]
pub struct AddStreamBody {
    pub lang: String,
    pub platform: String,
    pub rtmp_url: String,
    pub stream_key: String,
}

#[derive(Deserialize)]
pub struct PlatformCredentialBody {
    pub user_id: String,
    pub platform: String,
    pub rtmp_url: Option<String>,
    pub stream_key: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct DeleteCredentialQuery {
    pub user_id: String,
    pub platform: String,
}

#[derive(Deserialize)]
pub struct CreateVoiceBody {
    pub user_id: String,
    pub name: String,
    pub audio_base64: String, // base64 PCM 44.1kHz mono
}

// ── YouTube OAuth ───────────────────────────────────────

/// GET /auth/youtube?user_id=... → redirect to Google OAuth consent
pub async fn youtube_auth(Query(q): Query<UserIdQuery>) -> impl IntoResponse {
    let url = youtube::oauth_url(&q.user_id);
    Redirect::temporary(&url)
}

/// GET /auth/youtube/callback?code=...&state=user_id → exchange code, store tokens
pub async fn youtube_callback(
    State(state): State<AppState>,
    Query(q): Query<OAuthCallback>,
) -> impl IntoResponse {
    let user_id = &q.state;

    // Ensure user exists
    db::get_or_create_user(&state.db, user_id).await;

    // Exchange code for tokens
    let tokens = match youtube::exchange_code(&q.code).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[YOUTUBE] OAuth exchange failed: {}", e);
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e}))).into_response();
        }
    };

    let refresh_token = tokens.refresh_token.unwrap_or_default();
    let expires_at = chrono::Utc::now().timestamp() + tokens.expires_in;

    // Get channel info
    let (channel_id, channel_name) = match youtube::get_channel_info(&tokens.access_token).await {
        Ok(info) => info,
        Err(e) => {
            eprintln!("[YOUTUBE] Channel info failed: {}", e);
            ("".to_string(), "".to_string())
        }
    };

    // Store tokens in DB
    db::update_youtube_tokens(
        &state.db,
        user_id,
        &tokens.access_token,
        &refresh_token,
        expires_at,
        &channel_id,
        &channel_name,
    )
    .await;

    eprintln!(
        "[YOUTUBE] OAuth complete for user {} (channel: {})",
        user_id, channel_name
    );

    // Redirect back to frontend dashboard
    Redirect::temporary("https://brivva.pages.dev/dashboard?youtube=connected").into_response()
}

// ── User ────────────────────────────────────────────────

/// GET /api/user?user_id=... → user info including YouTube connection status
pub async fn get_user(
    State(state): State<AppState>,
    Query(q): Query<UserIdQuery>,
) -> impl IntoResponse {
    let user = db::get_or_create_user(&state.db, &q.user_id).await;
    Json(serde_json::json!({
        "id": user.id,
        "youtube_connected": user.youtube_channel_id.is_some(),
        "youtube_channel_name": user.youtube_channel_name,
        "youtube_channel_id": user.youtube_channel_id,
        "created_at": user.created_at,
    }))
}

// ── Sessions ────────────────────────────────────────────

/// POST /api/sessions → create session + streams per platform
pub async fn create_session(
    State(state): State<AppState>,
    Json(body): Json<CreateSessionBody>,
) -> impl IntoResponse {
    let target_langs_json = serde_json::to_string(&body.target_langs).unwrap_or_default();

    let session = db::create_session(
        &state.db,
        &body.user_id,
        body.voice_id.as_deref(),
        &body.title,
        &body.source_lang,
        &target_langs_json,
    )
    .await;

    // All languages: source + targets
    let mut all_langs = vec![body.source_lang.clone()];
    for lang in &body.target_langs {
        if !all_langs.contains(lang) {
            all_langs.push(lang.clone());
        }
    }

    // Default to YouTube if no platforms specified
    let platforms = if body.platforms.is_empty() {
        vec![PlatformConfig {
            platform: "youtube".to_string(),
            rtmp_url: None,
            stream_key: None,
        }]
    } else {
        body.platforms.clone()
    };

    let mut streams = Vec::new();
    let mut errors = Vec::new();

    for platform_config in &platforms {
        let platform = &platform_config.platform;

        match platform.as_str() {
            "youtube" => {
                // Auto-create YouTube broadcasts via API
                let access_token = match youtube::ensure_valid_token(&state.db, &body.user_id).await {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("[SESSION] No YouTube token: {}", e);
                        errors.push(format!("YouTube: {}", e));
                        continue;
                    }
                };

                let scheduled_start = chrono::Utc::now().to_rfc3339();

                for lang in &all_langs {
                    let stream_record = db::create_stream(&state.db, &session.id, lang, "youtube").await;
                    let broadcast_title = format!("{} [{}]", body.title, lang.to_uppercase());

                    let broadcast = match youtube::create_broadcast(&access_token, &broadcast_title, &scheduled_start).await {
                        Ok(b) => b,
                        Err(e) => {
                            eprintln!("[SESSION] YouTube broadcast failed for {}: {}", lang, e);
                            errors.push(format!("YouTube {}: {}", lang, e));
                            continue;
                        }
                    };

                    let yt_stream = match youtube::create_live_stream(&access_token, &broadcast_title).await {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("[SESSION] YouTube stream failed for {}: {}", lang, e);
                            errors.push(format!("YouTube stream {}: {}", lang, e));
                            continue;
                        }
                    };

                    let _ = youtube::bind_broadcast(&access_token, &broadcast.broadcast_id, &yt_stream.stream_id).await;

                    db::update_stream_platform(
                        &state.db,
                        &stream_record.id,
                        &broadcast.broadcast_id,
                        &yt_stream.stream_id,
                        &yt_stream.stream_key,
                        &yt_stream.rtmp_url,
                    )
                    .await;

                    streams.push(serde_json::json!({
                        "id": stream_record.id,
                        "lang": lang,
                        "platform": "youtube",
                        "broadcast_id": broadcast.broadcast_id,
                        "rtmp_url": format!("{}/{}", yt_stream.rtmp_url, yt_stream.stream_key),
                        "status": "ready",
                    }));
                }
            }
            // Manual RTMP platforms: 1 stream per platform account (1 RTMP key = 1 stream)
            _ => {
                let rtmp_url = match &platform_config.rtmp_url {
                    Some(u) => u.clone(),
                    None => {
                        errors.push(format!("{}: RTMP URL required", platform));
                        continue;
                    }
                };
                let stream_key = platform_config.stream_key.as_deref().unwrap_or("");

                // Use the first target language for this stream
                // (Phase 2 will allow per-language platform assignment)
                let lang = body.target_langs.first().map(|s| s.as_str()).unwrap_or(&body.source_lang);

                let record = db::create_stream_manual(
                    &state.db,
                    &session.id,
                    lang,
                    platform,
                    &rtmp_url,
                    stream_key,
                )
                .await;

                // Auto-save credential for next time
                let _ = db::upsert_platform_credential(
                    &state.db,
                    &body.user_id,
                    platform,
                    platform_config.rtmp_url.as_deref(),
                    platform_config.stream_key.as_deref(),
                    None,
                )
                .await;

                streams.push(serde_json::json!({
                    "id": record.id,
                    "lang": lang,
                    "platform": platform,
                    "rtmp_url": if stream_key.is_empty() {
                        rtmp_url.clone()
                    } else {
                        format!("{}/{}", rtmp_url, stream_key)
                    },
                    "status": "ready",
                }));
            }
        }
    }

    db::update_session_status(&state.db, &session.id, "live", None).await;

    let mut result = serde_json::json!({
        "session": session,
        "streams": streams,
    });
    if !errors.is_empty() {
        result["errors"] = serde_json::json!(errors);
    }
    Json(result)
}

/// POST /api/sessions/:id/streams → add a manual RTMP stream to existing session
pub async fn add_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AddStreamBody>,
) -> impl IntoResponse {
    if db::get_session(&state.db, &id).await.is_none() {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Session not found"}))).into_response();
    }

    let record = db::create_stream_manual(
        &state.db,
        &id,
        &body.lang,
        &body.platform,
        &body.rtmp_url,
        &body.stream_key,
    )
    .await;

    Json(serde_json::json!(record)).into_response()
}

/// DELETE /api/sessions/:session_id/streams/:stream_id → remove a stream
pub async fn remove_stream(
    State(state): State<AppState>,
    Path((_, stream_id)): Path<(String, String)>,
) -> impl IntoResponse {
    db::delete_stream(&state.db, &stream_id).await;
    Json(serde_json::json!({"status": "deleted"}))
}

/// GET /api/sessions?user_id=... → list user's sessions
pub async fn list_sessions(
    State(state): State<AppState>,
    Query(q): Query<UserIdQuery>,
) -> impl IntoResponse {
    let sessions = db::list_sessions(&state.db, &q.user_id).await;
    Json(serde_json::json!({ "sessions": sessions }))
}

/// GET /api/sessions/:id → session details + streams
pub async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let session = db::get_session(&state.db, &id).await;
    let streams = db::list_streams(&state.db, &id).await;
    Json(serde_json::json!({
        "session": session,
        "streams": streams,
    }))
}

/// DELETE /api/sessions/:id → end session, transition broadcasts to "complete"
pub async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let session = match db::get_session(&state.db, &id).await {
        Some(s) => s,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "Session not found"}))),
    };

    // End YouTube broadcasts (only for YouTube platform streams)
    let streams = db::list_streams(&state.db, &id).await;
    let yt_streams: Vec<_> = streams.iter().filter(|s| s.platform == "youtube").collect();
    if !yt_streams.is_empty() {
        if let Ok(access_token) = youtube::ensure_valid_token(&state.db, &session.user_id).await {
            for stream in &yt_streams {
                if let Some(broadcast_id) = &stream.platform_broadcast_id {
                    if let Err(e) = youtube::transition_broadcast(&access_token, broadcast_id, "complete").await {
                        eprintln!("[SESSION] Failed to end broadcast {}: {}", broadcast_id, e);
                    }
                }
            }
        }
    }

    db::update_session_status(&state.db, &id, "ended", None).await;
    (StatusCode::OK, Json(serde_json::json!({"status": "ended"})))
}

// ── Voices ──────────────────────────────────────────────

/// POST /api/voices → clone voice via ElevenLabs and save
pub async fn create_voice(
    State(state): State<AppState>,
    Json(body): Json<CreateVoiceBody>,
) -> impl IntoResponse {
    use base64::Engine;

    let pcm = match base64::engine::general_purpose::STANDARD.decode(&body.audio_base64) {
        Ok(data) => data,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("Invalid base64: {}", e)})),
            )
                .into_response();
        }
    };

    // Clone voice via ElevenLabs
    let elevenlabs_voice_id = match clone_voice_elevenlabs(&pcm, &body.name).await {
        Ok(id) => id,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": format!("Voice clone failed: {}", e)})),
            )
                .into_response();
        }
    };

    let voice = db::create_voice(&state.db, &body.user_id, &elevenlabs_voice_id, &body.name).await;
    Json(serde_json::json!(voice)).into_response()
}

/// GET /api/voices?user_id=... → list user's saved voices
pub async fn list_voices(
    State(state): State<AppState>,
    Query(q): Query<UserIdQuery>,
) -> impl IntoResponse {
    let voices = db::list_voices(&state.db, &q.user_id).await;
    Json(serde_json::json!({ "voices": voices }))
}

/// DELETE /api/voices/:id → delete voice from DB + ElevenLabs
pub async fn delete_voice(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Get voice to find ElevenLabs ID
    if let Some(voice) = db::get_voice(&state.db, &id).await {
        // Delete from ElevenLabs
        let el_id = voice.elevenlabs_voice_id.clone();
        tokio::spawn(async move {
            crate::pipeline::delete_cloned_voice(&el_id).await;
        });
    }

    db::delete_voice_db(&state.db, &id).await;
    Json(serde_json::json!({"status": "deleted"}))
}

// ── Platform Credentials ────────────────────────────────

/// GET /api/credentials?user_id=... → list saved platform credentials
pub async fn list_credentials(
    State(state): State<AppState>,
    Query(q): Query<UserIdQuery>,
) -> impl IntoResponse {
    match db::list_platform_credentials(&state.db, &q.user_id).await {
        Ok(creds) => Json(serde_json::json!({ "credentials": creds })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/credentials → save/update a platform credential
pub async fn save_credential(
    State(state): State<AppState>,
    Json(body): Json<PlatformCredentialBody>,
) -> impl IntoResponse {
    match db::upsert_platform_credential(
        &state.db,
        &body.user_id,
        &body.platform,
        body.rtmp_url.as_deref(),
        body.stream_key.as_deref(),
        body.display_name.as_deref(),
    )
    .await
    {
        Ok(cred) => Json(cred).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// DELETE /api/credentials?user_id=...&platform=... → delete a platform credential
pub async fn delete_credential(
    State(state): State<AppState>,
    Query(q): Query<DeleteCredentialQuery>,
) -> impl IntoResponse {
    match db::delete_platform_credential(&state.db, &q.user_id, &q.platform).await {
        Ok(_) => Json(serde_json::json!({ "status": "deleted" })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ── Helpers ─────────────────────────────────────────────

/// Clone a voice via ElevenLabs API (PCM → WAV → upload)
async fn clone_voice_elevenlabs(pcm: &[u8], name: &str) -> Result<String, String> {
    // Convert PCM to WAV (44.1kHz mono 16-bit)
    let wav = pcm_to_wav(pcm, 44100);

    let api_key = std::env::var("ELEVENLABS_API_KEY").map_err(|_| "ELEVENLABS_API_KEY not set")?;

    let client = reqwest::Client::new();
    let part = reqwest::multipart::Part::bytes(wav)
        .file_name("voice_sample.wav")
        .mime_str("audio/wav")
        .unwrap();

    let form = reqwest::multipart::Form::new()
        .text("name", name.to_string())
        .part("files", part);

    let resp = client
        .post("https://api.elevenlabs.io/v1/voices/add")
        .header("xi-api-key", &api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("ElevenLabs request failed: {}", e))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("ElevenLabs error: {}", body));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    data["voice_id"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| "No voice_id in response".to_string())
}

/// Convert raw PCM (16-bit mono) to WAV
fn pcm_to_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let file_len = 36 + data_len;
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_len.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}
