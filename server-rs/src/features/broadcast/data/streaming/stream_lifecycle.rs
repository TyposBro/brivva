//! Stream health checking, crash detection, and cleanup.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use super::types::{QueuedAudio, MAX_FFMPEG_RESTARTS, THREAD_JOIN_TIMEOUT_SECS};

/// Internal stream handle — exposed to manager only.
pub(super) struct RtmpStream {
    pub(super) child: std::process::Child,
    pub(super) video_handle: Option<std::thread::JoinHandle<()>>,
    pub(super) audio_handle: Option<std::thread::JoinHandle<()>>,
    pub(super) audio_fifo: String,
    pub(super) lang: String,
    pub(super) rtmp_url: String,
    pub(super) audio_queue: Arc<StdMutex<VecDeque<QueuedAudio>>>,
    pub(super) stop_flag: Arc<AtomicBool>,
    pub(super) restart_count: u32,
    pub(super) rtmp_error: Arc<AtomicBool>,
}

/// Check a single stream's health. Returns restart info if it needs restarting.
pub(super) fn check_stream_health(id: &str, stream: &mut RtmpStream) -> Option<(String, String, String)> {
    match stream.child.try_wait() {
        Ok(Some(status)) => check_exited_process(id, stream, status),
        Ok(None) => check_rtmp_error(id, stream),
        Err(e) => {
            tracing::error!("[FFMPEG] Error checking process status for {}: {}", id, e);
            None
        }
    }
}

fn check_exited_process(
    id: &str,
    stream: &mut RtmpStream,
    status: std::process::ExitStatus,
) -> Option<(String, String, String)> {
    if stream.stop_flag.load(Ordering::Acquire) {
        return None;
    }
    let code = status.code().unwrap_or(-1);
    tracing::error!(
        "[FFMPEG] Process crashed for lang={}, exit={}, restarting...",
        stream.lang, code
    );
    if !can_restart(stream) {
        return None;
    }
    Some((id.to_string(), stream.lang.clone(), stream.rtmp_url.clone()))
}

fn check_rtmp_error(id: &str, stream: &mut RtmpStream) -> Option<(String, String, String)> {
    if !stream.rtmp_error.load(Ordering::Acquire) {
        return None;
    }
    if stream.stop_flag.load(Ordering::Acquire) {
        return None;
    }
    tracing::error!(
        "[FFMPEG] RTMP connection error for lang={}, killing for restart",
        stream.lang
    );
    let _ = stream.child.kill();
    let _ = stream.child.wait();
    if !can_restart(stream) {
        return None;
    }
    Some((id.to_string(), stream.lang.clone(), stream.rtmp_url.clone()))
}

fn can_restart(stream: &mut RtmpStream) -> bool {
    if stream.restart_count >= MAX_FFMPEG_RESTARTS {
        tracing::error!(
            "[FFMPEG] Failed to restart after {} attempts for lang={}",
            MAX_FFMPEG_RESTARTS, stream.lang
        );
        stream.stop_flag.store(true, Ordering::Release);
        return false;
    }
    true
}

/// Stop the stream, kill the process, remove the FIFO, and return restart state.
pub(super) fn cleanup_single_stream(old: &mut RtmpStream) -> (u32, Arc<StdMutex<VecDeque<QueuedAudio>>>) {
    old.stop_flag.store(true, Ordering::Release);
    let _ = old.child.kill();
    let _ = old.child.wait();
    let _ = std::fs::remove_file(&old.audio_fifo);
    (old.restart_count, old.audio_queue.clone())
}

/// Kill an FFmpeg child process and log the result.
pub(super) fn kill_ffmpeg_process(id: &str, child: &mut std::process::Child) {
    match child.kill() {
        Ok(_) => {
            let _ = child.wait();
            tracing::info!("[FFMPEG:{}] killed", id);
        }
        Err(e) => tracing::error!("[FFMPEG:{}] kill error: {}", id, e),
    }
}

/// Join video and audio drain threads with a timeout.
pub(super) async fn join_drain_threads(id: &str, stream: &mut RtmpStream) {
    let join_timeout = Duration::from_secs(THREAD_JOIN_TIMEOUT_SECS);
    for (label, handle) in [
        ("video", stream.video_handle.take()),
        ("audio", stream.audio_handle.take()),
    ] {
        if let Some(h) = handle {
            let id_clone = id.to_string();
            let result = tokio::time::timeout(join_timeout, async {
                tokio::task::spawn_blocking(move || h.join()).await
            })
            .await;
            log_thread_join_result(&id_clone, label, result);
        }
    }
}

fn log_thread_join_result(
    id: &str,
    label: &str,
    result: Result<Result<Result<(), Box<dyn std::any::Any + Send>>, tokio::task::JoinError>, tokio::time::error::Elapsed>,
) {
    match result {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(_))) => tracing::error!("[FFMPEG:{}] {} thread panicked", id, label),
        Ok(Err(_)) => tracing::warn!("[FFMPEG:{}] {} thread join cancelled", id, label),
        Err(_) => tracing::warn!("[FFMPEG:{}] {} thread join timed out ({}s), abandoning", id, label, THREAD_JOIN_TIMEOUT_SECS),
    }
}
