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
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::pipeline;
use crate::types::{Guest, Lang, Room, RoomQuery, Rooms, ServerMsg};
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
    ws.on_upgrade(move |socket| handle_socket(socket, state.rooms, query))
}

async fn handle_socket(socket: WebSocket, rooms: Rooms, query: RoomQuery) {
    eprintln!("[WS] handle_socket called, role={}", query.role);
    let (sender, receiver) = socket.split();

    match query.role.as_str() {
        "host" => {
            let source_lang = query
                .source_lang
                .as_deref()
                .and_then(Lang::from_str)
                .unwrap_or(Lang::En);
            handle_host(sender, receiver, rooms, source_lang).await;
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
            handle_guest(sender, receiver, rooms, room_id, lang).await;
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
    rooms: Rooms,
    source_lang: Lang,
) {
    let room_id = generate_room_id();

    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();

    let mut room = Room::new(room_id.clone(), source_lang);
    room.host_tx = Some(host_tx);
    rooms.insert(room_id.clone(), room);

    // Send room:created immediately
    let _ = sender
        .send(to_ws(&ServerMsg::RoomCreated {
            room_id: room_id.clone(),
        }))
        .await;

    // Forward channel → host WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = host_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Audio channel + STT pipeline (lazy-started on first audio)
    let mut audio_tx: Option<mpsc::UnboundedSender<Vec<u8>>> = None;

    // Read loop: host sends binary audio or text commands
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Binary(data) => {
                // Lazy-start STT pipeline on first audio (avoids Deepgram timeout during voice setup)
                if audio_tx.is_none() {
                    let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
                    audio_tx = Some(tx);
                    let source_lang = rooms.get(&room_id).map(|r| r.source_lang.clone()).unwrap_or(Lang::En);
                    let pipeline_rooms = rooms.clone();
                    let pipeline_rid = room_id.clone();
                    tokio::spawn(async move {
                        pipeline::start_stt(pipeline_rid, pipeline_rooms, source_lang, rx).await;
                    });
                    eprintln!("[HOST] First audio received, STT pipeline started for room {}", room_id);
                }
                // Forward audio to the STT pipeline
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
                        // Forward face frames to all guests (live video)
                        Some("face:frame") => {
                            if let Some(data) = json.get("data").and_then(|v| v.as_str()) {
                                if let Some(room) = rooms.get(&room_id) {
                                    room.send_to_all_guests(to_ws(&ServerMsg::FaceFrame {
                                        data: data.to_string(),
                                    }));
                                }
                            }
                        }
                        // Voice sample for cloning (base64 PCM from dedicated recording)
                        Some("voice:sample") => {
                            if let Some(pcm_b64) = json.get("data").and_then(|v| v.as_str()) {
                                use base64::Engine;
                                if let Ok(pcm) = base64::engine::general_purpose::STANDARD.decode(pcm_b64) {
                                    eprintln!("[VOICE_CLONE] received voice sample: {} bytes PCM", pcm.len());
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

    // Host disconnected — clean up
    if let Some((_, room)) = rooms.remove(&room_id) {
        room.send_to_all_guests(to_ws(&ServerMsg::RoomClosed));

        // Delete cloned voice from ElevenLabs
        if let Some(voice_id) = &room.voice_clone_id {
            let voice_id = voice_id.clone();
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
    // Check room exists
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

    // Register guest
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

    // Send room:joined
    let _ = sender
        .send(to_ws(&ServerMsg::RoomJoined {
            room_id: room_id.clone(),
        }))
        .await;

    eprintln!("Guest {} joined room {} ({})", guest_id, room_id, lang);

    // Forward channel → guest WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = guest_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Wait for disconnect
    while let Some(Ok(msg)) = receiver.next().await {
        if matches!(msg, Message::Close(_)) {
            break;
        }
    }

    // Guest disconnected — remove from room
    if let Some(room) = rooms.get(&room_id) {
        room.guests.remove(&guest_id);
        let counts = room.guest_counts();
        room.send_to_host(to_ws(&ServerMsg::GuestCount { counts }));
    }

    send_task.abort();
    eprintln!("Guest {} left room {}", guest_id, room_id);
}
