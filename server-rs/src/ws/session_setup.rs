//! Session creation and initialization.

use tokio::sync::mpsc;
use axum::extract::ws::Message;

use crate::constants::DEFAULT_TTS_MODEL;
use crate::types::{Lang, Session, Sessions};
use crate::voice_clone;

use super::WsQuery;

pub fn create_session(query: &WsQuery, sessions: &Sessions) -> Option<(String, Lang)> {
    let source_lang = Lang::from_str(&query.source_lang).unwrap_or(Lang::En);
    let target_langs: Vec<Lang> = query
        .target_langs
        .split(',')
        .filter_map(|s| Lang::from_str(s.trim()))
        .collect();

    if target_langs.is_empty() {
        tracing::warn!("[WS] No valid target languages, closing");
        return None;
    }

    let session_id = uuid::Uuid::new_v4().to_string()[..8].to_string();
    let tts_model = resolve_tts_model(&query.tts_model);

    tracing::info!(
        "[WS] Session {} started: {} -> {:?} (tier {}, tts={})",
        session_id, source_lang, target_langs, query.tier, tts_model,
    );

    let mut session = Session::new(session_id.clone(), source_lang.clone(), target_langs, query.tier);
    session.tts_model = tts_model;
    if let Some(vid) = voice_clone::load_persisted_voice() {
        session.voice_clone_id = Some(vid);
    }
    sessions.insert(session_id.clone(), session);

    Some((session_id, source_lang))
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
) {
    let sessions_clone = sessions.clone();
    let sid = session_id.to_string();
    let sl = source_lang.clone();
    tokio::spawn(async move {
        crate::pipeline::start_stt(sid, sessions_clone, sl, audio_rx).await;
    });
}

fn resolve_tts_model(model: &str) -> String {
    match model {
        "flash" => "eleven_flash_v2_5",
        _ => DEFAULT_TTS_MODEL,
    }.to_string()
}
