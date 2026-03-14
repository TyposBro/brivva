use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::Response,
};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::types::{ClientMsg, Guest, Lang, Room, Rooms, ServerMsg};

/// Helper to serialize a ServerMsg and wrap in a WS text frame
fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}

/// Axum handler — upgrades HTTP → WebSocket
pub async fn ws_handler(ws: WebSocketUpgrade, State(rooms): State<Rooms>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, rooms))
}

/// Called once the WebSocket is established.
/// First message from client determines role (host or guest).
async fn handle_socket(socket: WebSocket, rooms: Rooms) {
    // Split the socket into sender + receiver halves.
    // Why? You can't hold &mut to both read AND write at the same time (borrow checker).
    // Splitting gives you two independent owned handles.
    let (mut sender, mut receiver) = socket.split();

    // Wait for the first message to determine role
    let first_msg = match receiver.next().await {
        Some(Ok(Message::Text(text))) => text.to_string(),
        _ => return, // bad connection, bail
    };

    let client_msg: ClientMsg = match serde_json::from_str(&first_msg) {
        Ok(msg) => msg,
        Err(_) => {
            let _ = sender
                .send(to_ws(&ServerMsg::Error {
                    message: "Invalid first message".into(),
                }))
                .await;
            return;
        }
    };

    match client_msg {
        ClientMsg::HostCreate { source_lang } => {
            handle_host(sender, receiver, rooms, source_lang).await;
        }
        ClientMsg::GuestJoin { room_id, lang } => {
            handle_guest(sender, receiver, rooms, room_id, lang).await;
        }
        _ => return,
    }
}

/// Generate a 6-char room code (like your generateRoomId.ts)
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

    // Create a channel so other tasks can send messages TO the host's WebSocket.
    // mpsc = "multiple producer, single consumer"
    // - Multiple guests/tasks can send TO the host (multiple producers)
    // - One task reads from the channel and writes to the WebSocket (single consumer)
    // This is how you solve "multiple tasks need to write to one WebSocket" in Rust.
    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();

    // Create the room and insert into shared state
    let mut room = Room::new(room_id.clone(), source_lang);
    room.host_tx = Some(host_tx);
    rooms.insert(room_id.clone(), room);

    // Send room:created to host
    let _ = sender
        .send(to_ws(&ServerMsg::RoomCreated {
            room_id: room_id.clone(),
        }))
        .await;

    // Spawn a task to forward channel messages → host WebSocket.
    // This runs concurrently. Like a goroutine or a detached Promise.
    let send_task = tokio::spawn(async move {
        while let Some(msg) = host_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Read loop: process incoming messages from host
    let rooms_ref = rooms.clone();
    let rid = room_id.clone();
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Binary(data) => {
                // Host sent audio — forward to all guests (for now, just echo)
                // Later: pipe to STT → translate → TTS → broadcast
                if let Some(room) = rooms_ref.get(&rid) {
                    room.send_to_all_guests(Message::Binary(data));
                }
            }
            Message::Text(text) => {
                if let Ok(ClientMsg::HostEnd) = serde_json::from_str(&text) {
                    break;
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

    // Create channel for this guest (same pattern as host)
    let (guest_tx, mut guest_rx) = mpsc::unbounded_channel::<Message>();

    // Register guest in room
    {
        let room = rooms.get(&room_id).unwrap();
        room.guests.insert(
            guest_id.clone(),
            Guest {
                lang: lang.clone(),
                tx: guest_tx,
            },
        );

        // Notify host of updated guest counts
        let counts = room.guest_counts();
        room.send_to_host(to_ws(&ServerMsg::GuestCount { counts }));
    }

    // Send room:joined to guest
    let _ = sender
        .send(to_ws(&ServerMsg::RoomJoined {
            room_id: room_id.clone(),
            lang,
        }))
        .await;

    // Spawn task to forward channel → guest WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = guest_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Read loop — guests don't send much, just wait for disconnect
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
}
