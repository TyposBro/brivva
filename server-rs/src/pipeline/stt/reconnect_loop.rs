//! STT reconnection loop — the main entry point for the STT pipeline.

use std::sync::Arc;
use std::time::Duration;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tracing::error;

use crate::constants::STT_RECONNECT_DELAY_SECS;
use crate::stt::config::{DEFAULT_ENDPOINTING_SECS, DEFAULT_MAX_DURATION_SECS};
use crate::types::{Lang, Sessions};

use super::state::{ExitReason, SttState, MessageAction, WsStream};
use super::connection::connect_gladia;
use super::message_handler::process_gladia_message;
use super::audio_forwarder::forward_audio_to_gladia;

// ── Types ────────────────────────────────────────────────

type AudioAcc = Arc<std::sync::Mutex<Vec<Vec<u8>>>>;
type AudioRx = Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>;
type WsSink = Arc<tokio::sync::Mutex<
    futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
>>;
type WsRecvStream = futures_util::stream::SplitStream<WsStream>;

struct LoopState {
    utterance_counter: u64,
    reconnect_count: u32,
    endpointing: f64,
    max_duration: f64,
    wpm_samples: Vec<u32>,
    adapted: bool,
}

impl LoopState {
    fn new() -> Self {
        Self {
            utterance_counter: 0,
            reconnect_count: 0,
            endpointing: DEFAULT_ENDPOINTING_SECS,
            max_duration: DEFAULT_MAX_DURATION_SECS,
            wpm_samples: Vec::new(),
            adapted: false,
        }
    }
}

enum ReconnectDecision {
    AdaptiveReconnect,
    StandardReconnect,
    Stop,
}

// ── Public API ───────────────────────────────────────────

pub async fn start_stt(
    session_id: String,
    sessions: Sessions,
    source_lang: Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    if super::super::STT_API_KEY.is_empty() {
        error!("[STT] STT_API_KEY not set, STT disabled");
        return;
    }

    let audio_rx: AudioRx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let audio_acc: AudioAcc = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut state = LoopState::new();
    let reconnect_delay = Duration::from_secs(STT_RECONNECT_DELAY_SECS);

    loop {
        let result = run_one_connection(
            &session_id, &sessions, &source_lang, &audio_rx, &audio_acc, &state,
        ).await;

        match handle_connection_result(result, &mut state, &audio_acc) {
            ReconnectDecision::AdaptiveReconnect => continue,
            ReconnectDecision::Stop => break,
            ReconnectDecision::StandardReconnect => {}
        }

        if !should_reconnect(&session_id, &sessions, &mut state) {
            break;
        }
        clear_accumulator(&audio_acc);
        tokio::time::sleep(reconnect_delay).await;
    }
}

// ── Connection lifecycle ─────────────────────────────────

async fn run_one_connection(
    session_id: &str,
    sessions: &Sessions,
    source_lang: &Lang,
    audio_rx: &AudioRx,
    audio_acc: &AudioAcc,
    loop_state: &LoopState,
) -> Option<SttState> {
    let ws_stream = connect_gladia(
        session_id, sessions, source_lang,
        loop_state.endpointing, loop_state.max_duration, loop_state.reconnect_count,
    ).await?;

    let (stt_sink, stt_stream) = ws_stream.split();
    let stt_sink: WsSink = Arc::new(tokio::sync::Mutex::new(stt_sink));

    let send_task = spawn_send_task(audio_rx, &stt_sink, audio_acc, session_id);
    let recv_task = spawn_recv_task(
        stt_stream, stt_sink.clone(), sessions, session_id, source_lang, audio_acc, loop_state,
    );

    await_tasks(send_task, recv_task).await
}

fn spawn_send_task(
    audio_rx: &AudioRx,
    stt_sink: &WsSink,
    audio_acc: &AudioAcc,
    session_id: &str,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(forward_audio_to_gladia(
        audio_rx.clone(), stt_sink.clone(), audio_acc.clone(), session_id.to_string(),
    ))
}

fn spawn_recv_task(
    mut stt_stream: WsRecvStream,
    sink_for_ctrl: WsSink,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    audio_acc: &AudioAcc,
    loop_state: &LoopState,
) -> tokio::task::JoinHandle<SttState> {
    let sessions_ref = sessions.clone();
    let sid = session_id.to_string();
    let source_lang_clone = source_lang.clone();
    let acc_rx = audio_acc.clone();
    let lang_str = source_lang.to_string();
    let utterance_counter = loop_state.utterance_counter;
    let wpm_samples = loop_state.wpm_samples.clone();
    let adapted = loop_state.adapted;

    tokio::spawn(async move {
        let mut state = SttState::new(&lang_str, utterance_counter, wpm_samples, adapted);
        recv_loop(&mut state, &mut stt_stream, &sessions_ref, &sid, &source_lang_clone, &acc_rx, &sink_for_ctrl).await;
        state
    })
}

async fn recv_loop(
    state: &mut SttState,
    stt_stream: &mut WsRecvStream,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    audio_acc: &AudioAcc,
    sink: &WsSink,
) {
    while let Some(msg_result) = stt_stream.next().await {
        match handle_ws_message(msg_result, state, sessions, session_id, source_lang, audio_acc, sink).await {
            MessageAction::Continue => {}
            MessageAction::Break => break,
        }
    }
}

async fn handle_ws_message(
    msg_result: Result<tungstenite::Message, tungstenite::Error>,
    state: &mut SttState,
    sessions: &Sessions,
    session_id: &str,
    source_lang: &Lang,
    audio_acc: &AudioAcc,
    sink: &WsSink,
) -> MessageAction {
    let msg = match msg_result {
        Ok(m) => m,
        Err(e) => { error!("[STT] read error: {}", e); state.exit_reason = ExitReason::Disconnected; return MessageAction::Break; }
    };

    let text = match extract_text(msg) {
        Some(t) => t,
        None => return MessageAction::Continue,
    };

    let gm: crate::stt::GladiaMessage = match serde_json::from_str(&text) {
        Ok(d) => d,
        Err(_) => return MessageAction::Continue,
    };

    process_gladia_message(gm, state, sessions, session_id, source_lang, audio_acc, sink).await
}

fn extract_text(msg: tungstenite::Message) -> Option<String> {
    match msg {
        tungstenite::Message::Text(t) => Some(t.to_string()),
        tungstenite::Message::Close(_) => None,
        _ => None,
    }
}

async fn await_tasks(
    send_task: tokio::task::JoinHandle<()>,
    recv_task: tokio::task::JoinHandle<SttState>,
) -> Option<SttState> {
    let send_abort = send_task.abort_handle();
    let recv_abort = recv_task.abort_handle();
    tokio::select! {
        _ = send_task => { recv_abort.abort(); None },
        result = recv_task => { send_abort.abort(); result.ok() },
    }
}

// ── Reconnection logic ──────────────────────────────────

fn handle_connection_result(
    stt_state: Option<SttState>,
    loop_state: &mut LoopState,
    audio_acc: &AudioAcc,
) -> ReconnectDecision {
    let st = match stt_state {
        Some(st) => st,
        None => return ReconnectDecision::StandardReconnect,
    };

    apply_stt_state(&st, loop_state);

    match &st.exit_reason {
        ExitReason::AdaptiveReconnect { endpointing, max_duration } => {
            loop_state.endpointing = *endpointing;
            loop_state.max_duration = *max_duration;
            clear_accumulator(audio_acc);
            ReconnectDecision::AdaptiveReconnect
        }
        ExitReason::Disconnected => ReconnectDecision::StandardReconnect,
        ExitReason::Running => ReconnectDecision::Stop,
    }
}

fn apply_stt_state(st: &SttState, loop_state: &mut LoopState) {
    loop_state.utterance_counter = st.utterance_counter;
    loop_state.wpm_samples = st.wpm_samples.clone();
    loop_state.adapted = st.adapted;
}

fn should_reconnect(session_id: &str, sessions: &Sessions, loop_state: &mut LoopState) -> bool {
    if !sessions.contains_key(session_id) {
        return false;
    }

    loop_state.reconnect_count += 1;
    if loop_state.reconnect_count > crate::constants::STT_RECONNECT_MAX {
        error!("[STT] Exceeded max reconnects, giving up");
        return false;
    }
    true
}

fn clear_accumulator(acc: &AudioAcc) {
    if let Ok(mut a) = acc.lock() { a.clear(); }
}
