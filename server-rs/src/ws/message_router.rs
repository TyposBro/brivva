//! WebSocket message routing: binary (audio/video) and text (JSON commands).

use axum::extract::ws::Message;
use futures_util::StreamExt;
use tokio::sync::mpsc;

use crate::core::config::{MSG_TAG_AUDIO, MSG_TAG_VIDEO};
use crate::core::types::Sessions;

use super::rtmp_handlers;

type WsStream = futures_util::stream::SplitStream<axum::extract::ws::WebSocket>;

pub fn spawn_recv_task(
    mut ws_stream: WsStream,
    audio_tx: mpsc::UnboundedSender<Vec<u8>>,
    sessions: Sessions,
    session_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Binary(data) => {
                    handle_binary(&data, &audio_tx, &sessions, &session_id).await;
                }
                Message::Text(text) => {
                    handle_text(&text, &sessions, &session_id).await;
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    })
}

async fn handle_binary(
    data: &[u8],
    audio_tx: &mpsc::UnboundedSender<Vec<u8>>,
    sessions: &Sessions,
    session_id: &str,
) {
    if data.is_empty() { return; }
    match data[0] {
        MSG_TAG_AUDIO => { let _ = audio_tx.send(data[1..].to_vec()); }
        MSG_TAG_VIDEO => forward_video(data, sessions, session_id).await,
        tag => {
            tracing::warn!("[WS:{}] unknown tag 0x{:02x}, treating as audio: {}B", session_id, tag, data.len());
            let _ = audio_tx.send(data.to_vec());
        }
    }
}

async fn forward_video(data: &[u8], sessions: &Sessions, session_id: &str) {
    let mgr = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone());
    if let Some(manager) = mgr {
        let locked = manager.lock().await;
        locked.push_video_chunk(&data[1..]);
    }
}

async fn handle_text(text: &str, sessions: &Sessions, session_id: &str) {
    let json = match serde_json::from_str::<serde_json::Value>(text) {
        Ok(j) => j,
        Err(_) => return,
    };
    match json.get("type").and_then(|t| t.as_str()) {
        Some("video:codec") => rtmp_handlers::handle_video_codec(&json, sessions, session_id).await,
        Some("rtmp:config") => rtmp_handlers::handle_rtmp_config(&json, sessions, session_id).await,
        Some("rtmp:restart") => rtmp_handlers::handle_rtmp_restart(sessions, session_id).await,
        _ => {}
    }
}
