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
use std::io::Write;
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
const SPS_NALU_TYPE: u8 = 7;
const PPS_NALU_TYPE: u8 = 8;
const LIVE_SESSION_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
const LIVE_SESSION_LOOKUP_INTERVAL: Duration = Duration::from_millis(100);
const H264_PARAMETER_SET_PENDING_LIMIT: usize = 120;

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
    let Some(live_session_id) =
        wait_for_live_session_id(&state, session_id, LIVE_SESSION_LOOKUP_TIMEOUT).await
    else {
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

async fn wait_for_live_session_id(
    state: &BroadcastState,
    session_id: &str,
    timeout: Duration,
) -> Option<String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(live_session_id) = find_live_session_id(state, session_id) {
            return Some(live_session_id);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(LIVE_SESSION_LOOKUP_INTERVAL).await;
    }
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
            let mut parameter_sets = H264ParameterSets::default();
            let mut pending_chunks: Vec<Vec<u8>> = Vec::new();
            let mut debug_h264_dump = open_debug_h264_dump();
            loop {
                let Ok((packet, _)) = track.read_rtp().await else {
                    break;
                };
                let Ok(chunk) =
                    depacketizer.depacketize(&MediaBytes::copy_from_slice(&packet.payload))
                else {
                    continue;
                };
                if chunk.is_empty() {
                    continue;
                }
                let chunk = ensure_annex_b_start_code(&chunk);
                if let Some(file) = debug_h264_dump.as_mut() {
                    let _ = file.write_all(&chunk);
                }
                if !parameter_sets.complete() {
                    parameter_sets.merge(h264_parameter_sets_in_chunk(&chunk));
                    pending_chunks.push(chunk);
                    if pending_chunks.len() > H264_PARAMETER_SET_PENDING_LIMIT {
                        pending_chunks.remove(0);
                    }
                    if !parameter_sets.complete() {
                        continue;
                    }
                    tracing::info!(
                        live_session_id = %live_session_id,
                        "webrtc h264 parameter sets received"
                    );
                    if let Some(manager) = sessions
                        .get(&live_session_id)
                        .and_then(|session| session.rtmp_manager.clone())
                    {
                        let manager = manager.lock().await;
                        for pending in pending_chunks.drain(..) {
                            manager.push_h264_annex_b(&pending);
                        }
                    }
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

fn open_debug_h264_dump() -> Option<std::fs::File> {
    let path = std::env::var("BRIVVA_DEBUG_WEBRTC_H264_DUMP").ok()?;
    std::fs::File::create(path).ok()
}

#[derive(Default, Clone, Copy)]
struct H264ParameterSets {
    sps: bool,
    pps: bool,
}

impl H264ParameterSets {
    fn merge(&mut self, other: Self) {
        self.sps |= other.sps;
        self.pps |= other.pps;
    }

    fn complete(&self) -> bool {
        self.sps && self.pps
    }
}

fn h264_parameter_sets_in_chunk(chunk: &[u8]) -> H264ParameterSets {
    let mut sets = H264ParameterSets::default();
    let mut i = 0;
    while i < chunk.len() {
        let Some((start, nalu_start)) = find_annex_b_nalu(chunk, i) else {
            if i == 0 && !has_annex_b_start_code(chunk) {
                mark_parameter_set(&mut sets, chunk[0]);
            }
            break;
        };
        if nalu_start < chunk.len() {
            mark_parameter_set(&mut sets, chunk[nalu_start]);
        }
        i = start + 3;
    }
    sets
}

fn find_annex_b_nalu(chunk: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i + 3 <= chunk.len() {
        if chunk[i..].starts_with(&[0, 0, 1]) {
            return Some((i, i + 3));
        }
        if i + 4 <= chunk.len() && chunk[i..].starts_with(&[0, 0, 0, 1]) {
            return Some((i, i + 4));
        }
        i += 1;
    }
    None
}

fn mark_parameter_set(sets: &mut H264ParameterSets, nalu_header: u8) {
    match nalu_header & NALU_TYPE_BITMASK {
        SPS_NALU_TYPE => sets.sps = true,
        PPS_NALU_TYPE => sets.pps = true,
        _ => {}
    }
}

fn ensure_annex_b_start_code(chunk: &[u8]) -> Vec<u8> {
    if has_annex_b_start_code(chunk) {
        chunk.to_vec()
    } else {
        let mut annex_b = Vec::with_capacity(chunk.len() + 4);
        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(chunk);
        annex_b
    }
}

fn has_annex_b_start_code(chunk: &[u8]) -> bool {
    chunk.starts_with(&[0, 0, 1]) || chunk.starts_with(&[0, 0, 0, 1])
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
    use super::{
        ensure_annex_b_start_code, h264_parameter_sets_in_chunk, wait_for_live_session_id,
    };
    use crate::features::broadcast::data::state::BroadcastState;
    use crate::features::broadcast::domain::{Lang, LiveSession, PipelineConfig};
    use std::{sync::Arc, time::Duration};

    #[test]
    fn adds_annex_b_start_code_to_raw_nalu() {
        assert_eq!(
            ensure_annex_b_start_code(&[0x65, 1, 2, 3]),
            vec![0, 0, 0, 1, 0x65, 1, 2, 3]
        );
    }

    #[test]
    fn keeps_existing_annex_b_start_code() {
        assert_eq!(
            ensure_annex_b_start_code(&[0, 0, 0, 1, 0x65, 1]),
            vec![0, 0, 0, 1, 0x65, 1]
        );
    }

    #[test]
    fn detects_sps_and_pps_in_annex_b_chunk() {
        let sets =
            h264_parameter_sets_in_chunk(&[0, 0, 0, 1, 0x67, 1, 2, 3, 0, 0, 0, 1, 0x68, 4, 5, 6]);
        assert!(sets.sps);
        assert!(sets.pps);
        assert!(sets.complete());
    }

    #[tokio::test]
    async fn waits_for_matching_live_session() {
        let state = BroadcastState::default();
        let sessions = state.live_sessions.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            sessions.insert(
                "LIVE01".into(),
                LiveSession::new(
                    "LIVE01".into(),
                    Lang::En,
                    Some("sess-1".into()),
                    Arc::new(PipelineConfig::default()),
                ),
            );
        });

        let found = wait_for_live_session_id(&state, "sess-1", Duration::from_secs(1)).await;
        assert_eq!(found.as_deref(), Some("LIVE01"));
    }

    #[tokio::test]
    async fn wait_for_matching_live_session_times_out() {
        let state = BroadcastState::default();
        let found = wait_for_live_session_id(&state, "missing", Duration::from_millis(1)).await;
        assert!(found.is_none());
    }
}
