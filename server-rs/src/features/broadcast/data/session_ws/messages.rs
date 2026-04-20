use std::sync::Arc;
use tokio::sync::mpsc;

use crate::features::broadcast::data::pipeline;
use crate::features::broadcast::domain::{Lang, LiveSessions, PipelineConfig};

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
    if let Some(tx) = audio_tx.as_ref() {
        let _ = tx.try_send(data.clone());
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
    // Face video: push directly to FFmpeg. No preview, no guest broadcast.
    if let Some("face:frame") = json.get("type").and_then(|v| v.as_str())
        && let Some(data) = json.get("data").and_then(|v| v.as_str())
    {
        push_face_frame(live_sessions, live_session_id, data).await;
    }
}

async fn push_face_frame(live_sessions: &LiveSessions, live_session_id: &str, data: &str) {
    let rtmp_mgr = live_sessions
        .get(live_session_id)
        .and_then(|r| r.rtmp_manager.clone());
    let Some(mgr) = rtmp_mgr else { return };
    use base64::Engine;
    let Ok(jpeg_bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
        return;
    };
    let locked = mgr.lock().await;
    locked.push_video_frame(&jpeg_bytes);
}
