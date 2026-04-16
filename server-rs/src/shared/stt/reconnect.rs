//! STT reconnection loop — N+1 Soniox connections for source + target languages.

use std::sync::Arc;
use std::time::Duration;
use futures_util::StreamExt;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite;
use tracing::{error, info};

use crate::core::config::STT_RECONNECT_DELAY_SECS;
use crate::core::types::{Lang, Sessions};

use super::state::{ExitReason, SttState, SttCarryOver, SttContext, MessageAction, WsStream};
use super::connection::{connect_soniox, ConnectSession, SonioxConfig};
use super::handler::process_soniox_message;

// ── Types ────────────────────────────────────────────────

type AudioAcc = Arc<std::sync::Mutex<Vec<Vec<u8>>>>;
type WsSink = Arc<tokio::sync::Mutex<
    futures_util::stream::SplitSink<WsStream, tungstenite::Message>,
>>;
type WsRecvStream = futures_util::stream::SplitStream<WsStream>;

const AUDIO_BROADCAST_CAPACITY: usize = 64;

struct ConnectionLoopState {
    utterance_counter: u64,
    reconnect_count: u32,
}

impl ConnectionLoopState {
    fn new() -> Self {
        Self { utterance_counter: 0, reconnect_count: 0 }
    }
}

/// Immutable parts of start_stt shared across the entire session lifetime.
struct SessionEnv {
    session_id: String,
    sessions: Sessions,
    source_lang: Lang,
    audio_acc: AudioAcc,
    stt_api_key: String,
    tts_api_key: String,
    default_voice: String,
    http_client: reqwest::Client,
}

/// Config for a single Soniox translation connection.
struct SttConnectionConfig {
    target_lang: Option<String>,
    is_transcript_provider: bool,
    session_env: Arc<SessionEnv>,
    audio_rx: broadcast::Receiver<Vec<u8>>,
}

// ── Public API ───────────────────────────────────────────

/// All inputs needed to start an STT pipeline for a session.
pub struct SttStartRequest {
    pub session_id: String,
    pub sessions: Sessions,
    pub source_lang: Lang,
    pub target_langs: Vec<Lang>,
    pub audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    pub stt_api_key: String,
    pub tts_api_key: String,
    pub default_voice: String,
    pub http_client: reqwest::Client,
}

pub async fn start_stt(req: SttStartRequest) {
    if req.stt_api_key.is_empty() {
        error!("[STT] STT_API_KEY not set, STT disabled");
        return;
    }

    let env = Arc::new(build_session_env(&req));
    let (audio_tx, _) = broadcast::channel::<Vec<u8>>(AUDIO_BROADCAST_CAPACITY);

    let mut handles = spawn_all_connections(&req, &env, &audio_tx);
    let fanout_handle = spawn_audio_fanout(req.audio_rx, audio_tx);
    handles.push(fanout_handle);

    await_all_connections(handles).await;
}

// ── Setup ────────────────────────────────────────────────

fn build_session_env(req: &SttStartRequest) -> SessionEnv {
    SessionEnv {
        session_id: req.session_id.clone(),
        sessions: req.sessions.clone(),
        source_lang: req.source_lang.clone(),
        audio_acc: Arc::new(std::sync::Mutex::new(Vec::new())),
        stt_api_key: req.stt_api_key.clone(),
        tts_api_key: req.tts_api_key.clone(),
        default_voice: req.default_voice.clone(),
        http_client: req.http_client.clone(),
    }
}

fn spawn_all_connections(
    req: &SttStartRequest,
    env: &Arc<SessionEnv>,
    audio_tx: &broadcast::Sender<Vec<u8>>,
) -> Vec<tokio::task::JoinHandle<()>> {
    let mut handles = Vec::new();
    let mut first = true;

    for lang in &req.target_langs {
        if lang == &req.source_lang {
            continue;
        }
        let cfg = SttConnectionConfig {
            target_lang: Some(lang.to_string()),
            is_transcript_provider: first,
            session_env: env.clone(),
            audio_rx: audio_tx.subscribe(),
        };
        if first {
            info!("[STT] {} designated as transcript provider", lang);
        }
        first = false;
        handles.push(tokio::spawn(run_connection_loop(cfg)));
    }

    handles
}

fn spawn_audio_fanout(
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    audio_tx: broadcast::Sender<Vec<u8>>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(audio_fanout_loop(audio_rx, audio_tx))
}

async fn audio_fanout_loop(
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    audio_tx: broadcast::Sender<Vec<u8>>,
) {
    while let Some(data) = audio_rx.recv().await {
        if audio_tx.send(data).is_err() {
            break;
        }
    }
}

async fn await_all_connections(handles: Vec<tokio::task::JoinHandle<()>>) {
    for handle in handles {
        let _ = handle.await;
    }
}

// ── Per-connection reconnect loop ────────────────────────

async fn run_connection_loop(mut cfg: SttConnectionConfig) {
    let mut loop_state = ConnectionLoopState::new();
    let reconnect_delay = Duration::from_secs(STT_RECONNECT_DELAY_SECS);
    let lang_label = cfg.target_lang.clone().unwrap_or_else(|| "source".to_string());

    loop {
        info!("[STT:{}] {} connecting (attempt {})",
            cfg.session_env.session_id, lang_label, loop_state.reconnect_count + 1);

        let result = run_one_connection(&mut cfg, &loop_state).await;
        update_loop_state(&result, &mut loop_state);

        if !should_reconnect(&cfg.session_env, &mut loop_state, &lang_label) {
            break;
        }

        info!("[STT:{}] {} reconnecting in {}s (attempt {}/{})",
            cfg.session_env.session_id, lang_label, STT_RECONNECT_DELAY_SECS,
            loop_state.reconnect_count, crate::core::config::STT_RECONNECT_MAX);

        clear_accumulator(&cfg.session_env.audio_acc);
        tokio::time::sleep(reconnect_delay).await;
    }
}

fn update_loop_state(stt_state: &Option<SttState>, loop_state: &mut ConnectionLoopState) {
    if let Some(st) = stt_state {
        loop_state.utterance_counter = st.utterance_counter;
    }
}

// ── Single connection lifecycle ──────────────────────────

async fn run_one_connection(
    cfg: &mut SttConnectionConfig,
    loop_state: &ConnectionLoopState,
) -> Option<SttState> {
    let soniox_cfg = build_soniox_config(cfg);
    let sess = ConnectSession {
        session_id: &cfg.session_env.session_id,
        sessions: &cfg.session_env.sessions,
    };
    let ws_stream: WsStream = connect_soniox(&sess, &soniox_cfg).await?;
    let (stt_sink, stt_stream) = ws_stream.split();
    let stt_sink: WsSink = Arc::new(tokio::sync::Mutex::new(stt_sink));

    let ctx = build_stt_context(&cfg.session_env);
    let send_task = spawn_send_task(&stt_sink, &cfg.session_env, &mut cfg.audio_rx);
    let recv_task = spawn_recv_task(stt_stream, ctx, loop_state, cfg.target_lang.clone(), cfg.is_transcript_provider);

    await_tasks(send_task, recv_task).await
}

fn build_soniox_config(cfg: &SttConnectionConfig) -> SonioxConfig {
    SonioxConfig {
        api_key: cfg.session_env.stt_api_key.clone(),
        source_lang: cfg.session_env.source_lang.to_string(),
        target_lang: cfg.target_lang.clone(),
        max_endpoint_delay_ms: super::config::SONIOX_MAX_ENDPOINT_DELAY_MS,
        sample_rate: crate::core::config::SAMPLE_RATE,
    }
}

fn build_stt_context(env: &SessionEnv) -> SttContext {
    SttContext {
        sessions: env.sessions.clone(),
        session_id: env.session_id.clone(),
        source_lang: env.source_lang.clone(),
        audio_acc: env.audio_acc.clone(),
        tts_api_key: env.tts_api_key.clone(),
        default_voice: env.default_voice.clone(),
        http_client: env.http_client.clone(),
    }
}

// -- Send task ------------------------------------------------

struct BroadcastForwardEnv {
    sink: WsSink,
    accumulator: AudioAcc,
    session_id: String,
}

fn spawn_send_task(
    sink: &WsSink,
    env: &SessionEnv,
    audio_rx: &mut broadcast::Receiver<Vec<u8>>,
) -> tokio::task::JoinHandle<()> {
    let fwd_env = BroadcastForwardEnv {
        sink: sink.clone(),
        accumulator: env.audio_acc.clone(),
        session_id: env.session_id.clone(),
    };
    let mut rx = audio_rx.resubscribe();
    tokio::spawn(async move {
        forward_audio_from_broadcast(&mut rx, &fwd_env).await;
    })
}

async fn forward_audio_from_broadcast(
    rx: &mut broadcast::Receiver<Vec<u8>>,
    env: &BroadcastForwardEnv,
) {
    use futures_util::SinkExt;

    let keepalive_interval = Duration::from_secs(super::config::SONIOX_KEEPALIVE_INTERVAL_SECS);
    let mut chunk_count: u64 = 0;

    loop {
        let recv_result = tokio::time::timeout(keepalive_interval, rx.recv()).await;

        match recv_result {
            Ok(Ok(data)) => {
                chunk_count += 1;
                accumulate_audio(&env.accumulator, &data);

                let mut sink = env.sink.lock().await;
                if sink.send(tungstenite::Message::Binary(data.into())).await.is_err() {
                    error!("[STT:{}] sink write error, stopping audio forward", env.session_id);
                    break;
                }
            }
            Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                tracing::warn!("[STT:{}] broadcast lagged {} frames", env.session_id, n);
            }
            Ok(Err(broadcast::error::RecvError::Closed)) => break,
            Err(_) => {
                if send_keepalive(&env.sink, &env.session_id).await.is_err() {
                    break;
                }
            }
        }
    }

    send_end_of_stream(&env.sink, &env.session_id).await;
    info!("[STT:{}] audio forward ended ({} chunks)", env.session_id, chunk_count);
}

async fn send_keepalive(sink: &WsSink, session_id: &str) -> Result<(), ()> {
    use futures_util::SinkExt;

    let msg = tungstenite::Message::Text(r#"{"type":"keepalive"}"#.to_string().into());
    let mut sink = sink.lock().await;
    if sink.send(msg).await.is_err() {
        error!("[STT:{}] keepalive send failed, stopping", session_id);
        return Err(());
    }
    Ok(())
}

async fn send_end_of_stream(sink: &WsSink, session_id: &str) {
    use futures_util::SinkExt;

    let mut sink = sink.lock().await;
    let _ = sink.send(tungstenite::Message::Binary(vec![].into())).await;
    info!("[STT:{}] sent end-of-stream", session_id);
}

fn accumulate_audio(accumulator: &AudioAcc, data: &[u8]) {
    if let Ok(mut acc) = accumulator.lock() {
        acc.push(data.to_vec());
        trim_accumulator(&mut acc);
    }
}

fn trim_accumulator(acc: &mut Vec<Vec<u8>>) {
    let max = super::config::MAX_AUDIO_ACC_BYTES;
    let total: usize = acc.iter().map(|c| c.len()).sum();
    if total <= max {
        return;
    }
    let mut excess = total - max;
    while excess > 0 && !acc.is_empty() {
        let front_len = acc[0].len();
        if front_len <= excess {
            excess -= front_len;
            acc.remove(0);
        } else {
            acc[0] = acc[0][excess..].to_vec();
            break;
        }
    }
}

// ── Recv task ────────────────────────────────────────────

fn spawn_recv_task(
    mut stt_stream: WsRecvStream,
    ctx: SttContext,
    loop_state: &ConnectionLoopState,
    target_lang: Option<String>,
    is_transcript_provider: bool,
) -> tokio::task::JoinHandle<SttState> {
    let carry = SttCarryOver {
        utterance_counter: loop_state.utterance_counter,
    };

    tokio::spawn(async move {
        let mut state = SttState::new(carry, target_lang, is_transcript_provider);
        recv_loop(&mut state, &mut stt_stream, &ctx).await;
        state
    })
}

async fn recv_loop(
    state: &mut SttState,
    stt_stream: &mut WsRecvStream,
    ctx: &SttContext,
) {
    loop {
        match stt_stream.next().await {
            Some(msg_result) => {
                match handle_ws_message(msg_result, state, ctx).await {
                    MessageAction::Continue => {}
                    MessageAction::Break => break,
                }
            }
            None => break,
        }
    }
}

async fn handle_ws_message(
    msg_result: Result<tungstenite::Message, tungstenite::Error>,
    state: &mut SttState,
    ctx: &SttContext,
) -> MessageAction {
    let msg = match msg_result {
        Ok(m) => m,
        Err(e) => {
            error!("[STT] read error: {}", e);
            send_error_to_host(ctx, &format!("STT read error: {}", e));
            state.exit_reason = ExitReason::Disconnected;
            return MessageAction::Break;
        }
    };

    let text = match extract_text(msg) {
        Some(t) => t,
        None => return MessageAction::Continue,
    };

    process_soniox_message(&text, state, ctx).await
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

fn should_reconnect(env: &SessionEnv, loop_state: &mut ConnectionLoopState, lang: &str) -> bool {
    if !env.sessions.contains_key(&env.session_id) {
        info!("[STT:{}] {} session gone, stopping reconnect", env.session_id, lang);
        return false;
    }

    loop_state.reconnect_count += 1;
    if loop_state.reconnect_count > crate::core::config::STT_RECONNECT_MAX {
        error!("[STT:{}] Exceeded max reconnects for {}", env.session_id, lang);
        increment_stt_disconnect_counter(env);
        notify_stt_disconnected(env, lang);
        return false;
    }
    true
}

fn increment_stt_disconnect_counter(env: &SessionEnv) {
    if let Some(session) = env.sessions.get(&env.session_id) {
        session.pipeline_counters.increment_stt_disconnects();
    }
}

fn notify_stt_disconnected(env: &SessionEnv, lang: &str) {
    if let Some(session) = env.sessions.get(&env.session_id) {
        session.send_to_host(
            crate::features::broadcast::data::pipeline_helpers::to_ws(
                &crate::core::types::ServerMsg::PipelineWarning {
                    kind: "stt_disconnected".to_string(),
                    lang: lang.to_string(),
                    detail: "STT exhausted all reconnect attempts".to_string(),
                    utterance_id: 0,
                },
            ),
        );
    }
}

fn send_error_to_host(ctx: &SttContext, detail: &str) {
    if let Some(session) = ctx.sessions.get(&ctx.session_id) {
        session.send_to_host(
            crate::features::broadcast::data::pipeline_helpers::to_ws(
                &crate::core::types::ServerMsg::PipelineWarning {
                    kind: "stt_error".to_string(),
                    lang: ctx.source_lang.to_string(),
                    detail: detail.to_string(),
                    utterance_id: 0,
                },
            ),
        );
    }
}

fn clear_accumulator(acc: &AudioAcc) {
    if let Ok(mut a) = acc.lock() { a.clear(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_not_trim_when_under_limit() {
        let mut acc = vec![vec![0u8; 100], vec![0u8; 200]];
        trim_accumulator(&mut acc);
        assert_eq!(acc.len(), 2);
        assert_eq!(total_bytes(&acc), 300);
    }

    #[test]
    fn should_trim_oldest_chunks_when_over_limit() {
        let max = super::super::config::MAX_AUDIO_ACC_BYTES;
        let chunk_size = max / 4;
        let mut acc = vec![vec![1u8; chunk_size]; 6]; // 150% of max
        trim_accumulator(&mut acc);
        assert!(total_bytes(&acc) <= max);
    }

    #[test]
    fn should_partially_trim_front_chunk_when_needed() {
        let max = super::super::config::MAX_AUDIO_ACC_BYTES;
        let mut acc = vec![vec![0u8; max], vec![0u8; 100]];
        trim_accumulator(&mut acc);
        assert!(total_bytes(&acc) <= max);
        assert_eq!(acc.len(), 2);
        assert_eq!(acc[1].len(), 100);
    }

    #[test]
    fn should_handle_empty_accumulator() {
        let mut acc: Vec<Vec<u8>> = vec![];
        trim_accumulator(&mut acc);
        assert!(acc.is_empty());
    }

    #[test]
    fn should_accumulate_and_trim_through_public_fn() {
        let max = super::super::config::MAX_AUDIO_ACC_BYTES;
        let accumulator: AudioAcc = Arc::new(std::sync::Mutex::new(Vec::new()));
        let chunk = vec![0u8; max / 2];

        accumulate_audio(&accumulator, &chunk);
        accumulate_audio(&accumulator, &chunk);
        accumulate_audio(&accumulator, &chunk);

        let acc = accumulator.lock().unwrap();
        assert!(total_bytes(&acc) <= max);
    }

    fn total_bytes(acc: &[Vec<u8>]) -> usize {
        acc.iter().map(|c| c.len()).sum()
    }
}
