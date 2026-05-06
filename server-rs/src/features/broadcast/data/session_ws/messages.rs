use std::sync::Arc;
use std::time::Instant;

use base64::Engine as _;
use tokio::sync::mpsc;

use crate::features::broadcast::data::pipeline;
use crate::features::broadcast::data::session_log::SessionLogEmitter;
use crate::features::broadcast::domain::{Lang, LiveSessions, PipelineConfig};

use super::timeline_shadow::{
    AudioTimelineShadowSample, TimestampedAudioTimelineShadowSample, timeline_shadow_enabled,
    timeline_shadow_log_due,
};
use super::timestamped_audio::{BinaryAudioPayload, decode_binary_audio};
use super::webrtc::{WebRtcOffer, handle_webrtc_offer};

pub(super) struct BinaryArgs<'a> {
    pub data: Vec<u8>,
    pub live_sessions: &'a LiveSessions,
    pub live_session_id: &'a str,
    pub source_lang: &'a Lang,
    pub audio_tx: &'a mut Option<mpsc::Sender<Vec<u8>>>,
    pub session_log: &'a SessionLogEmitter,
    pub timeline_shadow: bool,
    pub timestamped_audio: bool,
    pub session_started_at: Instant,
    pub last_audio_timeline_shadow_log_at: &'a mut Option<Instant>,
}

pub(super) fn handle_binary(args: BinaryArgs<'_>) {
    let BinaryArgs {
        data,
        live_sessions,
        live_session_id,
        source_lang,
        audio_tx,
        session_log,
        timeline_shadow,
        timestamped_audio,
        session_started_at,
        last_audio_timeline_shadow_log_at,
    } = args;
    let queue_was_initialized = audio_tx.is_some();
    let now = Instant::now();
    let decoded = decode_binary_audio(&data, timestamped_audio);
    let (pcm_data, timestamped_sample) = match decoded {
        BinaryAudioPayload::RawPcm(pcm) => (pcm.to_vec(), None),
        BinaryAudioPayload::TimestampedPcm(frame) => {
            let sample = TimestampedAudioTimelineShadowSample::from_bridge(
                frame.pcm.len(),
                frame.sample_index,
                frame.sample_rate,
                frame.client_capture_time_us,
                frame.media_pts(),
            );
            (frame.pcm.to_vec(), Some(sample))
        }
        BinaryAudioPayload::RejectedTimestamped(error) => {
            tracing::warn!(
                live_session_id = %live_session_id,
                error = ?error,
                "rejected malformed timestamped audio frame"
            );
            return;
        }
    };
    if timeline_shadow_enabled(timeline_shadow)
        && timeline_shadow_log_due(last_audio_timeline_shadow_log_at, now)
    {
        let payload = timestamped_sample
            .map(|sample| sample.to_log_payload())
            .unwrap_or_else(|| {
                AudioTimelineShadowSample::from_arrival(
                    pcm_data.len(),
                    queue_was_initialized,
                    session_started_at,
                    now,
                )
                .to_log_payload()
            });
        tracing::info!(
            live_session_id = %live_session_id,
            payload = %payload,
            "v2 timeline shadow audio"
        );
    }
    if audio_tx.is_none() {
        *audio_tx = Some(spawn_stt_pipeline(
            live_sessions,
            live_session_id,
            source_lang,
        ));
        session_log.info("server.first_audio_received", serde_json::json!({}));
    }
    if let Some(tx) = audio_tx.as_ref()
        && let Err(error) = tx.try_send(pcm_data.clone())
    {
        // STT pipeline is behind or gone. Debug-level so a stuck
        // pipeline producing identical dropped-frame logs per audio
        // tick is greppable without drowning healthy sessions.
        tracing::debug!(
            session_id = %live_session_id,
            bytes = pcm_data.len(),
            error = %error,
            "dropped host audio chunk into stt pipeline channel"
        );
    }
    let rtmp_mgr = live_sessions
        .get(live_session_id)
        .and_then(|r| r.rtmp_manager.clone());
    if let Some(mgr) = rtmp_mgr {
        tokio::spawn(async move {
            mgr.lock().await.push_host_audio(&pcm_data);
        });
    }
}

fn spawn_stt_pipeline(
    live_sessions: &LiveSessions,
    live_session_id: &str,
    source_lang: &Lang,
) -> mpsc::Sender<Vec<u8>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
    let (target_langs, pipeline_cfg, translation_terms) = live_sessions
        .get(live_session_id)
        .map(|r| {
            (
                r.rtmp_langs.clone(),
                r.pipeline_config.clone(),
                r.translation_terms.clone(),
            )
        })
        .unwrap_or_else(|| (Vec::new(), Arc::new(PipelineConfig::default()), Vec::new()));
    let session = pipeline::PipelineSession {
        handle: crate::features::broadcast::domain::LiveSessionHandle::new(
            live_session_id.to_string(),
            live_sessions.clone(),
        ),
        source_lang: source_lang.clone(),
        target_langs,
        translation_terms,
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

pub(super) async fn handle_text(
    text: &str,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    session_log: &SessionLogEmitter,
    timeline_shadow: bool,
) {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    match json.get("type").and_then(|v| v.as_str()) {
        Some("webrtc:offer") => match serde_json::from_value::<WebRtcOffer>(json) {
            Ok(offer) => {
                handle_webrtc_offer(offer, live_sessions, live_session_id, timeline_shadow).await
            }
            Err(error) => tracing::warn!(
                live_session_id = %live_session_id,
                error = %error,
                "invalid webrtc offer"
            ),
        },
        Some("client:media_stats") => {
            let stats = json
                .get("stats")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            session_log.debug(
                "server.client_media_stats",
                serde_json::json!({ "stats": stats.clone() }),
            );
            tracing::info!(
                live_session_id = %live_session_id,
                stats = %stats,
                "client media stats"
            );
            for alert in media_quality_alerts(&stats) {
                session_log.warn(
                    "server.media_quality_warning",
                    alert.clone(),
                    serde_json::json!({ "alert": alert, "stats": stats.clone() }),
                );
                tracing::warn!(
                    live_session_id = %live_session_id,
                    alert = %alert,
                    stats = %stats,
                    "client media quality contract warning"
                );
            }
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

fn media_quality_alerts(stats: &serde_json::Value) -> Vec<String> {
    let mut alerts = Vec::new();
    push_track_quality_alerts(&mut alerts, stats, "sourceTrack", "capture");
    push_track_quality_alerts(&mut alerts, stats, "uplinkTrack", "uplink");

    if let Some(fps) = number_at(stats, &["cfr", "framesPerSecond"]) {
        push_fps_alert(&mut alerts, "cfr", fps);
    }
    if let Some(fps) = number_at(stats, &["outboundVideo", "framesPerSecond"]) {
        push_fps_alert(&mut alerts, "outbound", fps);
    }
    if let (Some(width), Some(height)) = (
        number_at(stats, &["outboundVideo", "frameWidth"]),
        number_at(stats, &["outboundVideo", "frameHeight"]),
    ) {
        push_resolution_alert(&mut alerts, "outbound", width, height);
    }
    if let Some(reason) = stats
        .pointer("/outboundVideo/qualityLimitationReason")
        .and_then(|v| v.as_str())
        .filter(|reason| !reason.is_empty() && *reason != "none")
    {
        alerts.push(format!("outbound quality limited: {reason}"));
    }
    alerts
}

fn push_track_quality_alerts(
    alerts: &mut Vec<String>,
    stats: &serde_json::Value,
    key: &str,
    label: &str,
) {
    if let (Some(width), Some(height)) = (
        number_at(stats, &[key, "width"]),
        number_at(stats, &[key, "height"]),
    ) {
        push_resolution_alert(alerts, label, width, height);
    }
    if let Some(fps) = number_at(stats, &[key, "frameRate"]) {
        push_fps_alert(alerts, label, fps);
    }
}

fn push_resolution_alert(alerts: &mut Vec<String>, label: &str, width: f64, height: f64) {
    let short_side = width.min(height);
    let long_side = width.max(height);
    if short_side < 720.0 || long_side < 1080.0 {
        alerts.push(format!(
            "{label} below HD portrait floor (short>=720 long>=1080): {}x{}",
            width.round() as u32,
            height.round() as u32
        ));
    }
}

fn push_fps_alert(alerts: &mut Vec<String>, label: &str, fps: f64) {
    if fps < 29.0 {
        alerts.push(format!("{label} below 30fps floor: {fps:.1}fps"));
    }
}

fn number_at(stats: &serde_json::Value, path: &[&str]) -> Option<f64> {
    let mut value = stats;
    for key in path {
        value = value.get(*key)?;
    }
    value.as_f64()
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn media_quality_alerts_warns_when_below_hd_portrait_or_fps_floor() {
        let stats = json!({
            "sourceTrack": { "width": 640, "height": 360, "frameRate": 24 },
            "uplinkTrack": { "width": 1920, "height": 1080, "frameRate": 30 },
            "cfr": { "framesPerSecond": 28.4 },
            "outboundVideo": {
                "frameWidth": 640,
                "frameHeight": 360,
                "framesPerSecond": 17,
                "qualityLimitationReason": "cpu"
            }
        });

        let alerts = media_quality_alerts(&stats);

        assert!(alerts.contains(
            &"capture below HD portrait floor (short>=720 long>=1080): 640x360".to_string()
        ));
        assert!(alerts.contains(&"capture below 30fps floor: 24.0fps".to_string()));
        assert!(alerts.contains(&"cfr below 30fps floor: 28.4fps".to_string()));
        assert!(alerts.contains(
            &"outbound below HD portrait floor (short>=720 long>=1080): 640x360".to_string()
        ));
        assert!(alerts.contains(&"outbound below 30fps floor: 17.0fps".to_string()));
        assert!(alerts.contains(&"outbound quality limited: cpu".to_string()));
    }

    #[test]
    fn media_quality_alerts_accepts_hd_portrait_and_landscape_quality_none() {
        let stats = json!({
            "sourceTrack": { "width": 720, "height": 1080, "frameRate": 30 },
            "uplinkTrack": { "width": 1920, "height": 1080, "frameRate": 30 },
            "cfr": { "framesPerSecond": 30 },
            "outboundVideo": {
                "frameWidth": 720,
                "frameHeight": 1080,
                "framesPerSecond": 29.8,
                "qualityLimitationReason": "none"
            }
        });

        assert!(media_quality_alerts(&stats).is_empty());
    }
}
