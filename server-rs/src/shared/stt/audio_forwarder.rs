//! Forward host audio to Soniox WebSocket + accumulate for passthrough.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio_tungstenite::tungstenite;
use tracing::{info, error, debug};

use super::config::SONIOX_KEEPALIVE_INTERVAL_SECS;
use super::state::WsStream;

pub(super) struct AudioForwardEnv {
    pub audio_rx: Arc<tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>>>,
    pub sink: Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
    pub accumulator: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    pub session_id: String,
}

pub(super) async fn forward_audio_to_stt(env: AudioForwardEnv) {
    let audio_flowing = Arc::new(AtomicBool::new(false));
    let keepalive_handle = spawn_keepalive(
        Arc::clone(&env.sink),
        Arc::clone(&audio_flowing),
        env.session_id.clone(),
    );

    let (chunk_count, total_bytes) = forward_loop(&env, &audio_flowing).await;
    keepalive_handle.abort();
    send_end_of_stream(&env, chunk_count, total_bytes).await;
}

async fn forward_loop(
    env: &AudioForwardEnv,
    audio_flowing: &Arc<AtomicBool>,
) -> (u64, u64) {
    let mut rx = env.audio_rx.lock().await;
    let mut chunk_count: u64 = 0;
    let mut total_bytes: u64 = 0;

    while let Some(data) = rx.recv().await {
        chunk_count += 1;
        total_bytes += data.len() as u64;
        audio_flowing.store(true, Ordering::Relaxed);
        log_progress(&env.session_id, chunk_count, total_bytes);
        accumulate_audio(&env.accumulator, &data);
        if send_audio(&env.sink, data, &env.session_id).await.is_err() {
            break;
        }
    }

    (chunk_count, total_bytes)
}

fn spawn_keepalive(
    sink: Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
    audio_flowing: Arc<AtomicBool>,
    session_id: String,
) -> tokio::task::JoinHandle<()> {
    let interval = Duration::from_secs(SONIOX_KEEPALIVE_INTERVAL_SECS);

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            if audio_flowing.swap(false, Ordering::Relaxed) {
                continue;
            }
            if send_keepalive(&sink, &session_id).await.is_err() {
                break;
            }
        }
    })
}

async fn send_keepalive(
    sink: &Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
    session_id: &str,
) -> Result<(), ()> {
    use futures_util::SinkExt;

    let mut sink = sink.lock().await;
    let msg = tungstenite::Message::Text(r#"{"type":"keepalive"}"#.to_string().into());
    if sink.send(msg).await.is_err() {
        debug!("[STT:{}] keepalive send failed, stopping", session_id);
        return Err(());
    }
    Ok(())
}

fn log_progress(session_id: &str, chunk_count: u64, total_bytes: u64) {
    if chunk_count.is_multiple_of(100) {
        debug!(
            "[STT:{}] forwarded {} audio chunks ({}KB total)",
            session_id,
            chunk_count,
            total_bytes / 1024,
        );
    }
}

fn accumulate_audio(accumulator: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>, data: &[u8]) {
    if let Ok(mut acc) = accumulator.lock() {
        acc.push(data.to_vec());
    }
}

async fn send_audio(
    sink: &Arc<tokio::sync::Mutex<
        futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
    >>,
    data: Vec<u8>,
    session_id: &str,
) -> Result<(), ()> {
    use futures_util::SinkExt;

    let mut sink = sink.lock().await;
    if sink.send(tungstenite::Message::Binary(data.into())).await.is_err() {
        error!("[STT:{}] sink write error, stopping audio forward", session_id);
        return Err(());
    }
    Ok(())
}

async fn send_end_of_stream(env: &AudioForwardEnv, chunk_count: u64, total_bytes: u64) {
    use futures_util::SinkExt;

    info!(
        "[STT:{}] sending end-of-stream (total: {} chunks, {}KB)",
        env.session_id,
        chunk_count,
        total_bytes / 1024,
    );
    let mut sink = env.sink.lock().await;
    let _ = sink.send(tungstenite::Message::Binary(vec![].into())).await;
}
