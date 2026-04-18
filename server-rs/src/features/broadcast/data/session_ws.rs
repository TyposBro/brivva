use axum::{
    extract::{
        Query, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::Response,
};
use futures_util::{
    SinkExt, StreamExt,
    stream::{SplitSink, SplitStream},
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::features::broadcast::data::{auth, pipeline, workers_api};
use crate::features::broadcast::domain::{Lang, LiveSession, SessionQuery};
use crate::orchestration::state::AppState;

/// WS entry. Accepts only authenticated hosts — no guests, no join codes.
pub async fn session_ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<SessionQuery>,
    State(state): State<AppState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_host_socket(socket, state, query))
}

async fn handle_host_socket(socket: WebSocket, state: AppState, query: SessionQuery) {
    // Auth gate — host must present a valid Workers-signed JWT.
    let token = match query.token.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => {
            tracing::warn!("ws host upgrade rejected: missing token");
            return;
        }
    };
    let claims = match auth::verify(token) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "ws host upgrade rejected: jwt verify failed");
            return;
        }
    };

    let source_lang = query
        .source_lang
        .as_deref()
        .and_then(Lang::from_str)
        .unwrap_or(Lang::En);

    let (sender, receiver) = socket.split();
    handle_host(
        sender,
        receiver,
        state,
        claims.sub,
        source_lang,
        query.session_id,
    )
    .await;
}

fn generate_live_session_id() -> String {
    Uuid::new_v4().to_string()[..6].to_uppercase()
}

fn next_available_live_session_id(live_sessions: &dashmap::DashMap<String, LiveSession>) -> String {
    for _ in 0..16 {
        let candidate = generate_live_session_id();
        if !live_sessions.contains_key(&candidate) {
            return candidate;
        }
    }

    loop {
        let candidate = Uuid::new_v4().simple().to_string()[..10].to_uppercase();
        if !live_sessions.contains_key(&candidate) {
            return candidate;
        }
    }
}

// ── Host Flow ─────────────────────────────────────────────

async fn handle_host(
    mut sender: SplitSink<WebSocket, Message>,
    mut receiver: SplitStream<WebSocket>,
    state: AppState,
    user_id: String,
    source_lang: Lang,
    session_id: Option<String>,
) {
    let live_sessions = state.live_sessions.clone();
    let live_session_id = next_available_live_session_id(&live_sessions);

    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();

    let mut live_session = LiveSession::new(
        live_session_id.clone(),
        source_lang.clone(),
        session_id.clone(),
    );
    live_session.host_tx = Some(host_tx);

    let ffmpeg_monitor_stop = Arc::new(AtomicBool::new(false));

    // Session context lives in Workers/D1. Fetch the bundle + start FFmpeg per stream.
    if let Some(ref sid) = session_id {
        match workers_api::fetch_session_bundle(sid).await {
            Ok(bundle) => {
                if bundle.session.user_id != user_id {
                    tracing::warn!(
                        session_id = %sid,
                        owner_user_id = %bundle.session.user_id,
                        jwt_user_id = %user_id,
                        "ws host upgrade rejected: session owner mismatch"
                    );
                    return;
                }

                if let Some(v) = bundle.voice {
                    live_session.selected_voice_id = Some(v.elevenlabs_voice_id);
                }

                if !bundle.streams.is_empty() {
                    let mut manager = crate::features::broadcast::data::ffmpeg::RtmpManager::new();
                    let mut rtmp_langs = Vec::new();
                    for s in &bundle.streams {
                        let (Some(rtmp_url), Some(stream_key)) = (&s.rtmp_url, &s.stream_key)
                        else {
                            continue;
                        };
                        let full_url = if stream_key.is_empty() {
                            rtmp_url.clone()
                        } else {
                            format!("{}/{}", rtmp_url.trim_end_matches('/'), stream_key)
                        };
                        let is_source = Lang::from_str(&s.lang).is_some_and(|l| l == source_lang);
                        if let Err(e) = manager.start_stream(
                            &s.id,
                            &s.lang,
                            &full_url,
                            s.delay_ms,
                            is_source,
                            s.host_gain.clamp(0.0, 1.0),
                        ) {
                            tracing::error!(
                                stream_id = %s.id,
                                lang = %s.lang,
                                error = %e,
                                "rtmp stream start failed"
                            );
                        } else if let Some(lang) = Lang::from_str(&s.lang) {
                            rtmp_langs.push(lang);
                        }
                    }
                    let shared_mgr = Arc::new(tokio::sync::Mutex::new(manager));
                    live_session.rtmp_manager = Some(shared_mgr.clone());
                    live_session.rtmp_langs = rtmp_langs;
                    tracing::info!(
                        session_id = %sid,
                        stream_count = bundle.streams.len(),
                        langs = ?live_session.rtmp_langs,
                        "ffmpeg rtmp streams started"
                    );
                    let _health_monitor =
                        crate::features::broadcast::data::ffmpeg::spawn_health_monitor(
                            shared_mgr,
                            ffmpeg_monitor_stop.clone(),
                        );
                }

                // Best-effort status update — don't block WS on the write.
                let sid_clone = sid.clone();
                let live_session_id_clone = live_session_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = workers_api::update_session_status(
                        &sid_clone,
                        "live",
                        Some(&live_session_id_clone),
                    )
                    .await
                    {
                        tracing::warn!(
                            session_id = %sid_clone,
                            error = %e,
                            "workers status=live update failed"
                        );
                    }
                });
            }
            Err(e) => {
                tracing::error!(session_id = %sid, error = %e, "session bundle fetch failed");
                return;
            }
        }
    }

    live_sessions.insert(live_session_id.clone(), live_session);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = host_rx.recv().await {
            if sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut audio_tx: Option<mpsc::Sender<Vec<u8>>> = None;

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Binary(data) => {
                if audio_tx.is_none() {
                    let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
                    audio_tx = Some(tx);
                    let pipeline_live_sessions = live_sessions.clone();
                    let pipeline_live_session_id = live_session_id.clone();
                    let source_lang = source_lang.clone();
                    let target_langs = live_sessions
                        .get(&live_session_id)
                        .map(|r| r.rtmp_langs.clone())
                        .unwrap_or_default();
                    tokio::spawn(async move {
                        pipeline::start_stt_pipelines(
                            pipeline_live_session_id,
                            pipeline_live_sessions,
                            source_lang,
                            target_langs,
                            rx,
                        )
                        .await;
                    });
                    tracing::info!(
                        live_session_id = %live_session_id,
                        "first host audio received, STT pipelines spawned"
                    );
                }
                if let Some(ref tx) = audio_tx {
                    let _ = tx.try_send(data.to_vec());
                }
                // Also feed the per-stream RTMP mixer so delayed host audio
                // is available to underlay the translated TTS.
                let rtmp_mgr = live_sessions
                    .get(&live_session_id)
                    .and_then(|r| r.rtmp_manager.clone());
                if let Some(mgr) = rtmp_mgr {
                    let bytes = data.to_vec();
                    tokio::spawn(async move {
                        mgr.lock().await.push_host_audio(&bytes);
                    });
                }
            }
            Message::Text(text) => {
                if text.contains("host:end") {
                    break;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&*text) {
                    match json.get("type").and_then(|v| v.as_str()) {
                        // Face video: push directly to FFmpeg. No preview, no guest broadcast.
                        Some("face:frame") => {
                            if let Some(data) = json.get("data").and_then(|v| v.as_str()) {
                                let rtmp_mgr = live_sessions
                                    .get(&live_session_id)
                                    .and_then(|r| r.rtmp_manager.clone());
                                if let Some(mgr) = rtmp_mgr {
                                    use base64::Engine;
                                    if let Ok(jpeg_bytes) =
                                        base64::engine::general_purpose::STANDARD.decode(data)
                                    {
                                        let locked = mgr.lock().await;
                                        locked.push_video_frame(&jpeg_bytes);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    ffmpeg_monitor_stop.store(true, Ordering::Release);

    if let Some((_, live_session)) = live_sessions.remove(&live_session_id) {
        if let Some(manager) = live_session.rtmp_manager {
            let mut mgr = manager.lock().await;
            mgr.stop_all().await;
        }

        if let Some(sid) = live_session.session_id {
            tokio::spawn(async move {
                if let Err(e) = workers_api::update_session_status(&sid, "ended", None).await {
                    tracing::warn!(
                        session_id = %sid,
                        error = %e,
                        "workers status=ended update failed"
                    );
                }
            });
        }
    }

    send_task.abort();
    tracing::info!(live_session_id = %live_session_id, "live session closed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::domain::Lang;

    #[test]
    fn next_available_live_session_id_skips_existing_entries() {
        let live_sessions = dashmap::DashMap::new();
        live_sessions.insert(
            "ABC123".into(),
            LiveSession::new("ABC123".into(), Lang::En, None),
        );

        for _ in 0..32 {
            let id = next_available_live_session_id(&live_sessions);
            assert_ne!(id, "ABC123");
            assert!(!id.is_empty());
        }
    }
}
