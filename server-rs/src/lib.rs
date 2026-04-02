pub mod ffmpeg;
pub mod stt;
mod pipeline;
mod types;

use axum::{
    Router,
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
    routing::get,
};
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use tower_http::cors::{Any, CorsLayer};

use types::{Lang, Session, Sessions, ServerMsg};

/// Start the Axum server on localhost:3000.
pub async fn run_server() {
    // Prefer .env.local (localhost config), fall back to .env
    if dotenvy::from_filename(".env.local").is_err() {
        if let Err(e) = dotenvy::dotenv() {
            eprintln!("Warning: .env not loaded ({e}). Using existing environment variables.");
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
        .layer(cors)
        .with_state(sessions);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();

    println!("Brivva server on http://localhost:3000");
    axum::serve(listener, app).await.unwrap();
}

// ── WebSocket Handler ────────────────────────────────────

#[derive(Deserialize)]
struct WsQuery {
    #[serde(rename = "sourceLang")]
    source_lang: String,
    #[serde(rename = "targetLangs")]
    target_langs: String, // comma-separated: "ja,ko,zh"
    /// Translation tier: 1 = subtitles only, 2 = voice + subtitles
    #[serde(default = "default_tier")]
    tier: u8,
}

fn default_tier() -> u8 { 2 }

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
        eprintln!("[WS] No valid target languages, closing");
        return;
    }

    let session_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let tier = query.tier;
    println!("[WS] Session {} started: {} -> {:?} (tier {})", session_id, source_lang, target_langs, tier);

    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channel for sending messages back to the WebSocket
    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();

    // Create session
    let mut session = Session::new(session_id.clone(), source_lang.clone(), target_langs, tier);
    session.host_tx = Some(host_tx);
    sessions.insert(session_id.clone(), session);

    // Send session ID to client
    let session_msg = serde_json::to_string(&ServerMsg::SessionCreated { id: session_id.clone() }).unwrap();
    let _ = ws_sink.send(Message::Text(session_msg.into())).await;

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

    // Task: forward host_rx → WebSocket
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
                        0x01 => {
                            // Audio PCM (tagged)
                            let _ = audio_tx.send(data[1..].to_vec());
                        }
                        0x02 => {
                            // Encoded video chunk from MediaRecorder
                            let mgr = sessions_ref.get(&sid)
                                .and_then(|s| s.rtmp_manager.clone());
                            if let Some(manager) = mgr {
                                let locked = manager.lock().await;
                                locked.push_video_chunk(&data[1..]);
                            }
                        }
                        _ => {
                            // Legacy: untagged = raw audio
                            let _ = audio_tx.send(data.to_vec());
                        }
                    }
                }
                Message::Text(text) => {
                    // Handle JSON messages
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                        match json.get("type").and_then(|t| t.as_str()) {
                            Some("voice:sample") => {
                                if let Some(audio_b64) = json.get("audio").and_then(|a| a.as_str()) {
                                    if let Ok(pcm) = base64::Engine::decode(
                                        &base64::engine::general_purpose::STANDARD,
                                        audio_b64,
                                    ) {
                                        let sessions_clone = sessions_ref.clone();
                                        let sid_clone = sid.clone();
                                        tokio::spawn(async move {
                                            pipeline::clone_voice(pcm, &sessions_clone, &sid_clone).await;
                                        });
                                    }
                                }
                            }
                            Some("video:codec") => {
                                // Frontend reports MediaRecorder codec for FFmpeg passthrough
                                if let Some(codec) = json.get("codec").and_then(|c| c.as_str()) {
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
                                    // Accept optional broadcastDelay from frontend
                                    if let Some(delay) = json.get("broadcastDelay").and_then(|d| d.as_u64()) {
                                        // SAFETY: single-threaded at this point, no concurrent env reads
                                        unsafe { std::env::set_var("BROADCAST_DELAY_MS", delay.to_string()); }
                                    }

                                    let mut manager = ffmpeg::RtmpManager::new();

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
                                                    eprintln!("[RTMP] Failed to start {}: {}", lang, e);
                                                    if let Some(session) = sessions_ref.get(&sid) {
                                                        let err_msg = serde_json::to_string(&ServerMsg::Error {
                                                            message: format!("RTMP failed for {}: {}", lang, e),
                                                        }).unwrap();
                                                        session.send_to_host(Message::Text(err_msg.into()));
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

                                    eprintln!("[RTMP] Started {} stream(s): {:?}", rtmp_langs.len(), rtmp_langs);
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
    println!("[WS] Session {} ended", session_id);
    if let Some((_, session)) = sessions.remove(&session_id) {
        // Stop RTMP streams and health monitor
        session.rtmp_stop.store(true, Ordering::Release);
        if let Some(mgr) = session.rtmp_manager {
            let mut locked = mgr.lock().await;
            locked.stop_all().await;
        }
        // Delete cloned voice
        if let Some(voice_id) = session.voice_clone_id {
            tokio::spawn(async move {
                pipeline::delete_cloned_voice(&voice_id).await;
            });
        }
    }
}
