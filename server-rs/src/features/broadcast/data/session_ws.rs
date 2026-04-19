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
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::core::contracts::workers::SessionBundle;
use crate::features::broadcast::data::metrics::spawn_metrics_reporter;
use crate::features::broadcast::data::state::BroadcastState;
use crate::features::broadcast::data::workers_api::WorkersApi;
use crate::features::broadcast::data::{auth, pipeline};
use crate::features::broadcast::domain::SessionMetrics;
use crate::features::broadcast::domain::{
    Lang, LiveSession, LiveSessions, PipelineConfig, SessionQuery,
};

/// Pure URL transform for the FORCE_RTMP_NOT_RTMPS kill-switch. Exposed so
/// the kill-switch integration tests can exercise branch selection without
/// spawning FFmpeg.
pub fn maybe_downgrade_rtmps(url: &str, force_rtmp: bool) -> String {
    if force_rtmp && let Some(rest) = url.strip_prefix("rtmps://") {
        return format!("rtmp://{}", rest);
    }
    url.to_string()
}

/// Wire value the frontend sends on a destination's `lang` field when the
/// user picks "Passthrough (source)" — the pipeline re-broadcasts host audio
/// unchanged and skips STT/translate/TTS for that stream. Kept in sync with
/// `@brivva/contracts/platforms` PASS_LANG_CODE.
pub const PASS_LANG_CODE: &str = "pass";

/// Per-stream pipeline flags derived from the Workers-provided stream row +
/// the session's source language. Pulled out of `start_rtmp_streams` so the
/// decision ("passthrough? is_source? what gain?") can be unit-tested without
/// spawning FFmpeg.
#[derive(Debug, PartialEq)]
pub struct StreamPipelineFlags {
    pub is_source: bool,
    pub passthrough: bool,
    pub host_gain: f32,
}

pub fn resolve_stream_flags(
    stream_lang: &str,
    source_lang: &Lang,
    raw_gain: f32,
) -> StreamPipelineFlags {
    let passthrough = stream_lang == PASS_LANG_CODE;
    let is_source_match = Lang::from_str(stream_lang).is_some_and(|l| &l == source_lang);
    let is_source = passthrough || is_source_match;
    let host_gain = if passthrough {
        1.0
    } else {
        raw_gain.clamp(0.0, 1.0)
    };
    StreamPipelineFlags {
        is_source,
        passthrough,
        host_gain,
    }
}

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

fn generate_live_session_id() -> String {
    Uuid::new_v4().to_string()[..6].to_uppercase()
}

fn next_available_live_session_id(live_sessions: &dashmap::DashMap<String, LiveSession>) -> String {
    for _ in 0..16 {
        let candidate = generate_live_session_id();
        if !live_sessions.contains_key(&candidate) {
            return candidate;
        }
    }

    loop {
        let candidate = Uuid::new_v4().simple().to_string()[..10].to_uppercase();
        if !live_sessions.contains_key(&candidate) {
            return candidate;
        }
    }
}

// ── Host Flow ─────────────────────────────────────────────

struct HostSocket {
    sender: SplitSink<WebSocket, Message>,
    receiver: SplitStream<WebSocket>,
    state: BroadcastState,
    user_id: String,
    source_lang: Lang,
    session_id: Option<String>,
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
    live_session.host_tx = Some(host_tx);

    let ffmpeg_monitor_stop = Arc::new(AtomicBool::new(false));

    if let Some(ref sid) = socket.session_id {
        let outcome = bootstrap_session(BootstrapArgs {
            workers_api: &workers_api,
            sid,
            user_id: &socket.user_id,
            source_lang: &socket.source_lang,
            live_session: &mut live_session,
            live_session_id: &live_session_id,
            ffmpeg_monitor_stop: ffmpeg_monitor_stop.clone(),
        })
        .await;
        if matches!(outcome, BootstrapOutcome::Abort) {
            return;
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

enum BootstrapOutcome {
    Continue,
    Abort,
}

struct BootstrapArgs<'a> {
    workers_api: &'a Arc<WorkersApi>,
    sid: &'a str,
    user_id: &'a str,
    source_lang: &'a Lang,
    live_session: &'a mut LiveSession,
    live_session_id: &'a str,
    ffmpeg_monitor_stop: Arc<AtomicBool>,
}

async fn bootstrap_session(args: BootstrapArgs<'_>) -> BootstrapOutcome {
    let BootstrapArgs {
        workers_api,
        sid,
        user_id,
        source_lang,
        live_session,
        live_session_id,
        ffmpeg_monitor_stop,
    } = args;

    let bundle = match workers_api.fetch_session_bundle(sid).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(session_id = %sid, error = %e, "session bundle fetch failed");
            return BootstrapOutcome::Abort;
        }
    };

    if bundle.session.user_id != user_id {
        tracing::warn!(
            session_id = %sid,
            owner_user_id = %bundle.session.user_id,
            jwt_user_id = %user_id,
            "ws host upgrade rejected: session owner mismatch"
        );
        return BootstrapOutcome::Abort;
    }

    if let Some(v) = bundle.voice.clone() {
        live_session.selected_voice_id = Some(v.elevenlabs_voice_id);
        // `enrollment_lang` isn't in the Workers Voice schema yet. When it
        // ships (as the optional `enrollment_lang` field on the Voice row),
        // populate `live_session.selected_voice_enrollment_lang` here with
        // `Lang::from_str(&v.enrollment_lang.unwrap_or_default())`.
    }

    let metrics = SessionMetrics::new();
    live_session.metrics = Some(metrics.clone());

    start_rtmp_streams(RtmpStartArgs {
        bundle: &bundle,
        source_lang,
        live_session,
        sid,
        ffmpeg_monitor_stop: ffmpeg_monitor_stop.clone(),
        metrics: metrics.clone(),
    });
    spawn_live_status_update(
        workers_api.clone(),
        sid.to_string(),
        live_session_id.to_string(),
    );
    let _metrics_reporter = spawn_metrics_reporter(
        metrics,
        workers_api.clone(),
        Some(sid.to_string()),
        live_session_id.to_string(),
        ffmpeg_monitor_stop,
    );
    BootstrapOutcome::Continue
}

struct RtmpStartArgs<'a> {
    bundle: &'a SessionBundle,
    source_lang: &'a Lang,
    live_session: &'a mut LiveSession,
    sid: &'a str,
    ffmpeg_monitor_stop: Arc<AtomicBool>,
    metrics: Arc<SessionMetrics>,
}

fn start_rtmp_streams(args: RtmpStartArgs<'_>) {
    let RtmpStartArgs {
        bundle,
        source_lang,
        live_session,
        sid,
        ffmpeg_monitor_stop,
        metrics,
    } = args;
    if bundle.streams.is_empty() {
        return;
    }
    let mut manager = crate::features::broadcast::data::ffmpeg::RtmpManager::new();
    manager.set_metrics(metrics);
    let mut rtmp_langs = Vec::new();
    let force_rtmp = live_session.pipeline_config.force_rtmp_not_rtmps;
    for s in &bundle.streams {
        let (Some(rtmp_url), Some(stream_key)) = (&s.rtmp_url, &s.stream_key) else {
            continue;
        };
        let full_url_with_key = if stream_key.is_empty() {
            rtmp_url.clone()
        } else {
            format!("{}/{}", rtmp_url.trim_end_matches('/'), stream_key)
        };
        let full_url = maybe_downgrade_rtmps(&full_url_with_key, force_rtmp);
        if force_rtmp && full_url != full_url_with_key {
            tracing::warn!(
                stream_id = %s.id,
                "kill-switch FORCE_RTMP_NOT_RTMPS: downgraded rtmps:// to rtmp://"
            );
        }
        // User explicitly picked "Passthrough (source)" on this destination.
        // The pipeline MUST skip STT/translate/TTS: host audio RTMP'd raw at
        // full gain, no caption overlay. We implement it via the same
        // `is_source=true` switch target-lang streams already use, and carry
        // a dedicated `passthrough` flag so downstream code can distinguish
        // "user chose passthrough" from "stream's lang happens to equal
        // source_lang" for tracing / future bifurcation.
        let flags = resolve_stream_flags(&s.lang, source_lang, s.host_gain);
        let spawn_args = crate::features::broadcast::data::ffmpeg::StartStreamArgs {
            stream_id: &s.id,
            lang: &s.lang,
            rtmp_url: &full_url,
            delay_ms: s.delay_ms,
            is_source: flags.is_source,
            host_gain: flags.host_gain,
            passthrough: flags.passthrough,
        };
        if let Err(e) = manager.start_stream(spawn_args) {
            tracing::error!(
                stream_id = %s.id,
                lang = %s.lang,
                error = %e,
                "rtmp stream start failed"
            );
            continue;
        }
        // Passthrough streams don't participate in translation, so they must
        // not seed an STT/translate pipeline. Source-lang matches are pushed
        // like before — `dedupe_target_langs` filters them against the source.
        if !flags.passthrough
            && let Some(lang) = Lang::from_str(&s.lang)
        {
            rtmp_langs.push(lang);
        }
    }
    let shared_mgr = Arc::new(tokio::sync::Mutex::new(manager));
    live_session.rtmp_manager = Some(shared_mgr.clone());
    live_session.rtmp_langs = rtmp_langs;
    tracing::info!(
        session_id = %sid,
        stream_count = bundle.streams.len(),
        langs = ?live_session.rtmp_langs,
        "ffmpeg rtmp streams started"
    );
    let _health_monitor = crate::features::broadcast::data::ffmpeg::spawn_health_monitor(
        shared_mgr,
        ffmpeg_monitor_stop,
    );
}

fn spawn_live_status_update(workers_api: Arc<WorkersApi>, sid: String, live_session_id: String) {
    tokio::spawn(async move {
        if let Err(e) = workers_api
            .update_session_status(&sid, "live", Some(&live_session_id))
            .await
        {
            tracing::warn!(
                session_id = %sid,
                error = %e,
                "workers status=live update failed"
            );
        }
    });
}

struct BinaryArgs<'a> {
    data: Vec<u8>,
    live_sessions: &'a LiveSessions,
    live_session_id: &'a str,
    source_lang: &'a Lang,
    audio_tx: &'a mut Option<mpsc::Sender<Vec<u8>>>,
}

fn handle_binary(args: BinaryArgs<'_>) {
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

async fn handle_text(text: &str, live_sessions: &LiveSessions, live_session_id: &str) {
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

struct TeardownArgs<'a> {
    live_sessions: &'a LiveSessions,
    live_session_id: &'a str,
    workers_api: &'a Arc<WorkersApi>,
    ffmpeg_monitor_stop: Arc<AtomicBool>,
}

async fn teardown_session(args: TeardownArgs<'_>) {
    let TeardownArgs {
        live_sessions,
        live_session_id,
        workers_api,
        ffmpeg_monitor_stop,
    } = args;
    ffmpeg_monitor_stop.store(true, Ordering::Release);
    let Some((_, live_session)) = live_sessions.remove(live_session_id) else {
        return;
    };
    if let Some(manager) = live_session.rtmp_manager {
        let mut mgr = manager.lock().await;
        mgr.stop_all().await;
    }
    if let Some(sid) = live_session.session_id {
        let workers_for_end = workers_api.clone();
        tokio::spawn(async move {
            if let Err(e) = workers_for_end
                .update_session_status(&sid, "ended", None)
                .await
            {
                tracing::warn!(
                    session_id = %sid,
                    error = %e,
                    "workers status=ended update failed"
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::data::state::BroadcastState;
    use crate::features::broadcast::domain::{Lang, LiveSessions, PipelineConfig};

    #[test]
    fn next_available_live_session_id_skips_existing_entries() {
        let live_sessions = dashmap::DashMap::new();
        live_sessions.insert(
            "ABC123".into(),
            LiveSession::new(
                "ABC123".into(),
                Lang::En,
                None,
                Arc::new(PipelineConfig::default()),
            ),
        );

        for _ in 0..32 {
            let id = next_available_live_session_id(&live_sessions);
            assert_ne!(id, "ABC123");
            assert!(!id.is_empty());
        }
    }

    #[test]
    fn generate_live_session_id_produces_6_char_uppercase_hex() {
        for _ in 0..8 {
            let id = generate_live_session_id();
            assert_eq!(id.len(), 6);
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
            );
        }
    }

    #[test]
    fn maybe_downgrade_rtmps_preserves_url_when_flag_is_off_even_for_rtmps() {
        let rewritten = maybe_downgrade_rtmps("rtmps://x/y", false);
        assert_eq!(rewritten, "rtmps://x/y");
    }

    #[test]
    fn maybe_downgrade_rtmps_does_not_rewrite_non_rtmps_schemes_even_with_flag() {
        assert_eq!(maybe_downgrade_rtmps("rtmp://x/y", true), "rtmp://x/y");
        assert_eq!(
            maybe_downgrade_rtmps("srt://host:9999?x=1", true),
            "srt://host:9999?x=1"
        );
    }

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

    #[tokio::test]
    async fn teardown_session_is_noop_when_live_session_id_missing() {
        let live_sessions: LiveSessions = Arc::new(dashmap::DashMap::new());
        let workers_api = Arc::new(WorkersApi::new("", ""));
        let stop = Arc::new(AtomicBool::new(false));
        teardown_session(TeardownArgs {
            live_sessions: &live_sessions,
            live_session_id: "MISSING",
            workers_api: &workers_api,
            ffmpeg_monitor_stop: stop.clone(),
        })
        .await;
        assert!(
            stop.load(Ordering::Acquire),
            "stop flag still set on noop path"
        );
    }

    #[tokio::test]
    async fn teardown_session_removes_live_session_and_sets_stop_flag() {
        let live_sessions: LiveSessions = Arc::new(dashmap::DashMap::new());
        live_sessions.insert(
            "LIVE01".into(),
            LiveSession::new(
                "LIVE01".into(),
                Lang::En,
                None,
                Arc::new(PipelineConfig::default()),
            ),
        );
        let workers_api = Arc::new(WorkersApi::new("", ""));
        let stop = Arc::new(AtomicBool::new(false));
        teardown_session(TeardownArgs {
            live_sessions: &live_sessions,
            live_session_id: "LIVE01",
            workers_api: &workers_api,
            ffmpeg_monitor_stop: stop.clone(),
        })
        .await;
        assert!(live_sessions.is_empty());
        assert!(stop.load(Ordering::Acquire));
    }

    // ── Passthrough decision logic ───────────────────────────

    #[test]
    fn resolve_stream_flags_marks_pass_sentinel_as_passthrough_with_unit_gain() {
        let flags = resolve_stream_flags(PASS_LANG_CODE, &Lang::En, 0.2);
        assert!(flags.passthrough);
        assert!(
            flags.is_source,
            "passthrough must short-circuit drain via is_source"
        );
        assert!((flags.host_gain - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_stream_flags_pins_host_gain_at_unit_even_if_workers_sent_lower_value() {
        // Defensive: Workers sets host_gain to 1.0 when lang==source_lang, but
        // the "pass" sentinel is a separate wire value. If some migration
        // path slips a passthrough row through with host_gain=0.2 (the stale
        // target default), Fargate must still emit raw audio at full volume.
        let flags = resolve_stream_flags(PASS_LANG_CODE, &Lang::Ko, 0.2);
        assert!((flags.host_gain - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_stream_flags_preserves_existing_source_match_behavior_when_lang_equals_source() {
        // Historical path: user picked the source language as a target. We
        // keep treating it as is_source=true (no TTS, gain honored as-is).
        let flags = resolve_stream_flags("en", &Lang::En, 0.2);
        assert!(!flags.passthrough);
        assert!(flags.is_source);
        assert!((flags.host_gain - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_stream_flags_translated_target_lang_is_not_source_and_clamps_gain() {
        let flags = resolve_stream_flags("ja", &Lang::En, 5.0);
        assert!(!flags.passthrough);
        assert!(!flags.is_source);
        assert!((flags.host_gain - 1.0).abs() < f32::EPSILON);

        let flags_neg = resolve_stream_flags("ja", &Lang::En, -0.5);
        assert!(flags_neg.host_gain.abs() < f32::EPSILON);
    }

    #[test]
    fn resolve_stream_flags_unknown_lang_code_treated_as_non_source_non_passthrough() {
        // Future-proofing: a new lang code lands in D1 before the server is
        // redeployed. Lang::from_str returns None, is_source=false, no
        // passthrough. The stream still starts (the FFmpeg layer accepts
        // arbitrary lang strings for caption/metrics labeling).
        let flags = resolve_stream_flags("th", &Lang::En, 0.2);
        assert!(!flags.passthrough);
        assert!(!flags.is_source);
    }
}
