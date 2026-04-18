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
use crate::types::{Guest, Lang, Room, RoomQuery, Rooms, ServerMsg};
use crate::workers_api;
use crate::AppState;

/// Helper to serialize a ServerMsg and wrap in a WS text frame
fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}

/// Axum handler — reads query params, then upgrades HTTP → WebSocket
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<RoomQuery>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state, query))
}

async fn handle_socket(socket: WebSocket, state: AppState, query: RoomQuery) {
    eprintln!("[WS] handle_socket called, role={}", query.role);
    let (sender, receiver) = socket.split();

    match query.role.as_str() {
        "host" => {
            // Host must present a valid Workers-issued JWT. Claims give us the
            // authenticated user id, which we match against the session owner
            // returned from the Workers /internal API.
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
            handle_host(sender, receiver, state, claims.sub, source_lang, query.session_id).await;
        }
        "guest" => {
            let room_id = match query.room_id {
                Some(id) => id,
                None => return,
            };
            let lang = query
                .lang
                .as_deref()
                .and_then(Lang::from_str)
                .unwrap_or(Lang::En);
            handle_guest(sender, receiver, state.rooms, room_id, lang).await;
        }
        _ => return,
    }
}

/// Generate a 6-char room code
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

    // If session_id provided, fetch bundle from Workers (streams, voice) and
    // boot up FFmpeg for each configured RTMP destination.
    if let Some(ref sid) = session_id {
        match workers_api::fetch_session_bundle(sid).await {
            Ok(bundle) => {
                // Owner check — JWT user must match session.user_id.
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
                            format!(
                                "{}/{}",
                                rtmp_url.trim_end_matches('/'),
                                stream_key
                            )
                        };
                        if let Err(e) = manager.start_stream(&s.id, &s.lang, &full_url) {
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
                // Continue without RTMP — host can still stream to guests only.
            }
        }
    }

    rooms.insert(room_id.clone(), room);

    let _ = sender
        .send(to_ws(&ServerMsg::RoomCreated {
            room_id: room_id.clone(),
        }))
        .await;

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
            }
            Message::Text(text) => {
                if text.contains("host:end") {
                    break;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&*text) {
                    match json.get("type").and_then(|v| v.as_str()) {
                        Some("face:frame") => {
                            if let Some(data) = json.get("data").and_then(|v| v.as_str()) {
                                let rtmp_mgr = rooms
                                    .get(&room_id)
                                    .and_then(|r| r.rtmp_manager.clone());

                                if let Some(room) = rooms.get(&room_id) {
                                    room.send_to_all_guests(to_ws(&ServerMsg::FaceFrame {
                                        data: data.to_string(),
                                    }));
                                }

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
        room.send_to_all_guests(to_ws(&ServerMsg::RoomClosed));

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

// ── Guest Flow ────────────────────────────────────────────

async fn handle_guest(
    mut sender: SplitSink<WebSocket, Message>,
    mut receiver: SplitStream<WebSocket>,
    rooms: Rooms,
    room_id: String,
    lang: Lang,
) {
    if !rooms.contains_key(&room_id) {
        let _ = sender
            .send(to_ws(&ServerMsg::Error {
                message: "Room not found".into(),
            }))
            .await;
        return;
    }

    let guest_id = Uuid::new_v4().to_string();
    let (guest_tx, mut guest_rx) = mpsc::unbounded_channel::<Message>();

    {
        let room = rooms.get(&room_id).unwrap();
        room.guests.insert(
            guest_id.clone(),
            Guest {
                lang: lang.clone(),
                tx: guest_tx,
            },
        );
        let counts = room.guest_counts();
        room.send_to_host(to_ws(&ServerMsg::GuestCount { counts }));
    }

    let _ = sender
        .send(to_ws(&ServerMsg::RoomJoined {
            room_id: room_id.clone(),
        }))
        .await;

    eprintln!("Guest {} joined room {} ({})", guest_id, room_id, lang);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = guest_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        if matches!(msg, Message::Close(_)) {
            break;
        }
    }

    if let Some(room) = rooms.get(&room_id) {
        room.guests.remove(&guest_id);
        let counts = room.guest_counts();
        room.send_to_host(to_ws(&ServerMsg::GuestCount { counts }));
    }

    send_task.abort();
    eprintln!("Guest {} left room {}", guest_id, room_id);
}
