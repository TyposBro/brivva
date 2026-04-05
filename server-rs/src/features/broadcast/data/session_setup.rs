//! Session creation and initialization.

use tokio::sync::mpsc;
use axum::extract::ws::Message;

use crate::core::config::DEFAULT_TTS_MODEL;
use crate::core::types::Lang; use crate::features::broadcast::domain::{Session, Sessions};
use crate::shared::voice_clone;

use super::ws_handler::{WsQuery, BroadcastDeps};

// ── Internal types ──────────────────────────────────────────────────────────

struct SessionParams {
    session_id: String,
    source_lang: Lang,
    target_langs: Vec<Lang>,
}

// ── Public API ──────────────────────────────────────────────────────────────

pub fn create_session(query: &WsQuery, sessions: &Sessions) -> Option<(String, Lang)> {
    let target_langs = parse_target_langs(&query.target_langs);
    if target_langs.is_empty() {
        tracing::warn!("[WS] No valid target languages, closing");
        return None;
    }

    let params = SessionParams {
        session_id: uuid::Uuid::new_v4().to_string()[..8].to_string(),
        source_lang: Lang::from_str(&query.source_lang).unwrap_or(Lang::En),
        target_langs,
    };
    log_session_start(&params, query);
    let session = build_session(&params, query);
    sessions.insert(params.session_id.clone(), session);

    Some((params.session_id, params.source_lang))
}

pub fn attach_host_channel(sessions: &Sessions, session_id: &str, host_tx: mpsc::UnboundedSender<Message>) {
    if let Some(mut session) = sessions.get_mut(session_id) {
        session.host_tx = Some(host_tx);
    }
}

pub fn spawn_stt_pipeline(
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    deps: &BroadcastDeps,
) {
    let target_langs = sessions.get(session_id)
        .map(|s| s.active_langs())
        .unwrap_or_default();
    let req = crate::shared::stt::SttStartRequest {
        session_id: session_id.to_string(),
        sessions: sessions.clone(),
        source_lang: source_lang.clone(),
        target_langs,
        audio_rx,
        stt_api_key: deps.stt_api_key.clone(),
        tts_api_key: deps.tts_api_key.clone(),
        default_voice: deps.default_voice.clone(),
        http_client: deps.http_client.clone(),
    };
    tokio::spawn(async move {
        crate::shared::stt::start_stt(req).await;
    });
}

// ── create_session helpers ──────────────────────────────────────────────────

fn parse_target_langs(raw: &str) -> Vec<Lang> {
    raw.split(',')
        .filter_map(|s| Lang::from_str(s.trim()))
        .collect()
}

fn log_session_start(params: &SessionParams, query: &WsQuery) {
    let tts_model = resolve_tts_model(&query.tts_model);
    tracing::info!(
        "[WS] Session {} started: {} -> {:?} (tier {}, tts={})",
        params.session_id, params.source_lang, params.target_langs, query.tier, tts_model,
    );
}

fn build_session(params: &SessionParams, query: &WsQuery) -> Session {
    let tts_model = resolve_tts_model(&query.tts_model);
    let mut session = Session::new(
        params.session_id.clone(),
        params.source_lang.clone(),
        params.target_langs.clone(),
        query.tier,
    );
    session.tts_model = tts_model;
    apply_persisted_voice(&mut session);
    session
}

fn apply_persisted_voice(session: &mut Session) {
    if let Some(vid) = voice_clone::load_persisted_voice() {
        session.voice_clone_id = Some(vid);
    }
}

fn resolve_tts_model(model: &str) -> String {
    match model {
        "flash" => "eleven_flash_v2_5",
        _ => DEFAULT_TTS_MODEL,
    }.to_string()
}
