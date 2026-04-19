use crate::features::broadcast::domain::{Lang, LiveSessionHandle, PipelineConfig};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::soniox::SonioxMode;
use super::stt_response::spawn_response_processor;
use super::stt_transport::{
    ConfigSendArgs, ConnectArgs, STT_RECONNECT_DELAY, STT_RECONNECT_MAX, connect_soniox,
    send_soniox_config, spawn_audio_forwarder,
};

/// Everything an STT pipeline needs at construction: where it lives (handle),
/// what language routing it should apply (source + targets), and the upstream
/// credentials it will talk to. Passed by value into tokio tasks.
#[derive(Clone)]
pub struct PipelineSession {
    pub handle: LiveSessionHandle,
    pub source_lang: Lang,
    pub target_langs: Vec<Lang>,
    pub config: Arc<PipelineConfig>,
}

pub async fn start_stt_pipelines(session: PipelineSession, audio_rx: mpsc::Receiver<Vec<u8>>) {
    if session.config.soniox_api_key.is_empty() {
        eprintln!("[STT] SONIOX_API_KEY not set — STT pipeline disabled");
        return;
    }

    let targets = dedupe_target_langs(session.source_lang.clone(), session.target_langs.clone());
    let (source_tx, source_rx) = mpsc::channel::<Vec<u8>>(64);
    let mut target_senders = Vec::new();
    let mut target_receivers = Vec::new();

    for lang in &targets {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
        target_senders.push(tx);
        target_receivers.push((lang.clone(), rx));
    }

    spawn_fan_out(audio_rx, source_tx, target_senders);
    spawn_source_session(&session, source_rx);
    spawn_translate_sessions(&session, target_receivers);
}

fn spawn_fan_out(
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
    source_tx: mpsc::Sender<Vec<u8>>,
    target_senders: Vec<mpsc::Sender<Vec<u8>>>,
) {
    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            let _ = source_tx.try_send(chunk.clone());
            for tx in &target_senders {
                let _ = tx.try_send(chunk.clone());
            }
        }
    });
}

fn dedupe_target_langs(source_lang: Lang, target_langs: Vec<Lang>) -> Vec<Lang> {
    let mut seen = std::collections::HashSet::new();
    target_langs
        .into_iter()
        .filter(|lang| *lang != source_lang && seen.insert(lang.clone()))
        .collect()
}

fn spawn_source_session(session: &PipelineSession, audio_rx: mpsc::Receiver<Vec<u8>>) {
    let session = session.clone();
    let mode = SonioxMode::Source {
        lang: session.source_lang.clone(),
    };
    tokio::spawn(async move {
        run_soniox_session(session, mode, audio_rx).await;
    });
}

fn spawn_translate_sessions(
    session: &PipelineSession,
    target_receivers: Vec<(Lang, mpsc::Receiver<Vec<u8>>)>,
) {
    for (target_lang, target_rx) in target_receivers {
        let session = session.clone();
        let mode = SonioxMode::Translate {
            source_lang: session.source_lang.clone(),
            target_lang,
        };
        tokio::spawn(async move {
            run_soniox_session(session, mode, target_rx).await;
        });
    }
}

async fn run_soniox_session(
    session: PipelineSession,
    mode: SonioxMode,
    audio_rx: mpsc::Receiver<Vec<u8>>,
) {
    let tag = mode.tag();
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let mut utterance_counter = 0;
    let mut reconnect_count = 0;

    loop {
        let Some(ws_stream) = connect_soniox(ConnectArgs {
            handle: &session.handle,
            tag: &tag,
            reconnect_count,
            ws_url: &session.config.soniox_ws_url,
        })
        .await
        else {
            return;
        };

        let (mut stt_sink, stt_stream) = ws_stream.split();
        if send_soniox_config(ConfigSendArgs {
            mode: &mode,
            tag: &tag,
            stt_sink: &mut stt_sink,
            reconnect_count: &mut reconnect_count,
            api_key: &session.config.soniox_api_key,
        })
        .await
        .is_err()
        {
            if reconnect_count > STT_RECONNECT_MAX {
                break;
            }
            continue;
        }

        let send_task = spawn_audio_forwarder(audio_rx.clone(), stt_sink);
        let recv_task = spawn_response_processor(super::stt_response::ProcessorArgs {
            handle: session.handle.clone(),
            mode: mode.clone(),
            tag: tag.clone(),
            utterance_counter,
            stt_stream,
        });

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

        if !disconnected_unexpectedly || !session.handle.sessions.contains_key(&session.handle.id) {
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

#[cfg(test)]
mod tests {
    use super::dedupe_target_langs;
    use crate::features::broadcast::domain::Lang;

    #[test]
    fn dedupe_drops_source_lang_from_targets() {
        let targets = dedupe_target_langs(Lang::En, vec![Lang::En, Lang::Ja, Lang::Ko]);
        assert_eq!(targets, vec![Lang::Ja, Lang::Ko]);
    }

    #[test]
    fn dedupe_collapses_duplicates_preserving_first_seen_order() {
        let targets = dedupe_target_langs(
            Lang::En,
            vec![Lang::Ja, Lang::Ko, Lang::Ja, Lang::Zh, Lang::Ko],
        );
        assert_eq!(targets, vec![Lang::Ja, Lang::Ko, Lang::Zh]);
    }

    #[test]
    fn dedupe_returns_empty_when_all_targets_match_source() {
        let targets = dedupe_target_langs(Lang::Ja, vec![Lang::Ja, Lang::Ja]);
        assert!(targets.is_empty());
    }

    #[test]
    fn dedupe_handles_empty_input() {
        assert!(dedupe_target_langs(Lang::En, vec![]).is_empty());
    }
}
