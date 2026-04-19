use crate::features::broadcast::domain::{Lang, LiveSessions, PipelineConfig};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::soniox::SonioxMode;
use super::stt_response::spawn_response_processor;
use super::stt_transport::{
    STT_RECONNECT_DELAY, STT_RECONNECT_MAX, connect_soniox, send_soniox_config,
    spawn_audio_forwarder,
};

pub async fn start_stt_pipelines(
    live_session_id: String,
    live_sessions: LiveSessions,
    source_lang: Lang,
    target_langs: Vec<Lang>,
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
    config: Arc<PipelineConfig>,
) {
    if config.soniox_api_key.is_empty() {
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

    spawn_source_session(
        &live_session_id,
        &live_sessions,
        source_lang.clone(),
        source_rx,
        config.clone(),
    );
    spawn_translate_sessions(
        &live_session_id,
        &live_sessions,
        source_lang,
        target_receivers,
        config,
    );
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
    config: Arc<PipelineConfig>,
) {
    let live_session_id = live_session_id.to_string();
    let live_sessions = live_sessions.clone();
    tokio::spawn(async move {
        run_soniox_session(
            live_session_id,
            live_sessions,
            SonioxMode::Source { lang: source_lang },
            audio_rx,
            config,
        )
        .await;
    });
}

fn spawn_translate_sessions(
    live_session_id: &str,
    live_sessions: &LiveSessions,
    source_lang: Lang,
    target_receivers: Vec<(Lang, mpsc::Receiver<Vec<u8>>)>,
    config: Arc<PipelineConfig>,
) {
    for (target_lang, target_rx) in target_receivers {
        let live_session_id = live_session_id.to_string();
        let live_sessions = live_sessions.clone();
        let source_lang = source_lang.clone();
        let config = config.clone();
        tokio::spawn(async move {
            run_soniox_session(
                live_session_id,
                live_sessions,
                SonioxMode::Translate {
                    source_lang,
                    target_lang,
                },
                target_rx,
                config,
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
    config: Arc<PipelineConfig>,
) {
    let tag = mode.tag();
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let mut utterance_counter = 0;
    let mut reconnect_count = 0;

    loop {
        let Some(ws_stream) = connect_soniox(
            &live_session_id,
            &live_sessions,
            &tag,
            reconnect_count,
            &config.soniox_ws_url,
        )
        .await
        else {
            return;
        };

        let (mut stt_sink, stt_stream) = ws_stream.split();
        if send_soniox_config(
            &mode,
            &tag,
            &mut stt_sink,
            &mut reconnect_count,
            &config.soniox_api_key,
        )
        .await
        .is_err()
        {
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
        let targets =
            dedupe_target_langs(Lang::En, vec![Lang::Ja, Lang::Ko, Lang::Ja, Lang::Zh, Lang::Ko]);
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
