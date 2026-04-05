//! WebSocket handler for client connections.

mod session_setup;
mod message_router;
mod rtmp_handlers;
mod cleanup;

use axum::{
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::features::broadcast::domain::{Sessions, ServerMsg};

/// Serialize a ServerMsg to a WebSocket text message.
fn to_ws_msg(msg: &ServerMsg) -> Option<Message> {
    match serde_json::to_string(msg) {
        Ok(s) => Some(Message::Text(s.into())),
        Err(e) => {
            tracing::error!("Failed to serialize ServerMsg: {}", e);
            None
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct WsQuery {
    #[serde(rename = "sourceLang")]
    source_lang: String,
    #[serde(rename = "targetLangs")]
    target_langs: String,
    #[serde(default = "default_tier")]
    tier: u8,
    #[serde(rename = "ttsModel", default = "default_tts_model")]
    tts_model: String,
}

fn default_tier() -> u8 { 2 }
fn default_tts_model() -> String { "turbo".to_string() }

pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(sessions): State<Sessions>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, query, sessions))
}

async fn handle_socket(socket: WebSocket, query: WsQuery, sessions: Sessions) {
    let (session_id, source_lang) = match session_setup::create_session(&query, &sessions) {
        Some(result) => result,
        None => return,
    };

    let (mut ws_sink, ws_stream) = socket.split();
    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();
    session_setup::attach_host_channel(&sessions, &session_id, host_tx);

    if let Some(msg) = to_ws_msg(&ServerMsg::SessionCreated { id: session_id.clone() }) {
        let _ = ws_sink.send(msg).await;
    }

    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    session_setup::spawn_stt_pipeline(&sessions, &session_id, &source_lang, audio_rx);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = host_rx.recv().await {
            if ws_sink.send(msg).await.is_err() { break; }
        }
    });

    let recv_task = message_router::spawn_recv_task(ws_stream, audio_tx, sessions.clone(), session_id.clone());

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    cleanup::cleanup_session(&session_id, &sessions).await;
}
