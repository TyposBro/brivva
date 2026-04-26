use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes as MediaBytes;
use ice::udp_network::{EphemeralUDP, UDPNetwork};
use rtp::codecs::h264::H264Packet;
use rtp::packetizer::Depacketizer;
use std::time::Duration;
use webrtc::api::APIBuilder;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::setting_engine::SettingEngine;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::RTPCodecType;

use crate::features::broadcast::data::{auth, state::BroadcastState};
use crate::features::broadcast::domain::SessionQuery;

const NALU_TYPE_BITMASK: u8 = 0x1f;
const STAP_A_NALU_TYPE: u8 = 24;
const SPS_NALU_TYPE: u8 = 7;
const STAP_A_NALU_LENGTH_SIZE: usize = 2;

pub async fn whip_session_handler(
    Query(query): Query<SessionQuery>,
    State(state): State<BroadcastState>,
    body: Bytes,
) -> Response {
    if authenticate(&state, &query).is_none() {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let Some(session_id) = query.session_id.as_deref() else {
        return (StatusCode::BAD_REQUEST, "sessionId required").into_response();
    };
    let Some(live_session_id) = find_live_session_id(&state, session_id) else {
        return (StatusCode::NOT_FOUND, "live session not found").into_response();
    };

    match accept_whip_offer(state, live_session_id, body).await {
        Ok(answer_sdp) => sdp_response(answer_sdp),
        Err(error) => {
            tracing::warn!(error = %error, "whip session setup failed");
            (StatusCode::BAD_REQUEST, error).into_response()
        }
    }
}

fn authenticate(state: &BroadcastState, query: &SessionQuery) -> Option<auth::Claims> {
    let token = query.token.as_deref().filter(|t| !t.is_empty())?;
    auth::verify(token, &state.jwt_secret).ok()
}

fn find_live_session_id(state: &BroadcastState, session_id: &str) -> Option<String> {
    state
        .live_sessions
        .iter()
        .find(|entry| entry.session_id.as_deref() == Some(session_id))
        .map(|entry| entry.id.clone())
}

async fn accept_whip_offer(
    state: BroadcastState,
    live_session_id: String,
    body: Bytes,
) -> Result<String, String> {
    let offer_sdp = String::from_utf8(body.to_vec()).map_err(|_| "offer SDP must be utf-8")?;
    let offer = RTCSessionDescription::offer(offer_sdp).map_err(|e| e.to_string())?;

    let mut media_engine = MediaEngine::default();
    media_engine
        .register_default_codecs()
        .map_err(|e| e.to_string())?;
    let mut setting_engine = SettingEngine::default();
    setting_engine.set_udp_network(UDPNetwork::Ephemeral(
        EphemeralUDP::new(state.webrtc_udp_port_min, state.webrtc_udp_port_max)
            .map_err(|e| e.to_string())?,
    ));
    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_setting_engine(setting_engine)
        .build();
    let peer = api
        .new_peer_connection(RTCConfiguration {
            ice_servers: stun_servers(&state),
            ..Default::default()
        })
        .await
        .map_err(|e| e.to_string())?;

    let sessions = state.live_sessions.clone();
    let live_session_id_for_track = live_session_id.clone();
    peer.on_track(Box::new(move |track, _, _| {
        let sessions = sessions.clone();
        let live_session_id = live_session_id_for_track.clone();
        Box::pin(async move {
            if track.kind() != RTPCodecType::Video {
                return;
            }
            let mut depacketizer = H264Packet::default();
            let mut has_parameter_sets = false;
            loop {
                let Ok((packet, _)) = track.read_rtp().await else {
                    break;
                };
                if !has_parameter_sets {
                    has_parameter_sets = contains_h264_sps(&packet.payload);
                    if !has_parameter_sets {
                        continue;
                    }
                    tracing::info!(
                        live_session_id = %live_session_id,
                        "webrtc h264 parameter sets received"
                    );
                }
                let Ok(chunk) =
                    depacketizer.depacketize(&MediaBytes::copy_from_slice(&packet.payload))
                else {
                    continue;
                };
                if chunk.is_empty() {
                    continue;
                }
                let manager = sessions
                    .get(&live_session_id)
                    .and_then(|session| session.rtmp_manager.clone());
                if let Some(manager) = manager {
                    manager.lock().await.push_h264_annex_b(&chunk);
                }
            }
            tracing::info!(live_session_id = %live_session_id, "webrtc video track ended");
        })
    }));

    peer.set_remote_description(offer)
        .await
        .map_err(|e| e.to_string())?;
    let answer = peer.create_answer(None).await.map_err(|e| e.to_string())?;
    let mut gathering_complete = peer.gathering_complete_promise().await;
    peer.set_local_description(answer)
        .await
        .map_err(|e| e.to_string())?;
    let _ = tokio::time::timeout(Duration::from_secs(3), gathering_complete.recv()).await;
    let answer = peer
        .local_description()
        .await
        .ok_or("missing local SDP answer")?;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(12 * 60 * 60)).await;
        let _ = peer.close().await;
    });
    tracing::info!(live_session_id = %live_session_id, "whip h264 video ingest accepted");
    Ok(answer.sdp)
}

fn contains_h264_sps(payload: &[u8]) -> bool {
    let Some((&first, rest)) = payload.split_first() else {
        return false;
    };
    match first & NALU_TYPE_BITMASK {
        SPS_NALU_TYPE => true,
        STAP_A_NALU_TYPE => stap_a_contains_sps(rest),
        _ => false,
    }
}

fn stap_a_contains_sps(mut payload: &[u8]) -> bool {
    if payload.is_empty() {
        return false;
    }
    while payload.len() >= STAP_A_NALU_LENGTH_SIZE {
        let nalu_size = u16::from_be_bytes([payload[0], payload[1]]) as usize;
        payload = &payload[STAP_A_NALU_LENGTH_SIZE..];
        if nalu_size == 0 || payload.len() < nalu_size {
            return false;
        }
        if payload[0] & NALU_TYPE_BITMASK == SPS_NALU_TYPE {
            return true;
        }
        payload = &payload[nalu_size..];
    }
    false
}

fn stun_servers(state: &BroadcastState) -> Vec<RTCIceServer> {
    if state.webrtc_stun_urls.is_empty() {
        Vec::new()
    } else {
        vec![RTCIceServer {
            urls: state.webrtc_stun_urls.clone(),
            ..Default::default()
        }]
    }
}

fn sdp_response(answer_sdp: String) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", HeaderValue::from_static("application/sdp"));
    (StatusCode::CREATED, headers, answer_sdp).into_response()
}

#[cfg(test)]
mod tests {
    use super::contains_h264_sps;

    #[test]
    fn detects_single_sps_nalu() {
        assert!(contains_h264_sps(&[0x67, 0x42, 0x00, 0x1f]));
    }

    #[test]
    fn detects_sps_inside_stap_a() {
        let payload = [
            24, // STAP-A
            0,
            4,
            0x67,
            0x42,
            0,
            0x1f,
            0,
            2,
            0x68,
            0xce,
        ];
        assert!(contains_h264_sps(&payload));
    }

    #[test]
    fn ignores_non_parameter_frames() {
        assert!(!contains_h264_sps(&[0x65, 1, 2, 3]));
        assert!(!contains_h264_sps(&[28, 0x85, 1, 2, 3]));
    }
}
