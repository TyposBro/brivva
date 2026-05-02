use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::Message;
use ice::udp_network::{EphemeralUDP, UDPNetwork};
use rtcp::payload_feedbacks::picture_loss_indication::PictureLossIndication;
use serde::Deserialize;
use webrtc::api::APIBuilder;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MIME_TYPE_H264, MediaEngine};
use webrtc::api::setting_engine::SettingEngine;
use webrtc::error::Result as WebRtcResult;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp::codecs::h264::{
    FU_END_BITMASK, FU_START_BITMASK, FUA_NALU_TYPE, H264Packet, NALU_TYPE_BITMASK, PPS_NALU_TYPE,
    SPS_NALU_TYPE, STAPA_NALU_TYPE,
};
use webrtc::rtp::packet::Packet;
use webrtc::rtp::packetizer::Depacketizer;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
use webrtc::track::track_remote::TrackRemote;

use crate::features::broadcast::domain::LiveSessions;

use super::timeline_shadow::{VideoTimelineShadow, timeline_shadow_enabled};

#[derive(Debug, Deserialize)]
pub(super) struct WebRtcOffer {
    pub sdp: String,
    #[serde(rename = "videoProfile")]
    pub video_profile: Option<ClientVideoProfile>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ClientVideoProfile {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<u32>,
}

pub(super) async fn handle_webrtc_offer(
    offer: WebRtcOffer,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    timeline_shadow: bool,
) {
    match accept_webrtc_video(offer, live_sessions, live_session_id, timeline_shadow).await {
        Ok(peer) => {
            if let Some(mut session) = live_sessions.get_mut(live_session_id) {
                session.webrtc_peer = Some(peer);
            }
        }
        Err(error) => {
            tracing::warn!(
                live_session_id = %live_session_id,
                error = %error,
                "webrtc offer rejected"
            );
        }
    }
}

async fn accept_webrtc_video(
    offer: WebRtcOffer,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    timeline_shadow: bool,
) -> WebRtcResult<Arc<RTCPeerConnection>> {
    apply_video_profile(&offer, live_sessions, live_session_id).await;
    let mut media = MediaEngine::default();
    media.register_default_codecs()?;
    let registry = register_default_interceptors(Registry::new(), &mut media)?;
    let mut settings = SettingEngine::default();
    settings.set_udp_network(UDPNetwork::Ephemeral(EphemeralUDP::new(
        webrtc_udp_port_min(),
        webrtc_udp_port_max(),
    )?));
    let api = APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .with_setting_engine(settings)
        .build();
    let peer = Arc::new(api.new_peer_connection(webrtc_config()).await?);

    wire_video_track(
        peer.clone(),
        live_sessions.clone(),
        live_session_id.to_string(),
        timeline_shadow,
    );

    let remote = RTCSessionDescription::offer(offer.sdp)?;
    peer.set_remote_description(remote).await?;
    let answer = peer.create_answer(None).await?;
    let mut gathering_complete = peer.gathering_complete_promise().await;
    peer.set_local_description(answer).await?;
    let _ = gathering_complete.recv().await;

    if let Some(desc) = peer.local_description().await
        && let Some(session) = live_sessions.get(live_session_id)
    {
        session.send_to_host(Message::Text(
            serde_json::json!({
                "type": "webrtc:answer",
                "sdp": desc.sdp,
            })
            .to_string()
            .into(),
        ));
    }

    Ok(peer)
}

async fn apply_video_profile(
    offer: &WebRtcOffer,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    let Some(client) = &offer.video_profile else {
        return;
    };
    let width = client.width.unwrap_or(1920).clamp(320, 3840);
    let height = client.height.unwrap_or(1080).clamp(180, 2160);
    let fps = client.fps.unwrap_or(30).clamp(10, 120);
    let manager = live_sessions
        .get(live_session_id)
        .and_then(|session| session.rtmp_manager.clone());
    let profile = if let Some(manager) = manager {
        manager
            .lock()
            .await
            .set_capture_video_profile(width, height, fps)
    } else {
        crate::features::broadcast::data::ffmpeg::VideoProfile::from_capture(width, height, fps)
    };
    tracing::info!(
        live_session_id = %live_session_id,
        capture_width = width,
        capture_height = height,
        capture_fps = fps,
        input_fps = profile.input_fps,
        output_fps = profile.output_fps,
        output_width = profile.max_width,
        output_height = profile.max_height,
        "webrtc video profile applied"
    );
}

fn webrtc_config() -> RTCConfiguration {
    RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn webrtc_udp_port_min() -> u16 {
    read_port_env("BRIVVA_WEBRTC_UDP_PORT_MIN", 40_000)
}

fn webrtc_udp_port_max() -> u16 {
    read_port_env("BRIVVA_WEBRTC_UDP_PORT_MAX", 40_100)
}

fn read_port_env(name: &str, default: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(default)
}

fn wire_video_track(
    peer: Arc<RTCPeerConnection>,
    live_sessions: LiveSessions,
    live_session_id: String,
    timeline_shadow: bool,
) {
    let peer_for_track = peer.clone();
    peer.on_track(Box::new(move |track, _, _| {
        let peer = peer_for_track.clone();
        let live_sessions = live_sessions.clone();
        let live_session_id = live_session_id.clone();
        let timeline_shadow = timeline_shadow_enabled(timeline_shadow);
        Box::pin(async move {
            if track.kind() != RTPCodecType::Video {
                return;
            }
            let codec = track.codec();
            if codec.capability.mime_type != MIME_TYPE_H264 {
                tracing::warn!(
                    live_session_id = %live_session_id,
                    mime_type = %codec.capability.mime_type,
                    "webrtc video track ignored: RTMP copy path requires H.264"
                );
                return;
            }
            forward_track_rtp(
                track,
                peer.clone(),
                live_sessions,
                live_session_id,
                timeline_shadow,
            )
            .await;
        })
    }));
}

async fn forward_track_rtp(
    track: Arc<TrackRemote>,
    peer: Arc<RTCPeerConnection>,
    live_sessions: LiveSessions,
    live_session_id: String,
    timeline_shadow: bool,
) {
    tracing::info!(live_session_id = %live_session_id, "webrtc H.264 video track started");
    let pli_task = spawn_periodic_pli(peer, track.ssrc(), live_session_id.clone());
    let mut depacketizer = H264AnnexBDepacketizer::default();
    let mut clock = VideoRtpClock::default();
    let mut fps_estimator = VideoFpsEstimator::default();
    let mut timeline_shadow = timeline_shadow.then(VideoTimelineShadow::default);
    while let Ok((packet, _)) = track.read_rtp().await {
        let Some(annex_b) = depacketizer.depacketize(&packet) else {
            continue;
        };
        let captured_at = clock.instant_for(packet.header.timestamp);
        if let Some(shadow) = timeline_shadow.as_mut() {
            let sample = shadow.observe_access_unit(
                packet.header.timestamp,
                packet.header.sequence_number,
                Instant::now(),
            );
            if sample.should_log {
                let payload = sample.to_log_payload();
                tracing::info!(
                    live_session_id = %live_session_id,
                    payload = %payload,
                    "v2 timeline shadow video"
                );
            }
        }
        let manager = live_sessions
            .get(&live_session_id)
            .and_then(|session| session.rtmp_manager.clone());
        let Some(manager) = manager else {
            break;
        };
        if let Some(fps) = fps_estimator.observe(packet.header.timestamp) {
            let profile = manager.lock().await.set_observed_video_fps(fps);
            tracing::info!(
                live_session_id = %live_session_id,
                observed_fps = fps,
                input_fps = profile.input_fps,
                output_fps = profile.output_fps,
                "webrtc video fps corrected from RTP timestamps"
            );
        }
        manager
            .lock()
            .await
            .push_video_h264_at(&annex_b, captured_at);
    }
    pli_task.abort();
    tracing::info!(live_session_id = %live_session_id, "webrtc video track ended");
}

fn spawn_periodic_pli(
    peer: Arc<RTCPeerConnection>,
    media_ssrc: u32,
    live_session_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            let packet = PictureLossIndication {
                sender_ssrc: 0,
                media_ssrc,
            };
            if let Err(error) = peer.write_rtcp(&[Box::new(packet)]).await {
                tracing::debug!(
                    live_session_id = %live_session_id,
                    error = %error,
                    "webrtc keyframe request failed"
                );
                break;
            }
            tracing::debug!(
                live_session_id = %live_session_id,
                media_ssrc,
                "webrtc keyframe requested"
            );
        }
    })
}

#[derive(Default)]
struct VideoFpsEstimator {
    first_rtp_ts: Option<u32>,
    last_rtp_ts: Option<u32>,
    access_units: u32,
    applied_fps: Option<u32>,
}

impl VideoFpsEstimator {
    const RTP_HZ: f64 = 90_000.0;
    const MIN_ACCESS_UNITS: u32 = 60;

    fn observe(&mut self, rtp_ts: u32) -> Option<u32> {
        if self.last_rtp_ts == Some(rtp_ts) {
            return None;
        }
        let first = *self.first_rtp_ts.get_or_insert(rtp_ts);
        self.last_rtp_ts = Some(rtp_ts);
        self.access_units = self.access_units.saturating_add(1);
        if self.access_units < Self::MIN_ACCESS_UNITS {
            return None;
        }
        let elapsed_ticks = rtp_ts.wrapping_sub(first);
        if elapsed_ticks == 0 {
            return None;
        }
        let fps = ((self.access_units.saturating_sub(1)) as f64 * Self::RTP_HZ
            / elapsed_ticks as f64)
            .round() as u32;
        let fps = fps.clamp(15, 120);
        if self
            .applied_fps
            .is_some_and(|applied| applied.abs_diff(fps) < 2)
        {
            return None;
        }
        self.applied_fps = Some(fps);
        Some(fps)
    }
}

#[derive(Default)]
struct VideoRtpClock {
    base_rtp_ts: Option<u32>,
    base_instant: Option<Instant>,
}

impl VideoRtpClock {
    const RTP_HZ: u64 = 90_000;

    fn instant_for(&mut self, rtp_ts: u32) -> Instant {
        let base_rtp_ts = *self.base_rtp_ts.get_or_insert(rtp_ts);
        let base_instant = *self.base_instant.get_or_insert_with(Instant::now);
        let delta_ticks = rtp_ts.wrapping_sub(base_rtp_ts) as u64;
        base_instant + Duration::from_micros(delta_ticks.saturating_mul(1_000_000) / Self::RTP_HZ)
    }
}

#[derive(Default)]
struct H264AnnexBDepacketizer {
    inner: H264Packet,
    has_parameter_set: bool,
    parameter_sets_annex_b: Vec<u8>,
    active_fu_a: bool,
    last_sequence_number: Option<u16>,
    access_unit: Vec<u8>,
}

impl H264AnnexBDepacketizer {
    fn depacketize(&mut self, packet: &Packet) -> Option<Vec<u8>> {
        if packet.payload.is_empty() {
            return None;
        }

        if let Some(last) = self.last_sequence_number
            && packet.header.sequence_number != last.wrapping_add(1)
        {
            self.reset_after_loss();
        }
        self.last_sequence_number = Some(packet.header.sequence_number);

        let nalu_type = packet.payload[0] & NALU_TYPE_BITMASK;
        if nalu_type == FUA_NALU_TYPE && !self.accept_fu_a_fragment(&packet.payload) {
            return None;
        }

        if !self.has_parameter_set {
            self.has_parameter_set = payload_has_sps(&packet.payload);
            if !self.has_parameter_set {
                return None;
            }
        }

        match self.inner.depacketize(&packet.payload) {
            Ok(bytes) if !bytes.is_empty() => self.push_depacketized(packet.header.marker, &bytes),
            Ok(_) => None,
            Err(error) => {
                tracing::debug!(error = %error, "webrtc H.264 depacketize failed");
                self.reset_after_loss();
                None
            }
        }
    }

    fn accept_fu_a_fragment(&mut self, payload: &[u8]) -> bool {
        if payload.len() < 2 {
            return false;
        }
        let is_start = (payload[1] & FU_START_BITMASK) != 0;
        let is_end = (payload[1] & FU_END_BITMASK) != 0;

        // The rtp crate buffers FU-A payload bytes even if the first packet we
        // see is a middle fragment. After packet loss/restart that produces a
        // syntactically Annex-B-looking NAL with corrupt slice data, which was
        // exactly what prod YouTube tests showed (`mb_type ... too large`,
        // `error while decoding MB ...`). Drop middle/end fragments until a
        // clean FU-A start arrives.
        if !is_start && !self.active_fu_a {
            return false;
        }
        if is_start {
            self.active_fu_a = true;
        }
        if is_end {
            self.active_fu_a = false;
        }
        true
    }

    fn push_depacketized(&mut self, marker: bool, bytes: &[u8]) -> Option<Vec<u8>> {
        self.access_unit.extend_from_slice(bytes);
        if marker && !self.access_unit.is_empty() {
            let mut access_unit = std::mem::take(&mut self.access_unit);
            if let Some(parameter_sets) = annex_b_parameter_sets(&access_unit)
                && !parameter_sets.is_empty()
            {
                self.parameter_sets_annex_b = parameter_sets;
            }
            if annex_b_contains_type(&access_unit, 5)
                && !annex_b_contains_type(&access_unit, SPS_NALU_TYPE)
                && !self.parameter_sets_annex_b.is_empty()
            {
                let mut prefixed =
                    Vec::with_capacity(self.parameter_sets_annex_b.len() + access_unit.len());
                prefixed.extend_from_slice(&self.parameter_sets_annex_b);
                prefixed.extend_from_slice(&access_unit);
                access_unit = prefixed;
            }
            return Some(access_unit);
        }
        None
    }

    fn reset_after_loss(&mut self) {
        self.inner = H264Packet::default();
        self.has_parameter_set = false;
        self.active_fu_a = false;
        self.access_unit.clear();
    }
}

fn payload_has_sps(payload: &[u8]) -> bool {
    if payload.is_empty() {
        return false;
    }
    match payload[0] & NALU_TYPE_BITMASK {
        SPS_NALU_TYPE => true,
        STAPA_NALU_TYPE => stap_a_has_sps(payload),
        _ => false,
    }
}

fn stap_a_has_sps(payload: &[u8]) -> bool {
    let mut offset = 1usize;
    while offset + 2 <= payload.len() {
        let nalu_len = u16::from_be_bytes([payload[offset], payload[offset + 1]]) as usize;
        offset += 2;
        if offset + nalu_len > payload.len() {
            return false;
        }
        if nalu_len > 0 && (payload[offset] & NALU_TYPE_BITMASK) == SPS_NALU_TYPE {
            return true;
        }
        offset += nalu_len;
    }
    false
}

fn annex_b_parameter_sets(access_unit: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (start, end, nalu_type) in annex_b_nalus(access_unit) {
        if nalu_type == SPS_NALU_TYPE || nalu_type == PPS_NALU_TYPE {
            out.extend_from_slice(&access_unit[start..end]);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn annex_b_contains_type(access_unit: &[u8], needle: u8) -> bool {
    annex_b_nalus(access_unit)
        .into_iter()
        .any(|(_, _, nalu_type)| nalu_type == needle)
}

fn annex_b_nalus(access_unit: &[u8]) -> Vec<(usize, usize, u8)> {
    let starts = annex_b_start_codes(access_unit);
    let mut nalus = Vec::new();
    for (idx, (start, prefix_len)) in starts.iter().copied().enumerate() {
        let nalu_idx = start + prefix_len;
        if nalu_idx >= access_unit.len() {
            continue;
        }
        let end = starts
            .get(idx + 1)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(access_unit.len());
        nalus.push((start, end, access_unit[nalu_idx] & NALU_TYPE_BITMASK));
    }
    nalus
}

fn annex_b_start_codes(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if bytes[i..].starts_with(&[0, 0, 0, 1]) {
            starts.push((i, 4));
            i += 4;
        } else if bytes[i..].starts_with(&[0, 0, 1]) {
            starts.push((i, 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;
    use webrtc::rtp::header::Header;

    #[test]
    fn payload_has_sps_detects_single_nalu_sps() {
        assert!(payload_has_sps(&[0x67, 1, 2, 3]));
        assert!(!payload_has_sps(&[0x65, 1, 2, 3]));
    }

    #[test]
    fn payload_has_sps_detects_stap_a_sps() {
        let payload = [
            24, // STAP-A
            0, 3, 0x67, 1, 2, 0, 2, 0x68, 3,
        ];
        assert!(payload_has_sps(&payload));
    }

    #[test]
    fn video_rtp_clock_maps_90khz_ticks_to_wall_clock() {
        let mut clock = VideoRtpClock::default();
        let first = clock.instant_for(10_000);
        let second = clock.instant_for(13_000);
        assert_eq!(second.duration_since(first), Duration::from_micros(33_333));
    }

    #[test]
    fn video_fps_estimator_derives_fps_from_rtp_timestamps() {
        let mut estimator = VideoFpsEstimator::default();
        let mut observed = None;
        for frame in 0..60 {
            observed = estimator.observe(10_000 + frame * 3_000).or(observed);
        }
        assert_eq!(observed, Some(30));
    }

    #[test]
    fn video_fps_estimator_ignores_duplicate_access_unit_timestamp() {
        let mut estimator = VideoFpsEstimator::default();
        let mut observed = None;
        for frame in 0..60 {
            let ts = 10_000 + frame * 1_500;
            observed = estimator.observe(ts).or(observed);
            observed = estimator.observe(ts).or(observed);
        }
        assert_eq!(observed, Some(60));
    }

    #[test]
    fn depacketizer_waits_for_parameter_set_then_outputs_annex_b_access_unit_on_marker() {
        let mut depacketizer = H264AnnexBDepacketizer::default();
        let idr = Packet {
            header: Header {
                marker: true,
                sequence_number: 1,
                ..Default::default()
            },
            payload: vec![0x65, 1, 2, 3].into(),
        };
        assert!(depacketizer.depacketize(&idr).is_none());

        let sps = Packet {
            header: Header {
                marker: false,
                sequence_number: 2,
                ..Default::default()
            },
            payload: vec![0x67, 1, 2, 3].into(),
        };
        assert!(depacketizer.depacketize(&sps).is_none());

        let idr_after_sps = Packet {
            header: Header {
                marker: true,
                sequence_number: 3,
                ..Default::default()
            },
            payload: vec![0x65, 4, 5, 6].into(),
        };
        let out = depacketizer
            .depacketize(&idr_after_sps)
            .expect("complete access unit");
        assert!(out.starts_with(&[0, 0, 0, 1, 0x67]));
        assert!(out.windows(5).any(|window| window == [0, 0, 0, 1, 0x65]));
    }

    #[test]
    fn depacketizer_prefixes_cached_parameter_sets_before_later_idr() {
        let mut depacketizer = H264AnnexBDepacketizer::default();
        let sps_pps_idr = Packet {
            header: Header {
                marker: true,
                sequence_number: 1,
                ..Default::default()
            },
            payload: vec![
                24, // STAP-A
                0, 3, 0x67, 1, 2, // SPS
                0, 3, 0x68, 3, 4, // PPS
                0, 3, 0x65, 5, 6, // IDR
            ]
            .into(),
        };
        let first = depacketizer
            .depacketize(&sps_pps_idr)
            .expect("initial access unit");
        assert!(first.windows(5).any(|window| window == [0, 0, 0, 1, 0x68]));

        let later_idr = Packet {
            header: Header {
                marker: true,
                sequence_number: 2,
                ..Default::default()
            },
            payload: vec![0x65, 7, 8, 9].into(),
        };
        let second = depacketizer
            .depacketize(&later_idr)
            .expect("later access unit");

        assert!(second.starts_with(&[0, 0, 0, 1, 0x67]));
        assert!(second.windows(5).any(|window| window == [0, 0, 0, 1, 0x68]));
        assert!(second.windows(5).any(|window| window == [0, 0, 0, 1, 0x65]));
    }

    #[test]
    fn depacketizer_drops_fu_a_middle_fragment_after_loss() {
        let mut depacketizer = H264AnnexBDepacketizer::default();
        let sps = Packet {
            header: Header {
                sequence_number: 1,
                marker: true,
                ..Default::default()
            },
            payload: vec![0x67, 1, 2, 3].into(),
        };
        assert!(depacketizer.depacketize(&sps).is_some());

        let mid_fu_a = Packet {
            header: Header {
                sequence_number: 10,
                marker: false,
                ..Default::default()
            },
            payload: vec![0x7c, 0x01, 9, 9, 9].into(),
        };
        assert!(depacketizer.depacketize(&mid_fu_a).is_none());
    }
}
