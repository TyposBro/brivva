use std::time::Instant;

use axum::extract::ws::Message;
use serde::Deserialize;

use crate::features::broadcast::data::ffmpeg::VideoInputCodec;
use crate::features::broadcast::data::session_log::SessionLogEmitter;
use crate::features::broadcast::domain::{
    EncodedVideoCodec, LiveSessions, VideoIngestKind, WebCodecsVideoIngestState,
};

const BTV1_HEADER_LEN: usize = 52;
const BTV1_MAGIC: &[u8; 4] = b"BTV1";
const BTV1_VERSION: u8 = 1;
const BTV1_CODEC_VP8: u8 = 1;
const BTV1_CODEC_H264_ANNEXB: u8 = 2;
const BTV1_FLAG_KEYFRAME: u8 = 0b0000_0001;
const BTV1_FLAG_CONFIG: u8 = 0b0000_0010;

#[derive(Debug, Deserialize)]
pub(super) struct WebCodecsStart {
    pub mode: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
    pub keyframe_interval_ms: u32,
    pub timebase_us: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WebCodecsWireCodec {
    Vp8,
    H264AnnexB,
}

impl WebCodecsWireCodec {
    fn from_id(id: u8) -> Option<Self> {
        match id {
            BTV1_CODEC_VP8 => Some(Self::Vp8),
            BTV1_CODEC_H264_ANNEXB => Some(Self::H264AnnexB),
            _ => None,
        }
    }

    fn encoded_codec(self) -> EncodedVideoCodec {
        match self {
            Self::Vp8 => EncodedVideoCodec::Vp8Ivf,
            Self::H264AnnexB => EncodedVideoCodec::H264AnnexB,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct WebCodecsVideoFrame {
    pub codec: WebCodecsWireCodec,
    pub key_frame: bool,
    pub config_frame: bool,
    pub sequence: u64,
    pub capture_time_us: u64,
    pub duration_us: Option<u64>,
    pub client_sent_time_us: u64,
    pub width: u32,
    pub height: u32,
    pub payload: Vec<u8>,
}

pub(super) fn server_capabilities_message(webcodecs_enabled: bool) -> Message {
    Message::Text(
        serde_json::json!({
            "type": "server:capabilities",
            "videoIngestModes": if webcodecs_enabled {
                vec!["webrtc", "webcodecs_ws"]
            } else {
                vec!["webrtc"]
            },
            "webcodecsCodecs": if webcodecs_enabled {
                vec!["vp8"]
            } else {
                Vec::<&str>::new()
            },
        })
        .to_string()
        .into(),
    )
}

pub(super) fn is_webcodecs_video_frame(data: &[u8]) -> bool {
    data.starts_with(BTV1_MAGIC)
}

pub(super) fn parse_webcodecs_video_frame(data: &[u8]) -> Result<WebCodecsVideoFrame, String> {
    if data.len() < BTV1_HEADER_LEN {
        return Err("BTV1 frame too short".into());
    }
    if &data[0..4] != BTV1_MAGIC {
        return Err("bad BTV1 magic".into());
    }
    if data[4] != BTV1_VERSION {
        return Err(format!("unsupported BTV1 version {}", data[4]));
    }
    let codec = WebCodecsWireCodec::from_id(data[5])
        .ok_or_else(|| format!("unsupported BTV1 codec {}", data[5]))?;
    let flags = data[6];
    let sequence = u64_at(data, 8);
    let capture_time_us = u64_at(data, 16);
    let duration_us = match u64_at(data, 24) {
        0 => None,
        value => Some(value),
    };
    let client_sent_time_us = u64_at(data, 32);
    let width = u32_at(data, 40);
    let height = u32_at(data, 44);
    let payload_len = u32_at(data, 48) as usize;
    let actual_payload_len = data.len().saturating_sub(BTV1_HEADER_LEN);
    if payload_len != actual_payload_len {
        return Err(format!(
            "BTV1 payload length mismatch declared={payload_len} actual={actual_payload_len}"
        ));
    }
    if payload_len == 0 {
        return Err("BTV1 payload empty".into());
    }
    Ok(WebCodecsVideoFrame {
        codec,
        key_frame: flags & BTV1_FLAG_KEYFRAME != 0,
        config_frame: flags & BTV1_FLAG_CONFIG != 0,
        sequence,
        capture_time_us,
        duration_us,
        client_sent_time_us,
        width,
        height,
        payload: data[BTV1_HEADER_LEN..].to_vec(),
    })
}

pub(super) async fn handle_webcodecs_start(
    start: WebCodecsStart,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    session_log: &SessionLogEmitter,
    webcodecs_ingest_enabled: bool,
) {
    if !webcodecs_ingest_enabled {
        reject_start(
            live_sessions,
            live_session_id,
            "VP8 WebCodecs ingest is disabled on this server",
        );
        return;
    }
    if start.mode != "webcodecs_ws" || start.codec != "vp8" || start.timebase_us != 1 {
        reject_start(
            live_sessions,
            live_session_id,
            "Only VP8 WebCodecs over WebSocket with 1us timebase is supported",
        );
        return;
    }
    let width = start.width.clamp(160, 3840);
    let height = start.height.clamp(90, 2160);
    let fps = start.fps.clamp(10, 120);
    let bitrate_bps = start.bitrate_bps.clamp(100_000, 20_000_000);
    let keyframe_interval_ms = start.keyframe_interval_ms.clamp(250, 10_000);

    let mut reject_message: Option<&'static str> = None;
    let manager = match live_sessions.get_mut(live_session_id) {
        Some(mut session) => match session.video_ingest_kind {
            VideoIngestKind::None => {
                session.video_ingest_kind = VideoIngestKind::WebCodecsWs;
                session.webcodecs_video = Some(WebCodecsVideoIngestState::new(
                    EncodedVideoCodec::Vp8Ivf,
                    width,
                    height,
                    fps,
                    bitrate_bps,
                    keyframe_interval_ms,
                ));
                session.rtmp_manager.clone()
            }
            VideoIngestKind::WebRtc => {
                reject_message =
                    Some("WebRTC video ingest is already active for this live session");
                None
            }
            VideoIngestKind::WebCodecsWs => {
                reject_message =
                    Some("WebCodecs video ingest is already active for this live session");
                None
            }
        },
        None => {
            reject_message = Some("live session not found");
            None
        }
    };
    if let Some(message) = reject_message {
        reject_start(live_sessions, live_session_id, message);
        return;
    }

    if let Some(manager) = manager {
        manager
            .lock()
            .await
            .switch_video_input_codec(VideoInputCodec::Vp8Ivf);
    }
    if let Some(session) = live_sessions.get(live_session_id) {
        session.send_to_host(Message::Text(
            serde_json::json!({
                "type": "video:webcodecs_ready",
                "accepted": true,
                "codec": "vp8",
                "message": "WebCodecs video ingest ready",
            })
            .to_string()
            .into(),
        ));
    }
    session_log.info(
        "server.video_ingest_selected",
        serde_json::json!({ "mode": "webcodecs_ws", "codec": "vp8" }),
    );
    session_log.info(
        "server.webcodecs_video_start",
        serde_json::json!({
            "codec": "vp8",
            "width": width,
            "height": height,
            "fps": fps,
            "bitrate_bps": bitrate_bps,
            "keyframe_interval_ms": keyframe_interval_ms,
        }),
    );
    tracing::info!(
        live_session_id = %live_session_id,
        codec = "vp8",
        width,
        height,
        fps,
        bitrate_bps,
        keyframe_interval_ms,
        "server.webcodecs_video_start"
    );
}

pub(super) async fn handle_webcodecs_stop(
    live_sessions: &LiveSessions,
    live_session_id: &str,
    session_log: &SessionLogEmitter,
) {
    let stats = live_sessions
        .get_mut(live_session_id)
        .and_then(|mut session| {
            if session.video_ingest_kind != VideoIngestKind::WebCodecsWs {
                return None;
            }
            session.video_ingest_kind = VideoIngestKind::None;
            session.webcodecs_video.take()
        });
    if let Some(stats) = stats {
        let elapsed_ms = stats.started_at.elapsed().as_millis() as u64;
        session_log.info(
            "server.video_ingest_stopped",
            serde_json::json!({
                "mode": "webcodecs_ws",
                "frames_received": stats.frames_received,
                "bytes_received": stats.bytes_received,
                "drops": stats.drops,
                "elapsed_ms": elapsed_ms,
            }),
        );
        tracing::info!(
            live_session_id = %live_session_id,
            mode = "webcodecs_ws",
            frames_received = stats.frames_received,
            bytes_received = stats.bytes_received,
            drops = stats.drops,
            elapsed_ms,
            "server.video_ingest_stopped"
        );
    }
}

pub(super) fn handle_webcodecs_binary(
    data: &[u8],
    live_sessions: &LiveSessions,
    live_session_id: &str,
    session_log: &SessionLogEmitter,
) {
    let frame = match parse_webcodecs_video_frame(data) {
        Ok(frame) => frame,
        Err(error) => {
            tracing::warn!(live_session_id = %live_session_id, error = %error, "rejected malformed webcodecs video frame");
            session_log.warn(
                "server.webcodecs_video_rejected",
                error.clone(),
                serde_json::json!({ "error": error }),
            );
            return;
        }
    };

    let mut push: Option<(
        crate::features::broadcast::data::ffmpeg::SharedRtmpManager,
        Vec<u8>,
        Instant,
    )> = None;
    let mut stats_payload: Option<serde_json::Value> = None;

    if let Some(mut session) = live_sessions.get_mut(live_session_id) {
        if session.video_ingest_kind != VideoIngestKind::WebCodecsWs {
            send_error(
                &session,
                "WebCodecs frame received before WebCodecs ingest was started",
            );
            return;
        }
        let manager = session.rtmp_manager.clone();
        let Some(state) = session.webcodecs_video.as_mut() else {
            send_error(&session, "WebCodecs ingest state is missing");
            return;
        };
        if state.codec != frame.codec.encoded_codec() {
            state.drops = state.drops.saturating_add(1);
            tracing::warn!(
                live_session_id = %live_session_id,
                expected = ?state.codec,
                got = ?frame.codec,
                "server.webcodecs_video_codec_mismatch"
            );
            return;
        }
        if state
            .next_sequence
            .is_some_and(|expected| expected != frame.sequence)
        {
            let expected = state.next_sequence.unwrap_or(frame.sequence);
            let gap = frame.sequence.saturating_sub(expected);
            state.drops = state.drops.saturating_add(gap.max(1));
            session_log.warn(
                "server.webcodecs_video_frame_gap",
                format!("expected sequence {expected}, got {}", frame.sequence),
                serde_json::json!({ "expected": expected, "got": frame.sequence, "gap": gap }),
            );
            tracing::warn!(
                live_session_id = %live_session_id,
                expected,
                got = frame.sequence,
                gap,
                "server.webcodecs_video_frame_gap"
            );
        }
        state.next_sequence = Some(frame.sequence.saturating_add(1));
        if !state.first_keyframe_seen && !frame.key_frame {
            state.drops = state.drops.saturating_add(1);
            tracing::debug!(
                live_session_id = %live_session_id,
                sequence = frame.sequence,
                "server.webcodecs_video_waiting_for_keyframe"
            );
            return;
        }
        if frame.key_frame {
            state.first_keyframe_seen = true;
            if !state.first_keyframe_logged {
                state.first_keyframe_logged = true;
                session_log.info(
                    "server.webcodecs_video_first_keyframe",
                    serde_json::json!({ "sequence": frame.sequence }),
                );
                tracing::info!(
                    live_session_id = %live_session_id,
                    sequence = frame.sequence,
                    "server.webcodecs_video_first_keyframe"
                );
            }
        }
        let captured_at = state.captured_at(frame.capture_time_us);
        let media_pts_us = state
            .base_capture_time_us
            .map(|base| frame.capture_time_us.saturating_sub(base))
            .unwrap_or(0);
        state.last_media_pts_us = Some(media_pts_us);
        state.frames_received = state.frames_received.saturating_add(1);
        state.bytes_received = state
            .bytes_received
            .saturating_add(frame.payload.len() as u64);
        let ivf = wrap_vp8_ivf_frame(state, &frame.payload);
        let drift_ms = frame
            .client_sent_time_us
            .checked_sub(frame.capture_time_us)
            .map(|delta| delta / 1000);
        if state.frames_received == 1 || state.frames_received % 60 == 0 {
            stats_payload = Some(serde_json::json!({
                "frames_received": state.frames_received,
                "bytes_received": state.bytes_received,
                "latest_media_pts_us": state.last_media_pts_us.unwrap_or(0),
                "drops": state.drops,
                "client_capture_to_send_ms": drift_ms,
            }));
        }
        if let Some(manager) = manager {
            push = Some((manager, ivf, captured_at));
        }
    } else {
        tracing::warn!(live_session_id = %live_session_id, "webcodecs frame for missing live session");
        return;
    }

    if let Some(payload) = stats_payload {
        if let Some(session) = live_sessions.get(live_session_id) {
            session.send_to_host(Message::Text(
                serde_json::json!({
                    "type": "video:webcodecs_stats",
                    "frames_received": payload["frames_received"],
                    "bytes_received": payload["bytes_received"],
                    "latest_media_pts_us": payload["latest_media_pts_us"],
                    "drops": payload["drops"],
                })
                .to_string()
                .into(),
            ));
        }
        session_log.debug("server.webcodecs_stats", payload);
    }

    if let Some((manager, ivf, captured_at)) = push {
        tokio::spawn(async move {
            manager
                .lock()
                .await
                .push_video_vp8_ivf_frame_at(&ivf, captured_at);
        });
    }
}

fn reject_start(live_sessions: &LiveSessions, live_session_id: &str, message: &str) {
    if let Some(session) = live_sessions.get(live_session_id) {
        send_error(&session, message);
    }
    tracing::warn!(live_session_id = %live_session_id, message, "server.webcodecs_video_start_rejected");
}

fn send_error(session: &crate::features::broadcast::domain::LiveSession, message: &str) {
    session.send_to_host(Message::Text(
        serde_json::json!({
            "type": "video:webcodecs_error",
            "message": message,
        })
        .to_string()
        .into(),
    ));
}

fn wrap_vp8_ivf_frame(state: &mut WebCodecsVideoIngestState, raw_frame: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw_frame.len() + 44);
    if !state.wrote_ivf_header {
        out.extend_from_slice(&ivf_stream_header(state.width, state.height, state.fps));
        state.wrote_ivf_header = true;
    }
    out.extend_from_slice(&ivf_frame_header(
        raw_frame.len() as u32,
        state.ivf_frame_index,
    ));
    out.extend_from_slice(raw_frame);
    state.ivf_frame_index = state.ivf_frame_index.saturating_add(1);
    out
}

fn ivf_stream_header(width: u32, height: u32, fps: u32) -> [u8; 32] {
    let mut header = [0u8; 32];
    header[0..4].copy_from_slice(b"DKIF");
    header[4..6].copy_from_slice(&0u16.to_le_bytes());
    header[6..8].copy_from_slice(&32u16.to_le_bytes());
    header[8..12].copy_from_slice(b"VP80");
    header[12..14].copy_from_slice(&(width.min(u16::MAX as u32) as u16).to_le_bytes());
    header[14..16].copy_from_slice(&(height.min(u16::MAX as u32) as u16).to_le_bytes());
    header[16..20].copy_from_slice(&fps.max(1).to_le_bytes());
    header[20..24].copy_from_slice(&1u32.to_le_bytes());
    header
}

fn ivf_frame_header(frame_len: u32, pts: u64) -> [u8; 12] {
    let mut header = [0u8; 12];
    header[0..4].copy_from_slice(&frame_len.to_le_bytes());
    header[4..12].copy_from_slice(&pts.to_le_bytes());
    header
}

fn u64_at(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        data[offset..offset + 8]
            .try_into()
            .expect("slice len checked"),
    )
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        data[offset..offset + 4]
            .try_into()
            .expect("slice len checked"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::domain::{Lang, LiveSession, PipelineConfig};
    use dashmap::DashMap;
    use std::sync::Arc;

    fn frame_bytes(overrides: impl FnOnce(&mut [u8])) -> Vec<u8> {
        let payload = [0x00, 0x9d, 0x01, 0x2a];
        let mut data = vec![0u8; BTV1_HEADER_LEN + payload.len()];
        data[0..4].copy_from_slice(BTV1_MAGIC);
        data[4] = BTV1_VERSION;
        data[5] = BTV1_CODEC_VP8;
        data[6] = BTV1_FLAG_KEYFRAME;
        data[8..16].copy_from_slice(&7u64.to_le_bytes());
        data[16..24].copy_from_slice(&12_345u64.to_le_bytes());
        data[24..32].copy_from_slice(&33_333u64.to_le_bytes());
        data[32..40].copy_from_slice(&12_500u64.to_le_bytes());
        data[40..44].copy_from_slice(&720u32.to_le_bytes());
        data[44..48].copy_from_slice(&1280u32.to_le_bytes());
        data[48..52].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        data[BTV1_HEADER_LEN..].copy_from_slice(&payload);
        overrides(&mut data);
        data
    }

    #[test]
    fn parser_accepts_valid_btv1_frame() {
        let parsed = parse_webcodecs_video_frame(&frame_bytes(|_| {})).unwrap();
        assert_eq!(parsed.codec, WebCodecsWireCodec::Vp8);
        assert!(parsed.key_frame);
        assert_eq!(parsed.sequence, 7);
        assert_eq!(parsed.capture_time_us, 12_345);
        assert_eq!(parsed.duration_us, Some(33_333));
        assert_eq!(parsed.width, 720);
        assert_eq!(parsed.height, 1280);
        assert_eq!(parsed.payload, vec![0x00, 0x9d, 0x01, 0x2a]);
    }

    #[test]
    fn parser_rejects_bad_magic_version_and_length() {
        assert!(parse_webcodecs_video_frame(b"NOPE").is_err());
        let mut bad_version = frame_bytes(|_| {});
        bad_version[4] = 2;
        assert!(
            parse_webcodecs_video_frame(&bad_version)
                .unwrap_err()
                .contains("version")
        );
        let mut bad_len = frame_bytes(|_| {});
        bad_len[48..52].copy_from_slice(&99u32.to_le_bytes());
        assert!(
            parse_webcodecs_video_frame(&bad_len)
                .unwrap_err()
                .contains("length")
        );
    }

    #[test]
    fn vp8_ivf_wrapper_writes_stream_header_once_then_frame_headers() {
        let mut state = WebCodecsVideoIngestState::new(
            EncodedVideoCodec::Vp8Ivf,
            720,
            1280,
            30,
            2_800_000,
            1000,
        );
        let first = wrap_vp8_ivf_frame(&mut state, &[1, 2, 3]);
        assert_eq!(&first[0..4], b"DKIF");
        assert_eq!(&first[8..12], b"VP80");
        assert_eq!(u16::from_le_bytes(first[12..14].try_into().unwrap()), 720);
        assert_eq!(u16::from_le_bytes(first[14..16].try_into().unwrap()), 1280);
        assert_eq!(u32::from_le_bytes(first[32..36].try_into().unwrap()), 3);
        assert_eq!(&first[44..], &[1, 2, 3]);

        let second = wrap_vp8_ivf_frame(&mut state, &[4, 5]);
        assert_eq!(u32::from_le_bytes(second[0..4].try_into().unwrap()), 2);
        assert_eq!(u64::from_le_bytes(second[4..12].try_into().unwrap()), 1);
        assert_eq!(&second[12..], &[4, 5]);
    }

    #[tokio::test]
    async fn start_rejects_when_webrtc_already_active() {
        let sessions: LiveSessions = Arc::new(DashMap::new());
        let mut session = LiveSession::new(
            "ROOM".into(),
            Lang::En,
            None,
            Arc::new(PipelineConfig::default()),
        );
        session.video_ingest_kind = VideoIngestKind::WebRtc;
        sessions.insert("ROOM".into(), session);
        let emitter = SessionLogEmitter::new(
            false,
            false,
            Arc::new(crate::features::broadcast::data::workers_api::WorkersApi::new("", "")),
            None,
            "ROOM".into(),
        );

        handle_webcodecs_start(
            WebCodecsStart {
                mode: "webcodecs_ws".into(),
                codec: "vp8".into(),
                width: 720,
                height: 1280,
                fps: 30,
                bitrate_bps: 2_800_000,
                keyframe_interval_ms: 1000,
                timebase_us: 1,
            },
            &sessions,
            "ROOM",
            &emitter,
            true,
        )
        .await;

        assert_eq!(
            sessions.get("ROOM").unwrap().video_ingest_kind,
            VideoIngestKind::WebRtc
        );
    }
}
