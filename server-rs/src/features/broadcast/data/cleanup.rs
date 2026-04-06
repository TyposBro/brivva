//! Session cleanup on WebSocket disconnect.
//!
//! Removing the session from the DashMap causes spawned STT and TTS tasks to
//! self-terminate: STT checks `sessions.contains_key()` on every message and
//! returns `Break` when the session is gone; TTS checks `sessions.get()` and
//! exits when the session is missing. Setting `rtmp_stop` ensures all FFmpeg
//! drain threads and the health monitor exit their loops.

use std::sync::atomic::Ordering;
use crate::features::broadcast::domain::Sessions;

pub async fn cleanup_session(session_id: &str, sessions: &Sessions) {
    tracing::info!("[WS] Session {} ending -- starting cleanup", session_id);
    if let Some((_, session)) = sessions.remove(session_id) {
        stop_rtmp(session_id, &session).await;
    }
    tracing::info!("[WS] Session {} cleanup complete", session_id);
}

async fn stop_rtmp(session_id: &str, session: &crate::features::broadcast::domain::Session) {
    tracing::info!("[WS:{}] signaling RTMP stop", session_id);
    session.rtmp_stop.store(true, Ordering::Release);
    if let Some(ref erased) = session.rtmp_manager {
        if let Some(mgr) = super::streaming::downcast_rtmp_manager(erased) {
            tracing::info!("[WS:{}] stopping all RTMP streams", session_id);
            let mut locked = mgr.lock().await;
            locked.stop_all().await;
            tracing::info!("[WS:{}] all RTMP streams stopped", session_id);
        }
    }
}
