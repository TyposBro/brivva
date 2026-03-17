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

/// Helper to serialize a ServerMsg and wrap in a WS text frame
fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}

/// Axum handler — reads query params, then upgrades HTTP → WebSocket
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<RoomQuery>,
    State(rooms): State<Rooms>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, rooms, query))
}

async fn handle_socket(socket: WebSocket, rooms: Rooms, query: RoomQuery) {
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

    // Create audio channel for the pipeline
    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Spawn the STT → translate → TTS pipeline
    let source_lang = rooms.get(&room_id).map(|r| r.source_lang.clone()).unwrap_or(Lang::En);
    let pipeline_rooms = rooms.clone();
    let pipeline_rid = room_id.clone();
    tokio::spawn(async move {
        pipeline::start_stt(pipeline_rid, pipeline_rooms, source_lang, audio_rx).await;
    });

    // Read loop: host sends binary audio or text commands
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Binary(data) => {
                // Forward audio to the STT pipeline
                let _ = audio_tx.send(data.to_vec());
            }
            Message::Text(text) => {
                if text.contains("host:end") {
                    break;
                }
                // Handle face frame for live video + lip-sync
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&*text) {
                    if json.get("type").and_then(|v| v.as_str()) == Some("face:frame") {
                        if let Some(data) = json.get("data").and_then(|v| v.as_str()) {
                            let face_data = data.to_string();

                            // Store latest face for lip-sync pipeline
                            if let Some(mut room) = rooms.get_mut(&room_id) {
                                room.latest_face = Some(face_data.clone());
                            }

                            // Forward face frame to all guests for live video
                            if let Some(room) = rooms.get(&room_id) {
                                room.send_to_all_guests(to_ws(&ServerMsg::FaceFrame {
                                    data: face_data,
                                }));
                            }
                        }
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
    }

    send_task.abort();
    println!("Room {} closed", room_id);
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

    println!("Guest {} joined room {} ({})", guest_id, room_id, lang);

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
    println!("Guest {} left room {}", guest_id, room_id);
}
