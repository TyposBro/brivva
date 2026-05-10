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
use super::timestamped_audio::{
    BinaryAudioPayload, HostAudioClockMapper, HostAudioClockMapping, decode_binary_audio,
};
use super::webcodecs::{
    WebCodecsStart, handle_webcodecs_binary, handle_webcodecs_start, handle_webcodecs_stop,
    is_webcodecs_video_frame,
};
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
    pub host_audio_clock: &'a mut HostAudioClockMapper,
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
        host_audio_clock,
        session_started_at,
        last_audio_timeline_shadow_log_at,
    } = args;
    if is_webcodecs_video_frame(&data) {
        handle_webcodecs_binary(&data, live_sessions, live_session_id, session_log);
        return;
    }
    let queue_was_initialized = audio_tx.is_some();
    let now = Instant::now();
    let decoded = decode_binary_audio(&data, timestamped_audio);
    let (pcm_data, timestamped_sample, audio_clock_mapping) = match decoded {
        BinaryAudioPayload::RawPcm(pcm) => (pcm.to_vec(), None, None),
        BinaryAudioPayload::TimestampedPcm(frame) => {
            let media_pts = frame.media_pts();
            let clock_mapping = host_audio_clock.map(media_pts, now);
            if let Some(reason) = clock_mapping.resync_reason {
                tracing::warn!(
                    live_session_id = %live_session_id,
                    audio_clock_source = "media_pts",
                    audio_media_pts_us = clock_mapping.media_pts.as_micros(),
                    audio_mapped_capture_age_ms = clock_mapping.mapped_capture_age.as_millis() as u64,
                    audio_clock_resync_count = clock_mapping.resync_count,
                    audio_clock_resync_reason = reason.as_str(),
                    "timestamped host audio clock resynced"
                );
            }
            let sample = TimestampedAudioTimelineShadowSample::from_bridge(
                frame.pcm.len(),
                frame.sample_index,
                frame.sample_rate,
                frame.client_capture_time_us,
                media_pts,
            );
            (frame.pcm.to_vec(), Some(sample), Some(clock_mapping))
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
            .map(|sample| {
                let mut payload = sample.to_log_payload();
                if let Some(mapping) = audio_clock_mapping {
                    enrich_audio_clock_payload(&mut payload, mapping);
                }
                payload
            })
            .unwrap_or_else(|| {
                let mut payload = AudioTimelineShadowSample::from_arrival(
                    pcm_data.len(),
                    queue_was_initialized,
                    session_started_at,
                    now,
                )
                .to_log_payload();
                payload["audio_clock_source"] = serde_json::json!("arrival");
                payload
            });
        tracing::info!(
            live_session_id = %live_session_id,
            audio_clock_source = payload.get("audio_clock_source").and_then(|v| v.as_str()).unwrap_or("unknown"),
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
        let rtmp_capture_at = audio_clock_mapping.map(|mapping| mapping.captured_at);
        tokio::spawn(async move {
            let mgr = mgr.lock().await;
            if let Some(captured_at) = rtmp_capture_at {
                mgr.push_host_audio_at(&pcm_data, captured_at);
            } else {
                mgr.push_host_audio(&pcm_data);
            }
        });
    }
}

fn enrich_audio_clock_payload(payload: &mut serde_json::Value, mapping: HostAudioClockMapping) {
    payload["audio_clock_source"] = serde_json::json!("media_pts");
    payload["audio_media_pts_us"] = serde_json::json!(mapping.media_pts.as_micros());
    payload["audio_mapped_capture_age_ms"] =
        serde_json::json!(mapping.mapped_capture_age.as_millis() as u64);
    payload["audio_clock_resync_count"] = serde_json::json!(mapping.resync_count);
    payload["audio_clock_resync_reason"] = mapping
        .resync_reason
        .map(|reason| serde_json::json!(reason.as_str()))
        .unwrap_or(serde_json::Value::Null);
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
    webcodecs_ingest_enabled: bool,
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
        Some("video:webcodecs_start") => match serde_json::from_value::<WebCodecsStart>(json) {
            Ok(start) => {
                handle_webcodecs_start(
                    start,
                    live_sessions,
                    live_session_id,
                    session_log,
                    webcodecs_ingest_enabled,
                )
                .await
            }
            Err(error) => tracing::warn!(
                live_session_id = %live_session_id,
                error = %error,
                "invalid webcodecs start"
            ),
        },
        Some("video:webcodecs_stop") => {
            handle_webcodecs_stop(live_sessions, live_session_id, session_log).await;
        }
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
    fn webcodecs_binary_is_routed_before_audio_decoder() {
        let live_sessions: LiveSessions = Arc::new(dashmap::DashMap::new());
        let mut audio_tx = None;
        let mut host_audio_clock = HostAudioClockMapper::default();
        let mut last_audio_timeline_shadow_log_at = None;
        let session_log = SessionLogEmitter::new(
            false,
            false,
            Arc::new(crate::features::broadcast::data::workers_api::WorkersApi::new("", "")),
            None,
            "ROOM".into(),
        );
        let mut data = vec![0u8; 56];
        data[0..4].copy_from_slice(b"BTV1");
        data[4] = 1;
        data[5] = 1;
        data[6] = 1;
        data[48..52].copy_from_slice(&4u32.to_le_bytes());
        data[52..56].copy_from_slice(&[0, 0x9d, 0x01, 0x2a]);

        handle_binary(BinaryArgs {
            data,
            live_sessions: &live_sessions,
            live_session_id: "ROOM",
            source_lang: &Lang::En,
            audio_tx: &mut audio_tx,
            session_log: &session_log,
            timeline_shadow: false,
            timestamped_audio: false,
            host_audio_clock: &mut host_audio_clock,
            session_started_at: Instant::now(),
            last_audio_timeline_shadow_log_at: &mut last_audio_timeline_shadow_log_at,
        });

        assert!(audio_tx.is_none());
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
