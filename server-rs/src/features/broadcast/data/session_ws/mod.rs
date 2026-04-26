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
use std::sync::atomic::AtomicBool;
use tokio::sync::mpsc;

use crate::features::broadcast::data::auth;
use crate::features::broadcast::data::ffmpeg::VideoInputMode;
use crate::features::broadcast::data::state::BroadcastState;
use crate::features::broadcast::data::workers_api::WorkersApi;
use crate::features::broadcast::domain::{Lang, LiveSession, PipelineConfig, SessionQuery};

mod active_voice_refresh;
mod bootstrap;
mod eviction;
mod ids;
mod messages;
mod rtmp;
mod teardown;

pub use active_voice_refresh::refresh_active_voice_once;
pub use bootstrap::{StreamPreflight, preflight_streams};
pub use rtmp::{PASS_LANG_CODE, StreamPipelineFlags, maybe_downgrade_rtmps, resolve_stream_flags};

use bootstrap::{BootstrapArgs, BootstrapOutcome, bootstrap_session};
use eviction::evict_stale_live_sessions;
use ids::next_available_live_session_id;
use messages::{BinaryArgs, handle_binary, handle_text};
use teardown::{TeardownArgs, teardown_session};

/// WS entry. Accepts only authenticated hosts — no guests, no join codes.
pub async fn session_ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<SessionQuery>,
    State(state): State<BroadcastState>,
) -> Response {
    ws.on_upgrade(move |socket| handle_host_socket(socket, state, query))
}

async fn handle_host_socket(socket: WebSocket, state: BroadcastState, query: SessionQuery) {
    let Some(claims) = authenticate(&state, &query) else {
        return;
    };
    let source_lang = query
        .source_lang
        .as_deref()
        .and_then(Lang::from_str)
        .unwrap_or(Lang::En);
    let (sender, receiver) = socket.split();
    handle_host(HostSocket {
        sender,
        receiver,
        state,
        user_id: claims.sub,
        source_lang,
        session_id: query.session_id,
        query_video_mode: query.video_mode,
    })
    .await;
}

fn authenticate(state: &BroadcastState, query: &SessionQuery) -> Option<auth::Claims> {
    let token = match query.token.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => {
            tracing::warn!("ws host upgrade rejected: missing token");
            return None;
        }
    };
    match auth::verify(token, &state.jwt_secret) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(error = %e, "ws host upgrade rejected: jwt verify failed");
            None
        }
    }
}

fn pipeline_config_from(state: &BroadcastState) -> Arc<PipelineConfig> {
    Arc::new(PipelineConfig {
        soniox_api_key: state.soniox_api_key.clone(),
        soniox_ws_url: state.soniox_ws_url.clone(),
        elevenlabs_api_key: state.elevenlabs_api_key.clone(),
        elevenlabs_base_url: state.elevenlabs_base_url.clone(),
        force_default_voice: state.force_default_voice,
        force_rtmp_not_rtmps: state.force_rtmp_not_rtmps,
    })
}

struct HostSocket {
    sender: SplitSink<WebSocket, Message>,
    receiver: SplitStream<WebSocket>,
    state: BroadcastState,
    user_id: String,
    source_lang: Lang,
    session_id: Option<String>,
    query_video_mode: Option<String>,
}

async fn handle_host(mut socket: HostSocket) {
    let live_sessions = socket.state.live_sessions.clone();
    let live_session_id = next_available_live_session_id(&live_sessions);

    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<Message>();
    let workers_api = Arc::new(WorkersApi::new(
        &socket.state.workers_api_url,
        &socket.state.internal_secret,
    ));

    let mut live_session = LiveSession::new(
        live_session_id.clone(),
        socket.source_lang.clone(),
        socket.session_id.clone(),
        pipeline_config_from(&socket.state),
    );
    live_session.video_input_mode = match socket.query_video_mode.as_deref() {
        Some("webrtc-h264") => VideoInputMode::H264AnnexB,
        _ => VideoInputMode::Mjpeg,
    };
    live_session.host_tx = Some(host_tx);

    let ffmpeg_monitor_stop = Arc::new(AtomicBool::new(false));

    // Defensive: a single broadcast session can only have ONE active
    // live_session. The FE is supposed to close its WS on "stop recording"
    // and only re-open on "start recording", but in production we observed
    // the prior WS lingering — its TTS/RTMP handles stayed bound to the
    // stale live_session while audio routed to the new one, silencing TTS
    // output for ~6 minutes. Evict any pre-existing live_session for the
    // same FE session_id BEFORE inserting the new one so there is never a
    // window where two pipelines push to the same RTMP destination.
    if let Some(ref sid) = socket.session_id {
        evict_stale_live_sessions(&live_sessions, sid, &live_session_id).await;
    }

    if let Some(ref sid) = socket.session_id {
        let outcome = bootstrap_session(BootstrapArgs {
            workers_api: &workers_api,
            sid,
            user_id: &socket.user_id,
            source_lang: &socket.source_lang,
            live_session: &mut live_session,
            live_session_id: &live_session_id,
            ffmpeg_monitor_stop: ffmpeg_monitor_stop.clone(),
            live_sessions: live_sessions.clone(),
        })
        .await;
        match outcome {
            BootstrapOutcome::Continue => {}
            BootstrapOutcome::Abort => return,
            BootstrapOutcome::AbortWithError(msg) => {
                // Surface the failure to the FE so it can render a banner
                // instead of the default "Waiting for utterances..." spinner.
                let payload = serde_json::json!({ "type": "error", "message": msg });
                let _ = socket
                    .sender
                    .send(Message::Text(payload.to_string().into()))
                    .await;
                let _ = socket.sender.close().await;
                return;
            }
        }
    }

    live_sessions.insert(live_session_id.clone(), live_session);

    let send_task = tokio::spawn(async move {
        while let Some(msg) = host_rx.recv().await {
            if socket.sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    let mut audio_tx: Option<mpsc::Sender<Vec<u8>>> = None;
    while let Some(Ok(msg)) = socket.receiver.next().await {
        match msg {
            Message::Binary(data) => {
                handle_binary(BinaryArgs {
                    data: data.to_vec(),
                    live_sessions: &live_sessions,
                    live_session_id: &live_session_id,
                    source_lang: &socket.source_lang,
                    audio_tx: &mut audio_tx,
                });
            }
            Message::Text(text) => {
                if text.contains("host:end") {
                    break;
                }
                handle_text(&text, &live_sessions, &live_session_id).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    teardown_session(TeardownArgs {
        live_sessions: &live_sessions,
        live_session_id: &live_session_id,
        workers_api: &workers_api,
        ffmpeg_monitor_stop,
    })
    .await;
    send_task.abort();
    tracing::info!(live_session_id = %live_session_id, "live session closed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::data::state::BroadcastState;

    #[test]
    fn pipeline_config_from_maps_every_broadcast_state_field() {
        let mut state = BroadcastState::new();
        state.soniox_api_key = "sk-s".into();
        state.soniox_ws_url = "wss://s".into();
        state.elevenlabs_api_key = "sk-e".into();
        state.elevenlabs_base_url = "https://e".into();
        state.force_default_voice = true;
        state.force_rtmp_not_rtmps = true;

        let cfg = pipeline_config_from(&state);
        assert_eq!(cfg.soniox_api_key, "sk-s");
        assert_eq!(cfg.soniox_ws_url, "wss://s");
        assert_eq!(cfg.elevenlabs_api_key, "sk-e");
        assert_eq!(cfg.elevenlabs_base_url, "https://e");
        assert!(cfg.force_default_voice);
        assert!(cfg.force_rtmp_not_rtmps);
    }
}
