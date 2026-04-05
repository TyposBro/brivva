//! Session cleanup on WebSocket disconnect.

use std::sync::atomic::Ordering;
use crate::core::types::Sessions;

pub async fn cleanup_session(session_id: &str, sessions: &Sessions) {
    tracing::info!("[WS] Session {} ending -- starting cleanup", session_id);
    if let Some((_, session)) = sessions.remove(session_id) {
        stop_rtmp(session_id, &session).await;
    }
    tracing::info!("[WS] Session {} cleanup complete", session_id);
}

async fn stop_rtmp(session_id: &str, session: &crate::core::types::Session) {
    tracing::info!("[WS:{}] signaling RTMP stop", session_id);
    session.rtmp_stop.store(true, Ordering::Release);
    if let Some(ref mgr) = session.rtmp_manager {
        tracing::info!("[WS:{}] stopping all RTMP streams", session_id);
        let mut locked = mgr.lock().await;
        locked.stop_all().await;
        tracing::info!("[WS:{}] all RTMP streams stopped", session_id);
    }
}
