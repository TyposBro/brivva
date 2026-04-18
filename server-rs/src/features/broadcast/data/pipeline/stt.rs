use crate::features::broadcast::domain::{Lang, LiveSessions, ServerMsg};
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use super::soniox::{
    SONIOX_END_TOKEN, SONIOX_WS_URL, SonioxMode, SonioxResponse, SONIOX_MODEL,
};
use super::to_ws;
use super::tts::broadcast_translated_tts;

static SONIOX_API_KEY: LazyLock<String> =
    LazyLock::new(|| std::env::var("SONIOX_API_KEY").unwrap_or_default());

const STT_RECONNECT_MAX: u32 = 5;
const STT_RECONNECT_DELAY: Duration = Duration::from_secs(1);

pub async fn start_stt_pipelines(
    live_session_id: String,
    live_sessions: LiveSessions,
    source_lang: Lang,
    target_langs: Vec<Lang>,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
) {
    if SONIOX_API_KEY.is_empty() {
        eprintln!("[STT] SONIOX_API_KEY not set — STT pipeline disabled");
        return;
    }

    let targets = dedupe_target_langs(source_lang.clone(), target_langs);
    let (source_tx, source_rx) = mpsc::channel::<Vec<u8>>(64);
    let mut target_senders = Vec::new();
    let mut target_receivers = Vec::new();

    for lang in &targets {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
        target_senders.push(tx);
        target_receivers.push((lang.clone(), rx));
    }

    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            let _ = source_tx.try_send(chunk.clone());
            for tx in &target_senders {
                let _ = tx.try_send(chunk.clone());
            }
        }
    });

    spawn_source_session(&live_session_id, &live_sessions, source_lang, source_rx);
    spawn_translate_sessions(&live_session_id, &live_sessions, source_lang, target_receivers);
}

fn dedupe_target_langs(source_lang: Lang, target_langs: Vec<Lang>) -> Vec<Lang> {
    let mut seen = std::collections::HashSet::new();
    target_langs
        .into_iter()
        .filter(|lang| *lang != source_lang && seen.insert(lang.clone()))
        .collect()
}

fn spawn_source_session(
    live_session_id: &str,
    live_sessions: &LiveSessions,
    source_lang: Lang,
    audio_rx: mpsc::Receiver<Vec<u8>>,
) {
    let live_session_id = live_session_id.to_string();
    let live_sessions = live_sessions.clone();
    tokio::spawn(async move {
        run_soniox_session(
            live_session_id,
            live_sessions,
            SonioxMode::Source { lang: source_lang },
            audio_rx,
        )
        .await;
    });
}

fn spawn_translate_sessions(
    live_session_id: &str,
    live_sessions: &LiveSessions,
    source_lang: Lang,
    target_receivers: Vec<(Lang, mpsc::Receiver<Vec<u8>>)>,
) {
    for (target_lang, target_rx) in target_receivers {
        let live_session_id = live_session_id.to_string();
        let live_sessions = live_sessions.clone();
        let source_lang = source_lang.clone();
        tokio::spawn(async move {
            run_soniox_session(
                live_session_id,
                live_sessions,
                SonioxMode::Translate {
                    source_lang,
                    target_lang,
                },
                target_rx,
            )
            .await;
        });
    }
}

async fn run_soniox_session(
    live_session_id: String,
    live_sessions: LiveSessions,
    mode: SonioxMode,
    audio_rx: mpsc::Receiver<Vec<u8>>,
) {
    let tag = mode.tag();
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let mut utterance_counter = 0;
    let mut reconnect_count = 0;

    loop {
        let Some(ws_stream) =
            connect_soniox(&live_session_id, &live_sessions, &tag, reconnect_count).await
        else {
            return;
        };

        let (mut stt_sink, mut stt_stream) = ws_stream.split();
        if send_soniox_config(&mode, &tag, &mut stt_sink, &mut reconnect_count).await.is_err() {
            if reconnect_count > STT_RECONNECT_MAX {
                break;
            }
            continue;
        }

        let send_task = spawn_audio_forwarder(audio_rx.clone(), stt_sink);
        let recv_task = spawn_response_processor(
            &live_session_id,
            &live_sessions,
            mode.clone(),
            tag.clone(),
            utterance_counter,
            stt_stream,
        );

        let disconnected_unexpectedly = tokio::select! {
            _ = send_task => false,
            result = recv_task => {
                if let Ok((next_counter, disconnected)) = result {
                    utterance_counter = next_counter;
                    disconnected
                } else {
                    true
                }
            },
        };

        if !disconnected_unexpectedly || !live_sessions.contains_key(&live_session_id) {
            break;
        }

        reconnect_count += 1;
        if reconnect_count > STT_RECONNECT_MAX {
            eprintln!("[STT {}] exceeded max reconnects", tag);
            break;
        }
        tokio::time::sleep(STT_RECONNECT_DELAY).await;
    }
}

async fn connect_soniox(
    live_session_id: &str,
    live_sessions: &LiveSessions,
    tag: &str,
    reconnect_count: u32,
) -> Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>> {
    let max_attempts = if reconnect_count == 0 { 10 } else { STT_RECONNECT_MAX };

    for attempt in 1..=max_attempts {
        if !live_sessions.contains_key(live_session_id) {
            eprintln!("[STT {}] live session gone, stopping", tag);
            return None;
        }

        match tokio_tungstenite::connect_async(SONIOX_WS_URL).await {
            Ok((stream, _)) => return Some(stream),
            Err(error) => {
                eprintln!(
                    "[STT {}] connect attempt {}/{} failed: {}",
                    tag, attempt, max_attempts, error
                );
                let delay = if reconnect_count == 0 {
                    Duration::from_secs(3)
                } else {
                    STT_RECONNECT_DELAY
                };
                tokio::time::sleep(delay).await;
            }
        }
    }

    eprintln!("[STT {}] giving up after {} attempts", tag, max_attempts);
    None
}

async fn send_soniox_config(
    mode: &SonioxMode,
    tag: &str,
    stt_sink: &mut futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tungstenite::Message,
    >,
    reconnect_count: &mut u32,
) -> Result<(), ()> {
    let config = mode.build_config(&SONIOX_API_KEY);
    let config_json = serde_json::to_string(&config).map_err(|error| {
        eprintln!("[STT {}] config serialize error: {}", tag, error);
    })?;

    stt_sink
        .send(tungstenite::Message::Text(config_json.into()))
        .await
        .map_err(|error| {
            eprintln!("[STT {}] config send failed: {}", tag, error);
            *reconnect_count += 1;
        })
}

fn spawn_audio_forwarder(
    audio_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<Vec<u8>>>>,
    mut stt_sink: futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tungstenite::Message,
    >,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut audio_rx = audio_rx.lock().await;
        while let Some(data) = audio_rx.recv().await {
            if stt_sink
                .send(tungstenite::Message::Binary(data.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    })
}

fn spawn_response_processor(
    live_session_id: &str,
    live_sessions: &LiveSessions,
    mode: SonioxMode,
    tag: String,
    utterance_counter: u64,
    mut stt_stream: futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> tokio::task::JoinHandle<(u64, bool)> {
    let live_session_id = live_session_id.to_string();
    let live_sessions = live_sessions.clone();
    tokio::spawn(async move {
        let mut final_text = String::new();
        let mut utterance_counter = utterance_counter;

        while let Some(message) = read_soniox_message(&tag, &mut stt_stream).await {
            let Some(response) = parse_soniox_response(&tag, &message) else {
                continue;
            };
            if response.error_code.is_some() {
                return (utterance_counter, true);
            }
            if !live_sessions.contains_key(&live_session_id) {
                return (utterance_counter, false);
            }

            let (interim_tail, endpoint_hit) = accumulate_tokens(&mode, &response, &mut final_text);
            emit_interim_if_needed(
                &mode,
                &live_sessions,
                &live_session_id,
                &final_text,
                &interim_tail,
            );

            if endpoint_hit {
                utterance_counter = finalize_utterance_if_needed(
                    &mode,
                    utterance_counter,
                    &live_sessions,
                    &live_session_id,
                    &mut final_text,
                )
                .await;
            }
        }

        (utterance_counter, true)
    })
}

async fn read_soniox_message(
    tag: &str,
    stt_stream: &mut futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Option<String> {
    let message = stt_stream.next().await?;
    let message = match message {
        Ok(message) => message,
        Err(error) => {
            eprintln!("[STT {}] WebSocket read error: {}", tag, error);
            return None;
        }
    };

    match message {
        tungstenite::Message::Text(text) => Some(text.to_string()),
        tungstenite::Message::Close(_) => {
            eprintln!("[STT {}] Soniox closed the connection", tag);
            None
        }
        _ => Some(String::new()),
    }
}

fn parse_soniox_response(tag: &str, text: &str) -> Option<SonioxResponse> {
    if text.is_empty() {
        return None;
    }

    match serde_json::from_str::<SonioxResponse>(text) {
        Ok(response) => {
            if let Some(code) = &response.error_code {
                eprintln!(
                    "[STT {}] Soniox error {}: {}",
                    tag,
                    code,
                    response.error_message.clone().unwrap_or_default()
                );
            }
            Some(response)
        }
        Err(error) => {
            eprintln!("[STT {}] parse error: {} — raw: {}", tag, error, text);
            None
        }
    }
}

fn accumulate_tokens(
    mode: &SonioxMode,
    response: &SonioxResponse,
    final_text: &mut String,
) -> (String, bool) {
    let mut interim_tail = String::new();
    let mut endpoint_hit = false;

    for token in &response.tokens {
        if !mode.accepts(token) {
            continue;
        }
        if token.text == SONIOX_END_TOKEN {
            endpoint_hit = true;
            continue;
        }
        if token.is_final {
            final_text.push_str(&token.text);
        } else {
            interim_tail.push_str(&token.text);
        }
    }

    (interim_tail, endpoint_hit)
}

fn emit_interim_if_needed(
    mode: &SonioxMode,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    final_text: &str,
    interim_tail: &str,
) {
    if !matches!(mode, SonioxMode::Source { .. }) {
        return;
    }

    let interim = format!("{}{}", final_text, interim_tail);
    if interim.is_empty() {
        return;
    }

    if let Some(live_session) = live_sessions.get(live_session_id) {
        live_session.send_to_host(to_ws(&ServerMsg::Interim {
            transcript: interim,
        }));
    }
}

async fn finalize_utterance_if_needed(
    mode: &SonioxMode,
    utterance_counter: u64,
    live_sessions: &LiveSessions,
    live_session_id: &str,
    final_text: &mut String,
) -> u64 {
    if final_text.trim().is_empty() {
        final_text.clear();
        return utterance_counter;
    }

    let next_utterance_id = utterance_counter + 1;
    let committed = std::mem::take(final_text);
    match mode {
        SonioxMode::Source { .. } => emit_final_source(
            &committed,
            next_utterance_id,
            live_sessions,
            live_session_id,
        ),
        SonioxMode::Translate { target_lang, .. } => {
            emit_translation(
                &committed,
                next_utterance_id,
                target_lang,
                live_sessions,
                live_session_id,
            )
            .await;
        }
    }

    next_utterance_id
}

fn emit_final_source(
    committed: &str,
    utterance_id: u64,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    if let Some(live_session) = live_sessions.get(live_session_id) {
        live_session.send_to_host(to_ws(&ServerMsg::Final {
            transcript: committed.to_string(),
            utterance_id,
        }));
    }
}

async fn emit_translation(
    committed: &str,
    utterance_id: u64,
    target_lang: &Lang,
    live_sessions: &LiveSessions,
    live_session_id: &str,
) {
    let selected_voice_id = live_sessions
        .get(live_session_id)
        .and_then(|session| session.selected_voice_id.clone());
    let rtmp_manager = live_sessions
        .get(live_session_id)
        .and_then(|session| session.rtmp_manager.clone());

    if let Some(live_session) = live_sessions.get(live_session_id) {
        live_session.send_to_host(to_ws(&ServerMsg::Translation {
            text: committed.to_string(),
            utterance_id,
            target_lang: target_lang.to_string(),
            translate_ms: 0,
        }));
    }

    if let Some(manager) = rtmp_manager {
        let lang = target_lang.to_string();
        let text = committed.to_string();
        tokio::spawn(async move {
            manager.lock().await.push_caption(&lang, text);
        });
    }

    broadcast_translated_tts(
        committed,
        utterance_id,
        target_lang,
        live_sessions,
        live_session_id,
        selected_voice_id.as_deref(),
    )
    .await;
}
