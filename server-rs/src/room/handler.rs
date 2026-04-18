use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::auth;
use crate::pipeline;
use crate::types::{Lang, Room, RoomQuery};
use crate::workers_api;
use crate::AppState;

/// WS entry. Accepts only authenticated hosts — no guests, no room codes.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<RoomQuery>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_host_socket(socket, state, query))
}

async fn handle_host_socket(socket: WebSocket, state: AppState, query: RoomQuery) {
    // Auth gate — host must present a valid Workers-signed JWT.
    let token = match query.token.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => {
            eprintln!("[WS] host without token, rejecting");
            return;
        }
    };
    let claims = match auth::verify(token) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[WS] host JWT verify failed: {}", e);
            return;
        }
    };

    let source_lang = query
        .source_lang
        .as_deref()
        .and_then(Lang::from_str)
        .unwrap_or(Lang::En);

    let (sender, receiver) = socket.split();
    handle_host(sender, receiver, state, claims.sub, source_lang, query.session_id).await;
}

fn generate_room_id() -> String {
    Uuid::new_v4().to_string()[..6].to_uppercase()
}

// ── Host Flow ─────────────────────────────────────────────

async fn handle_host(
    mut sender: SplitSink<WebSocket, Message>,
    mut receiver: SplitStream<WebSocket>,
    state: AppState,
    user_id: String,
    source_lang: Lang,
    session_id: Option<String>,
) {
    let rooms = state.rooms.clone();
    let room_id = generate_room_id();

    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();

    let mut room = Room::new(room_id.clone(), source_lang.clone(), session_id.clone());
    room.host_tx = Some(host_tx);

    let ffmpeg_monitor_stop = Arc::new(AtomicBool::new(false));

    // Session context lives in Workers/D1. Fetch the bundle + start FFmpeg per stream.
    if let Some(ref sid) = session_id {
        match workers_api::fetch_session_bundle(sid).await {
            Ok(bundle) => {
                if bundle.session.user_id != user_id {
                    eprintln!(
                        "[WS] session {} owner mismatch ({} vs jwt {})",
                        sid, bundle.session.user_id, user_id
                    );
                    return;
                }

                if let Some(v) = bundle.voice {
                    room.voice_clone_id = Some(v.elevenlabs_voice_id);
                }

                if !bundle.streams.is_empty() {
                    let mut manager = crate::ffmpeg::RtmpManager::new();
                    let mut rtmp_langs = Vec::new();
                    for s in &bundle.streams {
                        let (Some(rtmp_url), Some(stream_key)) =
                            (&s.rtmp_url, &s.stream_key)
                        else {
                            continue;
                        };
                        let full_url = if stream_key.is_empty() {
                            rtmp_url.clone()
                        } else {
                            format!("{}/{}", rtmp_url.trim_end_matches('/'), stream_key)
                        };
                        let is_source = Lang::from_str(&s.lang)
                            .is_some_and(|l| l == source_lang);
                        if let Err(e) = manager.start_stream(
                            &s.id,
                            &s.lang,
                            &full_url,
                            s.delay_ms,
                            is_source,
                            s.host_gain.clamp(0.0, 1.0),
                        ) {
                            eprintln!("[RTMP] Failed to start stream {}: {}", s.id, e);
                        } else if let Some(lang) = Lang::from_str(&s.lang) {
                            rtmp_langs.push(lang);
                        }
                    }
                    let shared_mgr = Arc::new(tokio::sync::Mutex::new(manager));
                    room.rtmp_manager = Some(shared_mgr.clone());
                    room.rtmp_langs = rtmp_langs;
                    eprintln!(
                        "[RTMP] Started {} FFmpeg streams for session {}, langs: {:?}",
                        bundle.streams.len(),
                        sid,
                        room.rtmp_langs
                    );
                    let _health_monitor = crate::ffmpeg::spawn_health_monitor(
                        shared_mgr,
                        ffmpeg_monitor_stop.clone(),
                    );
                }

                // Best-effort status update — don't block WS on the write.
                let sid_clone = sid.clone();
                let rid_clone = room_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = workers_api::update_session_status(
                        &sid_clone,
                        "live",
                        Some(&rid_clone),
                    )
                    .await
                    {
                        eprintln!("[WS] status→live failed: {}", e);
                    }
                });
            }
            Err(e) => {
                eprintln!("[WS] failed to fetch session {}: {}", sid, e);
                // Continue without RTMP — host still gets STT feedback.
            }
        }
    }

    rooms.insert(room_id.clone(), room);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = host_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut audio_tx: Option<mpsc::UnboundedSender<Vec<u8>>> = None;

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Binary(data) => {
                if audio_tx.is_none() {
                    let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
                    audio_tx = Some(tx);
                    let pipeline_rooms = rooms.clone();
                    let pipeline_rid = room_id.clone();
                    let source_lang = source_lang.clone();
                    tokio::spawn(async move {
                        pipeline::start_stt(pipeline_rid, pipeline_rooms, source_lang, rx).await;
                    });
                    eprintln!(
                        "[HOST] First audio received, STT pipeline started for room {}",
                        room_id
                    );
                }
                if let Some(ref tx) = audio_tx {
                    let _ = tx.send(data.to_vec());
                }
                // Also feed the per-stream RTMP mixer so delayed host audio
                // is available to underlay the translated TTS.
                let rtmp_mgr = rooms.get(&room_id).and_then(|r| r.rtmp_manager.clone());
                if let Some(mgr) = rtmp_mgr {
                    let bytes = data.to_vec();
                    tokio::spawn(async move {
                        mgr.lock().await.push_host_audio(&bytes);
                    });
                }
            }
            Message::Text(text) => {
                if text.contains("host:end") {
                    break;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&*text) {
                    match json.get("type").and_then(|v| v.as_str()) {
                        // Face video: push directly to FFmpeg. No preview, no guest broadcast.
                        Some("face:frame") => {
                            if let Some(data) = json.get("data").and_then(|v| v.as_str()) {
                                let rtmp_mgr = rooms
                                    .get(&room_id)
                                    .and_then(|r| r.rtmp_manager.clone());
                                if let Some(mgr) = rtmp_mgr {
                                    use base64::Engine;
                                    if let Ok(jpeg_bytes) =
                                        base64::engine::general_purpose::STANDARD.decode(data)
                                    {
                                        let locked = mgr.lock().await;
                                        locked.push_video_frame(&jpeg_bytes);
                                    }
                                }
                            }
                        }
                        Some("voice:sample") => {
                            if let Some(pcm_b64) = json.get("data").and_then(|v| v.as_str()) {
                                use base64::Engine;
                                if let Ok(pcm) =
                                    base64::engine::general_purpose::STANDARD.decode(pcm_b64)
                                {
                                    eprintln!(
                                        "[VOICE_CLONE] received voice sample: {} bytes PCM",
                                        pcm.len()
                                    );
                                    let rooms_clone = rooms.clone();
                                    let rid = room_id.clone();
                                    tokio::spawn(async move {
                                        pipeline::clone_voice(pcm, &rooms_clone, &rid).await;
                                    });
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

    ffmpeg_monitor_stop.store(true, Ordering::Release);

    if let Some((_, room)) = rooms.remove(&room_id) {
        if let Some(manager) = room.rtmp_manager {
            let mut mgr = manager.lock().await;
            mgr.stop_all().await;
        }

        if let Some(sid) = room.session_id {
            tokio::spawn(async move {
                if let Err(e) = workers_api::update_session_status(&sid, "ended", None).await {
                    eprintln!("[WS] status→ended failed: {}", e);
                }
            });
        }

        if let Some(voice_id) = room.voice_clone_id {
            tokio::spawn(async move {
                pipeline::delete_cloned_voice(&voice_id).await;
            });
        }
    }

    send_task.abort();
    eprintln!("Room {} closed", room_id);
}
