//! Session creation and initialization.

use tokio::sync::mpsc;
use axum::extract::ws::Message;

use crate::core::config::{DEFAULT_TTS_MODEL, DEFAULT_VOICE_ID, DASHSCOPE_TTS_MODEL_VC};
use crate::core::types::Lang;
use crate::features::broadcast::domain::{Session, Sessions};
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
    let session_ref = sessions.get(session_id);
    let target_langs = session_ref.as_ref()
        .map(|s| s.active_langs())
        .unwrap_or_default();
    let tts_provider = session_ref.as_ref()
        .map(|s| s.tts_provider.clone())
        .unwrap_or_default();

    spawn_tts_warmup(&session_ref, &tts_provider, deps);
    drop(session_ref);

    let tts_api_key = resolve_tts_api_key(&tts_provider, deps);
    let req = crate::shared::stt::SttStartRequest {
        session_id: session_id.to_string(),
        sessions: sessions.clone(),
        source_lang: source_lang.clone(),
        target_langs,
        audio_rx,
        stt_api_key: deps.stt_api_key.clone(),
        tts_api_key,
        default_voice: deps.default_voice.clone(),
        http_client: deps.http_client.clone(),
    };
    tokio::spawn(async move {
        crate::shared::stt::start_stt(req).await;
    });
}

fn resolve_tts_api_key(provider: &str, deps: &BroadcastDeps) -> String {
    match provider {
        "dashscope" => deps.dashscope_api_key.clone(),
        _ => deps.tts_api_key.clone(),
    }
}

// ── create_session helpers ──────────────────────────────────────────────────

fn parse_target_langs(raw: &str) -> Vec<Lang> {
    raw.split(',')
        .filter_map(|s| Lang::from_str(s.trim()))
        .collect()
}

fn log_session_start(params: &SessionParams, query: &WsQuery) {
    let tts_model = resolve_tts_model(&query.tts_model, &query.tts_provider);
    tracing::info!(
        "[WS] Session {} started: {} -> {:?} (tier {}, provider={}, tts={})",
        params.session_id, params.source_lang, params.target_langs,
        query.tier, query.tts_provider, tts_model,
    );
}

fn build_session(params: &SessionParams, query: &WsQuery) -> Session {
    let tts_model = resolve_tts_model(&query.tts_model, &query.tts_provider);
    let mut session = Session::new(
        params.session_id.clone(),
        params.source_lang.clone(),
        params.target_langs.clone(),
        query.tier,
    );
    session.tts_model = tts_model;
    session.tts_provider = query.tts_provider.clone();
    apply_persisted_voice(&mut session);
    session
}

fn apply_persisted_voice(session: &mut Session) {
    if let Some(vid) = voice_clone::load_persisted_voice() {
        session.voice_clone_id = Some(vid);
    }
}

fn resolve_tts_model(model: &str, provider: &str) -> String {
    match provider {
        "dashscope" => DASHSCOPE_TTS_MODEL_VC.to_string(),
        _ => match model {
            "flash" => "eleven_flash_v2_5",
            _ => DEFAULT_TTS_MODEL,
        }.to_string(),
    }
}

// ── TTS warm-up ─────────────────────────────────────────────────────────────

fn spawn_tts_warmup(
    session_ref: &Option<dashmap::mapref::one::Ref<'_, String, Session>>,
    tts_provider: &str,
    deps: &BroadcastDeps,
) {
    let Some(session) = session_ref.as_ref() else { return };
    if session.tier < 2 { return; }
    if tts_provider == "dashscope" { return; } // DashScope doesn't need warmup

    let voice_id = session.voice_clone_id.clone()
        .unwrap_or_else(|| DEFAULT_VOICE_ID.to_string());
    let model_id = session.tts_model.clone();
    let api_key = deps.tts_api_key.clone();

    tokio::spawn(async move {
        crate::shared::tts::warm_up_tts_ws(&voice_id, &model_id, &api_key).await;
    });
}
