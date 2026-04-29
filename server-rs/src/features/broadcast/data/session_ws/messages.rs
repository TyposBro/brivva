use std::sync::Arc;
use std::time::Instant;

use base64::Engine as _;
use tokio::sync::mpsc;

use crate::features::broadcast::data::pipeline;
use crate::features::broadcast::domain::{Lang, LiveSessions, PipelineConfig};

use super::webrtc::{WebRtcOffer, handle_webrtc_offer};

pub(super) struct BinaryArgs<'a> {
    pub data: Vec<u8>,
    pub live_sessions: &'a LiveSessions,
    pub live_session_id: &'a str,
    pub source_lang: &'a Lang,
    pub audio_tx: &'a mut Option<mpsc::Sender<Vec<u8>>>,
}

pub(super) fn handle_binary(args: BinaryArgs<'_>) {
    let BinaryArgs {
        data,
        live_sessions,
        live_session_id,
        source_lang,
        audio_tx,
    } = args;
    if audio_tx.is_none() {
        *audio_tx = Some(spawn_stt_pipeline(
            live_sessions,
            live_session_id,
            source_lang,
        ));
    }
    if let Some(tx) = audio_tx.as_ref()
        && let Err(error) = tx.try_send(data.clone())
    {
        // STT pipeline is behind or gone. Debug-level so a stuck
        // pipeline producing identical dropped-frame logs per audio
        // tick is greppable without drowning healthy sessions.
        tracing::debug!(
            session_id = %live_session_id,
            bytes = data.len(),
            error = %error,
            "dropped host audio chunk into stt pipeline channel"
        );
    }
    let rtmp_mgr = live_sessions
        .get(live_session_id)
        .and_then(|r| r.rtmp_manager.clone());
    if let Some(mgr) = rtmp_mgr {
        tokio::spawn(async move {
            mgr.lock().await.push_host_audio(&data);
        });
    }
}

fn spawn_stt_pipeline(
    live_sessions: &LiveSessions,
    live_session_id: &str,
    source_lang: &Lang,
) -> mpsc::Sender<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
    let (target_langs, pipeline_cfg) = live_sessions
        .get(live_session_id)
        .map(|r| (r.rtmp_langs.clone(), r.pipeline_config.clone()))
        .unwrap_or_else(|| (Vec::new(), Arc::new(PipelineConfig::default())));
    let session = pipeline::PipelineSession {
        handle: crate::features::broadcast::domain::LiveSessionHandle::new(
            live_session_id.to_string(),
            live_sessions.clone(),
        ),
        source_lang: source_lang.clone(),
        target_langs,
        config: pipeline_cfg,
    };
    tokio::spawn(async move {
        pipeline::start_stt_pipelines(session, rx).await;
    });
    tracing::info!(
        live_session_id = %live_session_id,
        "first host audio received, STT pipelines spawned"
    );
    tx
}

pub(super) async fn handle_text(text: &str, live_sessions: &LiveSessions, live_session_id: &str) {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    match json.get("type").and_then(|v| v.as_str()) {
        Some("webrtc:offer") => match serde_json::from_value::<WebRtcOffer>(json) {
            Ok(offer) => handle_webrtc_offer(offer, live_sessions, live_session_id).await,
            Err(error) => tracing::warn!(
                live_session_id = %live_session_id,
                error = %error,
                "invalid webrtc offer"
            ),
        },
        Some("client:media_stats") => {
            tracing::info!(
                live_session_id = %live_session_id,
                stats = %json.get("stats").cloned().unwrap_or(serde_json::Value::Null),
                "client media stats"
            );
        }
        Some("debug:h264_annexb") if accepts_debug_h264() => {
            if let Some(data) = json.get("data").and_then(|v| v.as_str()) {
                match base64::engine::general_purpose::STANDARD.decode(data) {
                    Ok(h264) => push_debug_h264(live_sessions, live_session_id, h264).await,
                    Err(error) => tracing::warn!(
                        live_session_id = %live_session_id,
                        error = %error,
                        "invalid debug h264 payload"
                    ),
                }
            }
        }
        _ => {}
    }
}

fn accepts_debug_h264() -> bool {
    matches!(
        std::env::var("BRIVVA_ACCEPT_DEBUG_H264").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}

async fn push_debug_h264(live_sessions: &LiveSessions, live_session_id: &str, h264: Vec<u8>) {
    let rtmp_mgr = live_sessions
        .get(live_session_id)
        .and_then(|r| r.rtmp_manager.clone());
    if let Some(mgr) = rtmp_mgr {
        mgr.lock().await.push_video_h264_at(&h264, Instant::now());
    }
}
