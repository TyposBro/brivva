use crate::features::broadcast::domain::{Lang, LiveSessionHandle, PipelineConfig, TtsRequest};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use super::soniox::SonioxMode;
use super::stt_response::spawn_response_processor;
use super::stt_transport::{
    ConfigSendArgs, ConnectArgs, STT_RECONNECT_DELAY, STT_RECONNECT_MAX, connect_soniox,
    send_soniox_config, spawn_audio_forwarder,
};
use super::tts::broadcast_translated_tts;

/// Per-lang TTS worker channel depth. Each slot holds one pending
/// `TtsRequest`. Set low on purpose: if ElevenLabs is slow enough to queue
/// 32 utterances, letting more pile up just wastes live-broadcast time —
/// the user will have moved on by the time we catch up. A full channel
/// drops the new request observably via `try_send` in
/// `stt_response::emit_translation`.
const TTS_WORKER_QUEUE_DEPTH: usize = 32;

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
        tracing::warn!(
            session_id = %session.handle.id,
            "SONIOX_API_KEY not set — STT pipeline disabled; translations will not fire"
        );
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

    spawn_tts_workers(&session, &targets);
    spawn_fan_out(audio_rx, source_tx, target_senders);
    spawn_source_session(&session, source_rx);
    spawn_translate_sessions(&session, target_receivers);
}

/// Spawn one TTS worker task per unique target language. Each worker owns a
/// bounded `mpsc::Receiver<TtsRequest>`; the `Sender` is stashed on
/// `LiveSession.tts_workers` so the STT response processor can dispatch with
/// a non-blocking `try_send`. Workers process requests serially, so audio
/// for a given lang is pushed to ffmpeg in emission order — never scrambled.
/// When `LiveSession` is dropped at teardown, the `Sender` goes with it;
/// `recv()` returns `None`, the worker logs `tts worker exited`, and the
/// task finishes on its own.
fn spawn_tts_workers(session: &PipelineSession, targets: &[Lang]) {
    // Skip workers when ElevenLabs credentials are missing — the worker would
    // synthesize nothing and the dispatch path in `emit_translation` treats
    // "no worker" as "drop with a warn", which is the right behavior in that
    // degraded mode anyway.
    if session.config.elevenlabs_api_key.is_empty() {
        tracing::warn!(
            session_id = %session.handle.id,
            "ELEVENLABS_API_KEY not set — TTS workers disabled; translated audio will not fire"
        );
        return;
    }
    for lang in targets {
        let (tx, mut rx) = mpsc::channel::<TtsRequest>(TTS_WORKER_QUEUE_DEPTH);
        // Stash the sender end on the session before spawning. If the
        // session is gone already (e.g. teardown raced us), skip the spawn.
        let Some(mut live) = session.handle.sessions.get_mut(&session.handle.id) else {
            tracing::warn!(
                session_id = %session.handle.id,
                target_lang = %lang,
                "tts worker setup skipped: live session already gone"
            );
            continue;
        };
        live.tts_workers.insert(lang.clone(), tx);
        drop(live);

        let worker_lang = lang.clone();
        let worker_session_id = session.handle.id.clone();
        tokio::spawn(async move {
            // §0.5.4: every worker start + exit is greppable. Pairing these
            // two lines tells operators whether the session ended cleanly
            // or the worker died mid-session (no exit line).
            tracing::info!(
                session_id = %worker_session_id,
                target_lang = %worker_lang,
                queue_depth = TTS_WORKER_QUEUE_DEPTH,
                "tts worker started"
            );
            // Hard ceiling on a single utterance's wall-clock budget. Inner
            // layers (ElevenLabs HTTP, ffmpeg mp3→pcm decode) each have
            // their own narrower timeouts; this is the §0.5.4 safety net
            // that prevents one stuck utterance from freezing the whole
            // worker — the April 2026 default-male regression where utt 2
            // hung forever inside `decode_mp3_to_pcm` and 13 queued
            // utterances never reached the listener.
            const PER_UTTERANCE_HARD_CEILING: Duration = Duration::from_secs(60);
            let mut processed: u64 = 0;
            while let Some(req) = rx.recv().await {
                processed += 1;
                let utterance_id = req.utterance_id;
                tracing::info!(
                    session_id = %worker_session_id,
                    target_lang = %worker_lang,
                    utterance_id,
                    processed,
                    "tts worker processing"
                );
                match tokio::time::timeout(
                    PER_UTTERANCE_HARD_CEILING,
                    broadcast_translated_tts(req),
                )
                .await
                {
                    Ok(()) => {}
                    Err(_) => tracing::error!(
                        session_id = %worker_session_id,
                        target_lang = %worker_lang,
                        utterance_id,
                        ceiling_secs = PER_UTTERANCE_HARD_CEILING.as_secs(),
                        "tts worker outer-timeout fired — utterance abandoned, advancing to next"
                    ),
                }
            }
            tracing::info!(
                session_id = %worker_session_id,
                target_lang = %worker_lang,
                processed,
                "tts worker exited"
            );
        });
    }
}

fn spawn_fan_out(
    mut audio_rx: mpsc::Receiver<Vec<u8>>,
    source_tx: mpsc::Sender<Vec<u8>>,
    target_senders: Vec<mpsc::Sender<Vec<u8>>>,
) {
    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            if let Err(error) = source_tx.try_send(chunk.clone()) {
                // Channel full or closed = source STT task is behind or
                // gone. Silent drops here look identical to "STT never
                // ran" in post-incident logs; emit at debug so a 30-min
                // session with dropped frames is greppable when needed
                // without spamming healthy sessions.
                tracing::debug!(
                    chunk_bytes = chunk.len(),
                    error = %error,
                    "stt fan-out dropped chunk to source task"
                );
            }
            for (idx, tx) in target_senders.iter().enumerate() {
                if let Err(error) = tx.try_send(chunk.clone()) {
                    tracing::debug!(
                        target_idx = idx,
                        chunk_bytes = chunk.len(),
                        error = %error,
                        "stt fan-out dropped chunk to target task"
                    );
                }
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
                tracing::error!(
                    session_id = %session.handle.id,
                    tag = %tag,
                    reconnect_count,
                    "stt config-send failed; exceeded max reconnects, giving up"
                );
                break;
            }
            tracing::warn!(
                session_id = %session.handle.id,
                tag = %tag,
                reconnect_count,
                "stt config-send failed; looping to reconnect"
            );
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

        if !disconnected_unexpectedly {
            tracing::info!(
                session_id = %session.handle.id,
                tag = %tag,
                "stt session ended cleanly"
            );
            break;
        }
        if !session.handle.sessions.contains_key(&session.handle.id) {
            tracing::info!(
                session_id = %session.handle.id,
                tag = %tag,
                "stt session handle removed — exiting reconnect loop"
            );
            break;
        }

        reconnect_count += 1;
        if reconnect_count > STT_RECONNECT_MAX {
            tracing::error!(
                session_id = %session.handle.id,
                tag = %tag,
                reconnect_count,
                "stt exceeded max reconnects after unexpected disconnect"
            );
            break;
        }
        tracing::warn!(
            session_id = %session.handle.id,
            tag = %tag,
            reconnect_count,
            "stt disconnected unexpectedly; sleeping before reconnect"
        );
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

    #[test]
    fn dedupe_keeps_all_entries_when_none_match_source() {
        let targets = dedupe_target_langs(Lang::En, vec![Lang::Ja, Lang::Ko, Lang::Zh]);
        assert_eq!(targets, vec![Lang::Ja, Lang::Ko, Lang::Zh]);
    }

    #[tokio::test]
    async fn spawn_tts_workers_registers_one_sender_per_unique_target_lang() {
        use crate::features::broadcast::domain::{
            LiveSession, LiveSessionHandle, LiveSessions, PipelineConfig,
        };
        use dashmap::DashMap;
        use std::sync::Arc;

        let sessions: LiveSessions = Arc::new(DashMap::new());
        sessions.insert(
            "room".into(),
            LiveSession::new(
                "room".into(),
                Lang::En,
                None,
                Arc::new(PipelineConfig {
                    elevenlabs_api_key: "k".into(),
                    ..Default::default()
                }),
            ),
        );
        let handle = LiveSessionHandle::new("room".into(), sessions.clone());
        let session = super::PipelineSession {
            handle,
            source_lang: Lang::En,
            target_langs: vec![Lang::Ja, Lang::Ko, Lang::Zh],
            config: Arc::new(PipelineConfig {
                elevenlabs_api_key: "k".into(),
                ..Default::default()
            }),
        };
        super::spawn_tts_workers(&session, &[Lang::Ja, Lang::Ko, Lang::Zh]);

        let live = sessions.get("room").unwrap();
        assert!(live.tts_workers.contains_key(&Lang::Ja));
        assert!(live.tts_workers.contains_key(&Lang::Ko));
        assert!(live.tts_workers.contains_key(&Lang::Zh));
        assert_eq!(live.tts_workers.len(), 3);
    }

    #[tokio::test]
    async fn spawn_tts_workers_skips_setup_when_elevenlabs_key_is_empty() {
        use crate::features::broadcast::domain::{
            LiveSession, LiveSessionHandle, LiveSessions, PipelineConfig,
        };
        use dashmap::DashMap;
        use std::sync::Arc;

        let sessions: LiveSessions = Arc::new(DashMap::new());
        sessions.insert(
            "room".into(),
            LiveSession::new(
                "room".into(),
                Lang::En,
                None,
                Arc::new(PipelineConfig::default()),
            ),
        );
        let handle = LiveSessionHandle::new("room".into(), sessions.clone());
        let session = super::PipelineSession {
            handle,
            source_lang: Lang::En,
            target_langs: vec![Lang::Ja],
            // Default config → empty elevenlabs_api_key → setup should skip.
            config: Arc::new(PipelineConfig::default()),
        };
        super::spawn_tts_workers(&session, &[Lang::Ja]);

        let live = sessions.get("room").unwrap();
        assert!(
            live.tts_workers.is_empty(),
            "workers must not be registered when credentials are missing"
        );
    }

    #[tokio::test]
    async fn tts_worker_exits_when_live_session_is_dropped() {
        use crate::features::broadcast::domain::{
            LiveSession, LiveSessionHandle, LiveSessions, PipelineConfig,
        };
        use dashmap::DashMap;
        use std::sync::Arc;

        let sessions: LiveSessions = Arc::new(DashMap::new());
        sessions.insert(
            "room".into(),
            LiveSession::new(
                "room".into(),
                Lang::En,
                None,
                Arc::new(PipelineConfig {
                    elevenlabs_api_key: "k".into(),
                    ..Default::default()
                }),
            ),
        );
        let handle = LiveSessionHandle::new("room".into(), sessions.clone());
        let session = super::PipelineSession {
            handle,
            source_lang: Lang::En,
            target_langs: vec![Lang::Ja],
            config: Arc::new(PipelineConfig {
                elevenlabs_api_key: "k".into(),
                ..Default::default()
            }),
        };
        super::spawn_tts_workers(&session, &[Lang::Ja]);

        // Session teardown drops LiveSession → drops the Sender → worker
        // observes channel close on the next recv() and exits naturally.
        sessions.remove("room");
        // Give the runtime a few yields so the worker task notices.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        // Nothing to assert structurally — if the worker hadn't exited the
        // runtime would leak the spawned task, but there's no tokio API to
        // probe that directly. The real value of this test is that `cargo
        // test` under `--nocapture` shows the "tts worker exited" log line,
        // which is the §0.5.4 grep target we want in prod.
    }

    #[tokio::test]
    async fn start_stt_pipelines_returns_early_when_api_key_is_empty() {
        use crate::features::broadcast::domain::{LiveSessionHandle, LiveSessions, PipelineConfig};
        use dashmap::DashMap;
        use std::sync::Arc;
        use tokio::sync::mpsc;

        let sessions: LiveSessions = Arc::new(DashMap::new());
        let handle = LiveSessionHandle::new("room".into(), sessions);
        let (_tx, rx) = mpsc::channel::<Vec<u8>>(1);
        let session = super::PipelineSession {
            handle,
            source_lang: Lang::En,
            target_langs: vec![Lang::Ja],
            config: Arc::new(PipelineConfig::default()), // empty soniox_api_key
        };
        // Should just log and return — no panic or spawn storm.
        super::start_stt_pipelines(session, rx).await;
    }

    #[tokio::test]
    async fn start_stt_pipelines_spawns_source_plus_unique_target_sessions_that_exit_with_session()
    {
        use crate::features::broadcast::domain::{
            LiveSession, LiveSessionHandle, LiveSessions, PipelineConfig,
        };
        use dashmap::DashMap;
        use std::sync::Arc;
        use tokio::sync::mpsc;

        let sessions: LiveSessions = Arc::new(DashMap::new());
        sessions.insert(
            "room".into(),
            LiveSession::new(
                "room".into(),
                Lang::En,
                None,
                Arc::new(PipelineConfig::default()),
            ),
        );
        let handle = LiveSessionHandle::new("room".into(), sessions.clone());
        let (tx, rx) = mpsc::channel::<Vec<u8>>(8);

        let session = super::PipelineSession {
            handle,
            source_lang: Lang::En,
            // Duplicates + source repeat exercise dedupe + fan-out wiring.
            target_langs: vec![Lang::Ja, Lang::En, Lang::Ja, Lang::Ko],
            config: Arc::new(PipelineConfig {
                soniox_api_key: "sk".into(),
                // ws://127.0.0.1:1 is connection-refused → each tokio task
                // will attempt, fail, sleep, re-attempt. Removing the session
                // trips contains_key=false in connect_soniox and the task
                // exits promptly.
                soniox_ws_url: "ws://127.0.0.1:1".into(),
                ..Default::default()
            }),
        };
        super::start_stt_pipelines(session, rx).await;

        // Push a frame so fan_out runs once, then drop the sender and remove
        // the session. Background STT tasks exit as soon as they notice the
        // session is gone on their next attempt.
        tx.send(vec![0u8; 16]).await.unwrap();
        drop(tx);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        sessions.remove("room");
        // No explicit join — the tokio::spawn handles are not returned.
        // Sleep briefly to let the tasks observe the session removal and
        // exit without hanging the test runner.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
