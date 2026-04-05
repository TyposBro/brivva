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

use super::state::{SttState, MessageAction};
use super::connection::connect_gladia;
use super::message_handler::process_gladia_message;
use super::audio_forwarder::forward_audio_to_gladia;

pub async fn start_stt(
    session_id: String,
    sessions: Sessions,
    source_lang: Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let audio_acc: Arc<std::sync::Mutex<Vec<Vec<u8>>>> = Arc::new(std::sync::Mutex::new(Vec::new()));

    let mut utterance_counter: u64 = 0;
    let mut reconnect_count: u32 = 0;
    let mut endpointing = DEFAULT_ENDPOINTING_SECS;
    let mut max_duration = DEFAULT_MAX_DURATION_SECS;
    let mut wpm_samples: Vec<u32> = Vec::new();
    let mut adapted = false;
    let reconnect_delay = Duration::from_secs(STT_RECONNECT_DELAY_SECS);

    if super::super::STT_API_KEY.is_empty() {
        error!("[STT] STT_API_KEY not set, STT disabled");
        return;
    }

    loop {
        let ws_stream = match connect_gladia(
            &session_id, &sessions, &source_lang, endpointing, max_duration, reconnect_count,
        ).await {
            Some(s) => s,
            None => return,
        };

        let (stt_sink, mut stt_stream) = ws_stream.split();
        let stt_sink = Arc::new(tokio::sync::Mutex::new(stt_sink));
        let sink_for_ctrl = stt_sink.clone();

        let send_task = tokio::spawn(forward_audio_to_gladia(
            audio_rx.clone(), stt_sink.clone(), audio_acc.clone(), session_id.clone(),
        ));

        let sessions_ref = sessions.clone();
        let sid = session_id.clone();
        let source_lang_clone = source_lang.clone();
        let acc_rx = audio_acc.clone();
        let lang_str = source_lang.to_string();

        let mut state = SttState::new(&lang_str, utterance_counter, wpm_samples.clone(), adapted);

        let recv_task = tokio::spawn(async move {
            while let Some(msg_result) = stt_stream.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => { error!("[STT] read error: {}", e); state.disconnected = true; break; }
                };

                let text = match msg {
                    tungstenite::Message::Text(t) => t.to_string(),
                    tungstenite::Message::Close(_) => break,
                    _ => continue,
                };

                let gm: crate::stt::GladiaMessage = match serde_json::from_str(&text) {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                match process_gladia_message(
                    gm, &mut state, &sessions_ref, &sid, &source_lang_clone, &acc_rx, &sink_for_ctrl,
                ).await {
                    MessageAction::Continue => {}
                    MessageAction::Break => break,
                }
            }
            state
        });

        let send_abort = send_task.abort_handle();
        let recv_abort = recv_task.abort_handle();
        tokio::select! {
            _ = send_task => { recv_abort.abort(); },
            result = recv_task => {
                send_abort.abort();
                if let Ok(st) = result {
                    utterance_counter = st.utterance_counter;
                    wpm_samples = st.wpm_samples;
                    adapted = st.adapted;

                    if st.needs_adaptive_reconnect {
                        if let Some((new_endp, new_max_dur)) = st.adaptive_params {
                            endpointing = new_endp;
                            max_duration = new_max_dur;
                        }
                        clear_accumulator(&audio_acc);
                        continue;
                    }

                    if !st.disconnected { break; }
                }
            },
        }

        if !sessions.contains_key(&session_id) { break; }

        reconnect_count += 1;
        if reconnect_count > crate::constants::STT_RECONNECT_MAX {
            error!("[STT] Exceeded max reconnects, giving up");
            break;
        }
        clear_accumulator(&audio_acc);
        tokio::time::sleep(reconnect_delay).await;
    }
}

fn clear_accumulator(acc: &Arc<std::sync::Mutex<Vec<Vec<u8>>>>) {
    if let Ok(mut a) = acc.lock() { a.clear(); }
}
