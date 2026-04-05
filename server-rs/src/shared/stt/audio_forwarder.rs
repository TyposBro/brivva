//! Forward host audio to Gladia WebSocket + accumulate for passthrough.

use std::sync::Arc;
use tokio_tungstenite::tungstenite;
use tracing::{info, error, debug};

use super::state::WsStream;

/// Owned environment for the audio-forwarding task (crosses `tokio::spawn`).
pub(super) struct AudioForwardEnv {
    pub audio_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>>,
    pub sink: Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
    pub accumulator: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    pub session_id: String,
}

pub(super) async fn forward_audio_to_gladia(env: AudioForwardEnv) {
    let mut rx = env.audio_rx.lock().await;
    let mut chunk_count: u64 = 0;
    let mut total_bytes: u64 = 0;

    while let Some(data) = rx.recv().await {
        chunk_count += 1;
        total_bytes += data.len() as u64;
        log_progress(&env.session_id, chunk_count, total_bytes);
        accumulate_audio(&env.accumulator, &data);
        if send_to_gladia(&env.sink, data, &env.session_id).await.is_err() { break; }
    }

    send_stop_recording(&env, chunk_count, total_bytes).await;
}

fn log_progress(session_id: &str, chunk_count: u64, total_bytes: u64) {
    if chunk_count.is_multiple_of(100) {
        debug!("[STT:{}] forwarded {} audio chunks ({}KB total)", session_id, chunk_count, total_bytes / 1024);
    }
}

fn accumulate_audio(accumulator: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>, data: &[u8]) {
    if let Ok(mut acc) = accumulator.lock() {
        acc.push(data.to_vec());
    }
}

async fn send_to_gladia(
    sink: &Arc<tokio::sync::Mutex<futures_util::stream::SplitSink<WsStream, tungstenite::Message>>>,
    data: Vec<u8>,
    session_id: &str,
) -> Result<(), ()> {
    use futures_util::SinkExt;
    let mut sink = sink.lock().await;
    if sink.send(tungstenite::Message::Binary(data.into())).await.is_err() {
        error!("[STT:{}] Gladia sink write error, stopping audio forward", session_id);
        return Err(());
    }
    Ok(())
}

async fn send_stop_recording(env: &AudioForwardEnv, chunk_count: u64, total_bytes: u64) {
    use futures_util::SinkExt;
    info!("[STT:{}] sending stop_recording (total: {} chunks, {}KB)", env.session_id, chunk_count, total_bytes / 1024);
    let mut sink = env.sink.lock().await;
    let _ = sink.send(tungstenite::Message::Text(
        r#"{"type":"stop_recording"}"#.to_string().into()
    )).await;
}
