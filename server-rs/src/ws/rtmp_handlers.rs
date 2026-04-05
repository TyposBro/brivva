//! RTMP-related WebSocket message handlers.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::types::{Lang, Sessions, ServerMsg};
use crate::ffmpeg;

use super::to_ws_msg;

pub async fn handle_video_codec(json: &serde_json::Value, sessions: &Sessions, session_id: &str) {
    let codec = match json.get("codec").and_then(|c| c.as_str()) {
        Some(c) => c,
        None => return,
    };
    tracing::info!("[WS:{}] video:codec = {}", session_id, codec);
    propagate_codec_to_manager(sessions, session_id, codec).await;
    store_codec_in_session(sessions, session_id, codec);
}

pub async fn handle_rtmp_config(json: &serde_json::Value, sessions: &Sessions, session_id: &str) {
    let streams = match json.get("streams").and_then(|s| s.as_array()) {
        Some(s) => s,
        None => return,
    };
    tracing::info!("[WS:{}] rtmp:config received: {} stream(s)", session_id, streams.len());

    let delay_ms = json.get("broadcastDelay").and_then(|d| d.as_u64()).unwrap_or(5000);
    store_broadcast_delay(sessions, session_id, delay_ms);

    let mut manager = ffmpeg::RtmpManager::with_delay(delay_ms);
    apply_existing_codec(sessions, session_id, &mut manager);

    let rtmp_langs = start_all_streams(&mut manager, streams, sessions, session_id);
    let shared_mgr = Arc::new(tokio::sync::Mutex::new(manager));

    let health_stop = sessions.get(session_id)
        .map(|s| s.rtmp_stop.clone())
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    ffmpeg::spawn_health_monitor(shared_mgr.clone(), health_stop);

    store_rtmp_state(sessions, session_id, shared_mgr, &rtmp_langs);
    tracing::info!("[RTMP] Started {} stream(s): {:?}", rtmp_langs.len(), rtmp_langs);
}

pub async fn handle_rtmp_restart(sessions: &Sessions, session_id: &str) {
    tracing::info!("[WS:{}] rtmp:restart requested", session_id);
    let mgr = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone());
    if let Some(manager) = mgr {
        let mut locked = manager.lock().await;
        locked.restart_all().await;
        notify_host(sessions, session_id, "RTMP streams restarted");
    }
}

// -- Private Helpers --------------------------------------------------

async fn propagate_codec_to_manager(sessions: &Sessions, session_id: &str, codec: &str) {
    if let Some(mgr) = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone()) {
        let mut locked = mgr.lock().await;
        locked.set_video_codec(codec);
    }
}

fn store_codec_in_session(sessions: &Sessions, session_id: &str, codec: &str) {
    if let Some(mut session) = sessions.get_mut(session_id) {
        session.video_codec = Some(codec.to_string());
    }
}

fn store_broadcast_delay(sessions: &Sessions, session_id: &str, delay_ms: u64) {
    if let Some(mut session) = sessions.get_mut(session_id) {
        session.broadcast_delay_ms = delay_ms;
    }
}

fn apply_existing_codec(sessions: &Sessions, session_id: &str, manager: &mut ffmpeg::RtmpManager) {
    if let Some(codec) = sessions.get(session_id).and_then(|s| s.video_codec.clone()) {
        manager.set_video_codec(&codec);
    }
}

fn start_all_streams(
    manager: &mut ffmpeg::RtmpManager,
    streams: &[serde_json::Value],
    sessions: &Sessions,
    session_id: &str,
) -> Vec<Lang> {
    let mut rtmp_langs = Vec::new();
    for stream_cfg in streams {
        if let (Some(lang), Some(url)) = (
            stream_cfg.get("lang").and_then(|l| l.as_str()),
            stream_cfg.get("url").and_then(|u| u.as_str()),
        ) {
            let stream_id = format!("{}_{}", session_id, lang);
            match manager.start_stream(&stream_id, lang, url) {
                Ok(_) => {
                    if let Some(l) = Lang::from_str(lang) { rtmp_langs.push(l); }
                }
                Err(e) => {
                    tracing::error!("[RTMP] Failed to start {}: {}", lang, e);
                    notify_host(sessions, session_id, &format!("RTMP failed for {}: {}", lang, e));
                }
            }
        }
    }
    rtmp_langs
}

fn store_rtmp_state(sessions: &Sessions, session_id: &str, mgr: ffmpeg::SharedRtmpManager, langs: &[Lang]) {
    if let Some(mut session) = sessions.get_mut(session_id) {
        session.rtmp_manager = Some(mgr);
        session.rtmp_langs = langs.to_vec();
    }
}

fn notify_host(sessions: &Sessions, session_id: &str, message: &str) {
    if let Some(session) = sessions.get(session_id)
        && let Some(msg) = to_ws_msg(&ServerMsg::Error { message: message.to_string() }) {
            session.send_to_host(msg);
        }
}
