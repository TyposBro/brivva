use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::Message;
use serde::Deserialize;
use ice::udp_network::{EphemeralUDP, UDPNetwork};
use webrtc::api::APIBuilder;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::{MIME_TYPE_H264, MediaEngine};
use webrtc::error::Result as WebRtcResult;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp::codecs::h264::{H264Packet, NALU_TYPE_BITMASK, SPS_NALU_TYPE, STAPA_NALU_TYPE};
use webrtc::rtp::packet::Packet;
use webrtc::rtp::packetizer::Depacketizer;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;
use webrtc::track::track_remote::TrackRemote;

use crate::features::broadcast::domain::LiveSessions;

#[derive(Debug, Deserialize)]
pub(super) struct WebRtcOffer {
    pub sdp: String,
}

pub(super) async fn handle_webrtc_offer(
    offer: WebRtcOffer,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    match accept_webrtc_video(offer, live_sessions, live_session_id).await {
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
) -> WebRtcResult<Arc<RTCPeerConnection>> {
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
) {
    peer.on_track(Box::new(move |track, _, _| {
        let live_sessions = live_sessions.clone();
        let live_session_id = live_session_id.clone();
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
            forward_track_rtp(track, live_sessions, live_session_id).await;
        })
    }));
}

async fn forward_track_rtp(
    track: Arc<TrackRemote>,
    live_sessions: LiveSessions,
    live_session_id: String,
) {
    tracing::info!(live_session_id = %live_session_id, "webrtc H.264 video track started");
    let mut depacketizer = H264AnnexBDepacketizer::default();
    let mut clock = VideoRtpClock::default();
    while let Ok((packet, _)) = track.read_rtp().await {
        let Some(annex_b) = depacketizer.depacketize(&packet) else {
            continue;
        };
        let captured_at = clock.instant_for(packet.header.timestamp);
        let manager = live_sessions
            .get(&live_session_id)
            .and_then(|session| session.rtmp_manager.clone());
        let Some(manager) = manager else {
            break;
        };
        manager.lock().await.push_video_h264_at(&annex_b, captured_at);
    }
    tracing::info!(live_session_id = %live_session_id, "webrtc video track ended");
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
}

impl H264AnnexBDepacketizer {
    fn depacketize(&mut self, packet: &Packet) -> Option<Vec<u8>> {
        if packet.payload.is_empty() {
            return None;
        }
        if !self.has_parameter_set {
            self.has_parameter_set = payload_has_sps(&packet.payload);
            if !self.has_parameter_set {
                return None;
            }
        }
        match self.inner.depacketize(&packet.payload) {
            Ok(bytes) if !bytes.is_empty() => Some(bytes.to_vec()),
            Ok(_) => None,
            Err(error) => {
                tracing::debug!(error = %error, "webrtc H.264 depacketize failed");
                None
            }
        }
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
    fn depacketizer_waits_for_parameter_set_then_outputs_annex_b() {
        let mut depacketizer = H264AnnexBDepacketizer::default();
        let idr = Packet {
            header: Header::default(),
            payload: vec![0x65, 1, 2, 3].into(),
        };
        assert!(depacketizer.depacketize(&idr).is_none());

        let sps = Packet {
            header: Header::default(),
            payload: vec![0x67, 1, 2, 3].into(),
        };
        let out = depacketizer.depacketize(&sps).expect("sps output");
        assert!(out.starts_with(&[0, 0, 0, 1, 0x67]));
    }
}
