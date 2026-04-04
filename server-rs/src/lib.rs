pub mod constants;
pub mod error;
pub mod ffmpeg;
pub mod stt;
pub mod translation;
pub mod tts;
pub mod voice_clone;
mod pipeline;
mod types;

use axum::{
    Router, Json,
    body::Bytes,
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use tokio::sync::mpsc;
use tower_http::cors::{Any, CorsLayer};

use constants::{
    BYTES_PER_SEC, MAX_BODY_SIZE, MSG_TAG_AUDIO, MSG_TAG_VIDEO, SERVER_ADDR, VOICE_CLONE_FILE,
};
use types::{Lang, Session, Sessions, ServerMsg};

pub(crate) static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .build()
        .expect("failed to build HTTP client")
});

/// Serialize a ServerMsg to a WebSocket text message, logging on failure.
fn to_ws_msg(msg: &ServerMsg) -> Option<Message> {
    match serde_json::to_string(msg) {
        Ok(s) => Some(Message::Text(s.into())),
        Err(e) => {
            tracing::error!("Failed to serialize ServerMsg: {}", e);
            None
        }
    }
}

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
        .route("/ws", get(ws_handler))
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

// -- WebSocket Handler --------------------------------------------

#[derive(Deserialize)]
struct WsQuery {
    #[serde(rename = "sourceLang")]
    source_lang: String,
    #[serde(rename = "targetLangs")]
    target_langs: String, // comma-separated: "ja,ko,zh"
    /// Translation tier: 1 = subtitles only, 2 = voice + subtitles
    #[serde(default = "default_tier")]
    tier: u8,
    /// ElevenLabs TTS model: "turbo" or "flash"
    #[serde(rename = "ttsModel", default = "default_tts_model")]
    tts_model: String,
}

fn default_tier() -> u8 { 2 }
fn default_tts_model() -> String { "turbo".to_string() }

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(sessions): State<Sessions>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, query, sessions))
}

async fn handle_socket(socket: WebSocket, query: WsQuery, sessions: Sessions) {
    let source_lang = Lang::from_str(&query.source_lang).unwrap_or(Lang::En);
    let target_langs: Vec<Lang> = query
        .target_langs
        .split(',')
        .filter_map(|s| Lang::from_str(s.trim()))
        .collect();

    if target_langs.is_empty() {
        tracing::warn!("[WS] No valid target languages, closing");
        return;
    }

    let session_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let tier = query.tier;
    let tts_model = match query.tts_model.as_str() {
        "flash" => "eleven_flash_v2_5",
        _ => constants::DEFAULT_TTS_MODEL,
    }.to_string();
    tracing::info!(
        "[WS] Session {} started: {} -> {:?} (tier {}, tts={})",
        session_id, source_lang, target_langs, tier, tts_model,
    );

    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channel for sending messages back to the WebSocket
    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();

    // Create session -- load persisted voice clone if available
    let mut session = Session::new(session_id.clone(), source_lang.clone(), target_langs, tier);
    session.tts_model = tts_model;
    session.host_tx = Some(host_tx);
    if let Some(vid) = voice_clone::load_persisted_voice() {
        session.voice_clone_id = Some(vid);
    }
    sessions.insert(session_id.clone(), session);

    // Send session ID to client
    if let Some(msg) = to_ws_msg(&ServerMsg::SessionCreated { id: session_id.clone() }) {
        let _ = ws_sink.send(msg).await;
    }

    // Audio channel for STT pipeline
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Start STT pipeline immediately
    {
        let sessions_clone = sessions.clone();
        let sid_clone = session_id.clone();
        let sl = source_lang.clone();
        tokio::spawn(async move {
            pipeline::start_stt(sid_clone, sessions_clone, sl, audio_rx).await;
        });
    }

    // Task: forward host_rx -> WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = host_rx.recv().await {
            if ws_sink.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Task: read from WebSocket
    let sessions_ref = sessions.clone();
    let sid = session_id.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Binary(data) => {
                    if data.is_empty() { continue; }
                    match data[0] {
                        MSG_TAG_AUDIO => {
                            let _ = audio_tx.send(data[1..].to_vec());
                        }
                        MSG_TAG_VIDEO => {
                            let mgr = sessions_ref.get(&sid)
                                .and_then(|s| s.rtmp_manager.clone());
                            if let Some(manager) = mgr {
                                let locked = manager.lock().await;
                                locked.push_video_chunk(&data[1..]);
                            }
                        }
                        tag => {
                            // Legacy: untagged = raw audio
                            tracing::warn!(
                                "[WS:{}] unknown tag 0x{:02x}, treating as audio: {}B",
                                sid, tag, data.len(),
                            );
                            let _ = audio_tx.send(data.to_vec());
                        }
                    }
                }
                Message::Text(text) => {
                    // Handle JSON messages
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        match json.get("type").and_then(|t| t.as_str()) {
                            Some("video:codec") => {
                                // Frontend reports MediaRecorder codec for FFmpeg passthrough
                                if let Some(codec) = json.get("codec").and_then(|c| c.as_str()) {
                                    tracing::info!("[WS:{}] video:codec = {}", sid, codec);
                                    if let Some(mgr) = sessions_ref.get(&sid)
                                        .and_then(|s| s.rtmp_manager.clone())
                                    {
                                        let mut locked = mgr.lock().await;
                                        locked.set_video_codec(codec);
                                    }
                                    // Store for streams not yet started
                                    if let Some(mut session) = sessions_ref.get_mut(&sid) {
                                        session.video_codec = Some(codec.to_string());
                                    }
                                }
                            }
                            Some("rtmp:config") => {
                                // Start RTMP streams per language
                                if let Some(streams) = json.get("streams").and_then(|s| s.as_array()) {
                                    tracing::info!(
                                        "[WS:{}] rtmp:config received: {} stream(s)",
                                        sid, streams.len(),
                                    );
                                    let delay_ms = json.get("broadcastDelay")
                                        .and_then(|d| d.as_u64())
                                        .unwrap_or(5000);

                                    // Store in session for TTS timeout calculation
                                    if let Some(mut session) = sessions_ref.get_mut(&sid) {
                                        session.broadcast_delay_ms = delay_ms;
                                    }

                                    let mut manager = ffmpeg::RtmpManager::with_delay(delay_ms);

                                    // Apply video codec if already reported
                                    if let Some(codec) = sessions_ref.get(&sid)
                                        .and_then(|s| s.video_codec.clone())
                                    {
                                        manager.set_video_codec(&codec);
                                    }

                                    let mut rtmp_langs = Vec::new();

                                    for stream_cfg in streams {
                                        if let (Some(lang), Some(url)) = (
                                            stream_cfg.get("lang").and_then(|l| l.as_str()),
                                            stream_cfg.get("url").and_then(|u| u.as_str()),
                                        ) {
                                            let stream_id = format!("{}_{}", &sid, lang);
                                            match manager.start_stream(&stream_id, lang, url) {
                                                Ok(_) => {
                                                    if let Some(l) = Lang::from_str(lang) {
                                                        rtmp_langs.push(l);
                                                    }
                                                }
                                                Err(e) => {
                                                    tracing::error!("[RTMP] Failed to start {}: {}", lang, e);
                                                    if let Some(session) = sessions_ref.get(&sid) {
                                                        if let Some(err_msg) = to_ws_msg(&ServerMsg::Error {
                                                            message: format!("RTMP failed for {}: {}", lang, e),
                                                        }) {
                                                            session.send_to_host(err_msg);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    let shared_mgr: ffmpeg::SharedRtmpManager =
                                        Arc::new(tokio::sync::Mutex::new(manager));

                                    // Start health monitor for crash recovery
                                    let health_stop = sessions_ref.get(&sid)
                                        .map(|s| s.rtmp_stop.clone())
                                        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
                                    ffmpeg::spawn_health_monitor(shared_mgr.clone(), health_stop);

                                    // Store in session
                                    if let Some(mut session) = sessions_ref.get_mut(&sid) {
                                        session.rtmp_manager = Some(shared_mgr);
                                        session.rtmp_langs = rtmp_langs.clone();
                                    }

                                    tracing::info!(
                                        "[RTMP] Started {} stream(s): {:?}",
                                        rtmp_langs.len(), rtmp_langs,
                                    );
                                }
                            }
                            Some("rtmp:restart") => {
                                tracing::info!("[WS:{}] rtmp:restart requested", sid);
                                let mgr = sessions_ref.get(&sid)
                                    .and_then(|s| s.rtmp_manager.clone());
                                if let Some(manager) = mgr {
                                    let mut locked = manager.lock().await;
                                    locked.restart_all().await;
                                    if let Some(session) = sessions_ref.get(&sid) {
                                        if let Some(msg) = to_ws_msg(&ServerMsg::Error {
                                            message: "RTMP streams restarted".to_string(),
                                        }) {
                                            session.send_to_host(msg);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    // Cleanup
    tracing::info!("[WS] Session {} ending -- starting cleanup", session_id);
    if let Some((_, session)) = sessions.remove(&session_id) {
        // Stop RTMP streams and health monitor
        tracing::info!("[WS:{}] signaling RTMP stop", session_id);
        session.rtmp_stop.store(true, Ordering::Release);
        if let Some(mgr) = session.rtmp_manager {
            tracing::info!("[WS:{}] stopping all RTMP streams", session_id);
            let mut locked = mgr.lock().await;
            locked.stop_all().await;
            tracing::info!("[WS:{}] all RTMP streams stopped", session_id);
        }
        // Voice clone persists across sessions -- don't delete on disconnect
    }
    tracing::info!("[WS] Session {} cleanup complete", session_id);
}
