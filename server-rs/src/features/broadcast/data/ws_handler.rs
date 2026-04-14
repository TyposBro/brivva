//! WebSocket handler for client connections.

use axum::{
    extract::{Query, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::features::broadcast::domain::{Sessions, ServerMsg};

/// Serialize a ServerMsg to a WebSocket text message.
pub(super) fn to_ws_msg(msg: &ServerMsg) -> Option<Message> {
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
    pub(super) source_lang: String,
    #[serde(rename = "targetLangs")]
    pub(super) target_langs: String,
    #[serde(default = "default_tier")]
    pub(super) tier: u8,
    #[serde(rename = "ttsModel", default = "default_tts_model")]
    pub(super) tts_model: String,
    #[serde(rename = "ttsProvider", default = "default_tts_provider")]
    pub(super) tts_provider: String,
    /// Comma-separated language codes that use default voice instead of the cloned voice.
    #[serde(rename = "voiceDefaultLangs", default)]
    pub(super) voice_default_langs: String,
    /// "female" or "male" — picks built-in default voice when no clone is active.
    #[serde(rename = "voiceGender", default = "default_voice_gender")]
    pub(super) voice_gender: String,
}

fn default_tier() -> u8 { 2 }
fn default_tts_model() -> String { "turbo".to_string() }
fn default_tts_provider() -> String { crate::core::config::DEFAULT_TTS_PROVIDER.to_string() }
fn default_voice_gender() -> String { "female".to_string() }

/// Dependencies injected from orchestration for broadcast WebSocket handlers.
#[derive(Clone)]
pub struct BroadcastDeps {
    pub stt_api_key: String,
    pub tts_api_key: String,
    pub dashscope_api_key: String,
    pub default_voice: String,
    pub http_client: reqwest::Client,
}

pub(crate) async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(sessions): State<Sessions>,
    axum::Extension(deps): axum::Extension<std::sync::Arc<BroadcastDeps>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, query, sessions, deps))
}

async fn handle_socket(
    socket: WebSocket,
    query: WsQuery,
    sessions: Sessions,
    deps: std::sync::Arc<BroadcastDeps>,
) {
    let (session_id, source_lang) = match super::session_setup::create_session(&query, &sessions) {
        Some(result) => result,
        None => return,
    };

    let (mut ws_sink, ws_stream) = socket.split();
    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();
    super::session_setup::attach_host_channel(&sessions, &session_id, host_tx);

    if let Some(msg) = to_ws_msg(&ServerMsg::SessionCreated { id: session_id.clone() }) {
        let _ = ws_sink.send(msg).await;
    }

    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    super::session_setup::spawn_stt_pipeline(&sessions, &session_id, &source_lang, audio_rx, &deps);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = host_rx.recv().await {
            if ws_sink.send(msg).await.is_err() { break; }
        }
    });

    let recv_task = super::message_router::spawn_recv_task(ws_stream, audio_tx, sessions.clone(), session_id.clone());

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    super::cleanup::cleanup_session(&session_id, &sessions).await;
}
