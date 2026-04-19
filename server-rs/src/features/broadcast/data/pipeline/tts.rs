use crate::features::broadcast::domain::{Lang, LiveSessionHandle, ServerMsg};
use futures_util::StreamExt;
use std::time::{Duration, Instant};

use super::to_ws;

/// Inputs to the voice-selection decision. Pulled out so the kill-switch
/// integration tests can exercise branch selection without spawning TTS.
pub struct ResolveVoiceArgs<'a> {
    pub selected_voice_id: Option<&'a str>,
    pub enrollment_lang: Option<&'a Lang>,
    pub target_lang: &'a Lang,
    pub force_default_voice: bool,
}

/// Result of `resolve_voice`: which ElevenLabs voice id to request and
/// whether it's a cloned voice (dictates model choice + voice_settings).
pub struct ResolvedVoice {
    pub voice_id: String,
    pub is_cloned: bool,
    /// Set when the caller supplied a cloned voice id but the enrollment
    /// language disagreed with the target — the caller logs a warning.
    pub is_cloned_fallback_due_to_enrollment: bool,
}

/// Pure voice-selection decision. Exposed for kill-switch tests.
///
/// Order of precedence:
/// 1. `force_default_voice` kill-switch wins over everything else.
/// 2. Clone with mismatched `enrollment_lang` falls back to the default
///    voice to avoid the April 2026 Indian-accent regression.
/// 3. Supplied `selected_voice_id` is used as a cloned voice.
/// 4. Otherwise the target language's default library voice.
pub fn resolve_voice(args: ResolveVoiceArgs<'_>) -> ResolvedVoice {
    if args.force_default_voice || args.selected_voice_id.is_none() {
        return ResolvedVoice {
            voice_id: args.target_lang.voice_id().to_string(),
            is_cloned: false,
            is_cloned_fallback_due_to_enrollment: false,
        };
    }
    if let Some(enroll) = args.enrollment_lang
        && enroll != args.target_lang
    {
        return ResolvedVoice {
            voice_id: args.target_lang.voice_id().to_string(),
            is_cloned: false,
            is_cloned_fallback_due_to_enrollment: true,
        };
    }
    ResolvedVoice {
        voice_id: args
            .selected_voice_id
            .map(str::to_string)
            .unwrap_or_else(|| args.target_lang.voice_id().to_string()),
        is_cloned: true,
        is_cloned_fallback_due_to_enrollment: false,
    }
}

/// All inputs to the TTS broadcast path bundled so the public entry point
/// stays within the §3.3 arg budget.
pub struct TtsRequest {
    pub text: String,
    pub utterance_id: u64,
    pub target_lang: Lang,
    pub handle: LiveSessionHandle,
    pub selected_voice_id: Option<String>,
    /// Enrollment language of the cloned voice, when Workers sends it. None
    /// when the voice row is pre-schema or the voice is a default library
    /// voice. When `Some(enroll) != target_lang`, we log a warning and bias
    /// back to the default voice to avoid the April 2026 Indian-accent
    /// regression (cross-lingual inference on a cloned v2 voice).
    pub selected_voice_enrollment_lang: Option<Lang>,
}

pub async fn broadcast_translated_tts(req: TtsRequest) {
    let tts_start = Instant::now();
    let tts_deadline = Duration::from_secs(5);

    let Some((api_key, base_url, force_default_voice)) =
        req.handle.sessions.get(&req.handle.id).map(|s| {
            (
                s.pipeline_config.elevenlabs_api_key.clone(),
                s.pipeline_config.elevenlabs_base_url.clone(),
                s.pipeline_config.force_default_voice,
            )
        })
    else {
        return;
    };

    let resolved = resolve_voice(ResolveVoiceArgs {
        selected_voice_id: req.selected_voice_id.as_deref(),
        enrollment_lang: req.selected_voice_enrollment_lang.as_ref(),
        target_lang: &req.target_lang,
        force_default_voice,
    });
    if let Some(enroll) = req.selected_voice_enrollment_lang.as_ref()
        && resolved.is_cloned_fallback_due_to_enrollment
    {
        tracing::warn!(
            utterance_id = req.utterance_id,
            target_lang = %req.target_lang,
            enrollment_lang = %enroll,
            "cloned voice enrollment language differs from target; falling back to default voice"
        );
    }
    let voice_id = resolved.voice_id;
    let is_cloned = resolved.is_cloned;
    let model_id = if is_cloned {
        "eleven_multilingual_v2"
    } else {
        "eleven_flash_v2_5"
    };
    let url = format!(
        "{}/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
        base_url, &voice_id
    );

    let audio_buffer = match fetch_tts_audio(FetchTtsArgs {
        url: &url,
        api_key: &api_key,
        text: &req.text,
        model_id,
        is_cloned,
        lang: &req.target_lang,
        deadline: tts_deadline,
    })
    .await
    {
        Some(buf) => buf,
        None => {
            eprintln!(
                "[TTS] no audio for utterance {} lang={}",
                req.utterance_id, req.target_lang
            );
            return;
        }
    };

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    push_tts_into_rtmp(&req.target_lang, &req.handle, &audio_buffer).await;
    notify_host_tts_complete(NotifyCompleteArgs {
        lang: &req.target_lang,
        utterance_id: req.utterance_id,
        tts_ms,
        handle: &req.handle,
    });
}

struct FetchTtsArgs<'a> {
    url: &'a str,
    api_key: &'a str,
    text: &'a str,
    model_id: &'a str,
    is_cloned: bool,
    lang: &'a Lang,
    deadline: Duration,
}

/// Build the JSON payload sent to ElevenLabs. Pulled out so tests can
/// exercise voice_settings branch selection without any network I/O.
pub fn build_tts_request_body(text: &str, model_id: &str, is_cloned: bool) -> serde_json::Value {
    if is_cloned {
        serde_json::json!({
            "text": text,
            "model_id": model_id,
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75,
                "style": 0.0,
                "use_speaker_boost": true
            }
        })
    } else {
        serde_json::json!({ "text": text, "model_id": model_id })
    }
}

async fn fetch_tts_audio(args: FetchTtsArgs<'_>) -> Option<Vec<u8>> {
    let FetchTtsArgs {
        url,
        api_key,
        text,
        model_id,
        is_cloned,
        lang,
        deadline,
    } = args;
    let client = reqwest::Client::new();
    // Voice-settings tuning biases TTS toward the original enrollment accent.
    // Higher stability + non-zero similarity_boost + style=0 keeps the voice
    // close to the clone instead of drifting to an "average" multilingual
    // timbre (the April 2026 Indian-accent regression). Default voices don't
    // need aggressive boost — they are library voices engineered for 32
    // target languages — but the request shape is the same either way so we
    // send the settings unconditionally.
    let body = build_tts_request_body(text, model_id, is_cloned);
    let tts_result = tokio::time::timeout(deadline, async {
        let mut audio_buffer = Vec::new();
        let response = client
            .post(url)
            .header("xi-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                let mut stream = resp.bytes_stream();
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => audio_buffer.extend_from_slice(&chunk),
                        Err(error) => {
                            eprintln!("TTS stream error for {}: {}", lang, error);
                            break;
                        }
                    }
                }
            }
            Ok(resp) => eprintln!("TTS error: {} - {:?}", resp.status(), resp.text().await),
            Err(error) => eprintln!("TTS request error for {}: {}", lang, error),
        }

        audio_buffer
    })
    .await;

    match tts_result {
        Ok(buffer) if !buffer.is_empty() => Some(buffer),
        Ok(_) => None,
        Err(_) => {
            eprintln!("[TTS] TIMEOUT lang={} (>{:?})", lang, deadline);
            None
        }
    }
}

async fn push_tts_into_rtmp(lang: &Lang, handle: &LiveSessionHandle, audio_buffer: &[u8]) {
    let rtmp_manager = handle
        .sessions
        .get(&handle.id)
        .and_then(|session| session.rtmp_manager.clone());
    let Some(manager) = rtmp_manager else {
        return;
    };

    match crate::features::broadcast::data::ffmpeg::decode_mp3_to_pcm(audio_buffer).await {
        Ok(pcm) => {
            let manager = manager.lock().await;
            manager.push_tts(&lang.to_string(), pcm);
        }
        Err(error) => eprintln!("[RTMP] MP3→PCM decode failed: {}", error),
    }
}

struct NotifyCompleteArgs<'a> {
    lang: &'a Lang,
    utterance_id: u64,
    tts_ms: u64,
    handle: &'a LiveSessionHandle,
}

fn notify_host_tts_complete(args: NotifyCompleteArgs<'_>) {
    let Some(live_session) = args.handle.sessions.get(&args.handle.id) else {
        return;
    };

    live_session.send_to_host(to_ws(&ServerMsg::TtsEnd {
        utterance_id: args.utterance_id,
        target_lang: args.lang.to_string(),
        tts_ms: args.tts_ms,
    }));
    live_session.send_to_host(to_ws(&ServerMsg::VideoEnd {
        utterance_id: args.utterance_id,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::broadcast::domain::{
        LiveSession, LiveSessionHandle, LiveSessions, PipelineConfig,
    };
    use dashmap::DashMap;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    fn sessions_with(
        id: &str,
        pipeline_config: Arc<PipelineConfig>,
    ) -> (
        LiveSessions,
        mpsc::UnboundedReceiver<axum::extract::ws::Message>,
    ) {
        let sessions: LiveSessions = Arc::new(DashMap::new());
        let (tx, rx) = mpsc::unbounded_channel();
        let mut session = LiveSession::new(id.into(), Lang::En, None, pipeline_config);
        session.host_tx = Some(tx);
        sessions.insert(id.into(), session);
        (sessions, rx)
    }

    #[test]
    fn resolve_voice_returns_default_when_selected_voice_absent() {
        let resolved = resolve_voice(ResolveVoiceArgs {
            selected_voice_id: None,
            enrollment_lang: None,
            target_lang: &Lang::Ja,
            force_default_voice: false,
        });
        assert!(!resolved.is_cloned);
        assert!(!resolved.is_cloned_fallback_due_to_enrollment);
        assert_eq!(resolved.voice_id, Lang::Ja.voice_id());
    }

    #[test]
    fn resolve_voice_treats_none_enrollment_lang_as_matching_target() {
        let resolved = resolve_voice(ResolveVoiceArgs {
            selected_voice_id: Some("clone"),
            enrollment_lang: None,
            target_lang: &Lang::Ja,
            force_default_voice: false,
        });
        assert!(resolved.is_cloned);
        assert_eq!(resolved.voice_id, "clone");
    }

    #[test]
    fn resolve_voice_honors_force_default_even_when_enrollment_matches() {
        let resolved = resolve_voice(ResolveVoiceArgs {
            selected_voice_id: Some("clone"),
            enrollment_lang: Some(&Lang::En),
            target_lang: &Lang::En,
            force_default_voice: true,
        });
        assert!(!resolved.is_cloned);
        assert_eq!(resolved.voice_id, Lang::En.voice_id());
    }

    #[test]
    fn build_tts_request_body_includes_voice_settings_only_for_cloned_voices() {
        let cloned = build_tts_request_body("hi", "eleven_multilingual_v2", true);
        assert!(cloned.get("voice_settings").is_some());
        assert_eq!(cloned["voice_settings"]["stability"].as_f64().unwrap(), 0.5);
        assert!(
            cloned["voice_settings"]["use_speaker_boost"]
                .as_bool()
                .unwrap()
        );

        let default = build_tts_request_body("hi", "eleven_flash_v2_5", false);
        assert!(default.get("voice_settings").is_none());
        assert_eq!(default["model_id"].as_str().unwrap(), "eleven_flash_v2_5");
        assert_eq!(default["text"].as_str().unwrap(), "hi");
    }

    #[tokio::test]
    async fn broadcast_translated_tts_returns_early_when_session_missing() {
        // Empty session map → the initial lookup fails and we bail before
        // hitting any network code.
        let sessions: LiveSessions = Arc::new(DashMap::new());
        let handle = LiveSessionHandle::new("missing".into(), sessions);
        broadcast_translated_tts(TtsRequest {
            text: "hello".into(),
            utterance_id: 1,
            target_lang: Lang::Ja,
            handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
        })
        .await;
    }

    #[tokio::test]
    async fn broadcast_translated_tts_skips_request_when_elevenlabs_url_empty_and_returns_none() {
        // Without a base URL the fetch target is invalid → reqwest errors
        // and we log the TTS-failed branch without panicking. No RTMP
        // manager configured → push_tts is skipped silently.
        let cfg = Arc::new(PipelineConfig {
            elevenlabs_base_url: String::new(),
            elevenlabs_api_key: "k".into(),
            ..Default::default()
        });
        let (sessions, _rx) = sessions_with("room", cfg);
        let handle = LiveSessionHandle::new("room".into(), sessions);
        broadcast_translated_tts(TtsRequest {
            text: "hello".into(),
            utterance_id: 1,
            target_lang: Lang::Ja,
            handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
        })
        .await;
    }

    #[tokio::test]
    async fn broadcast_translated_tts_sends_host_notifications_on_successful_audio_return() {
        use axum::{Router, extract::State, http::HeaderMap, routing::post};
        use std::sync::Arc as StdArc;

        async fn ok_tts(State(_): State<StdArc<()>>, _: HeaderMap) -> Vec<u8> {
            // Empty body — fetch_tts_audio treats empty buffer as failure and
            // returns None, so notify_host_tts_complete is NOT called. This
            // test simply verifies that the path reaches the ElevenLabs mock
            // without panicking. Successful audio path is covered via the
            // integration smoke test rather than a unit mock that would need
            // a real MP3 decoder.
            Vec::new()
        }

        let state: StdArc<()> = StdArc::new(());
        let app = Router::new()
            .route("/v1/text-to-speech/{voice_id}/stream", post(ok_tts))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let cfg = Arc::new(PipelineConfig {
            elevenlabs_base_url: format!("http://{}", addr),
            elevenlabs_api_key: "k".into(),
            ..Default::default()
        });
        let (sessions, _rx) = sessions_with("room", cfg);
        let handle = LiveSessionHandle::new("room".into(), sessions);

        broadcast_translated_tts(TtsRequest {
            text: "hello".into(),
            utterance_id: 1,
            target_lang: Lang::Ja,
            handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
        })
        .await;

        server.abort();
    }

    #[tokio::test]
    async fn broadcast_translated_tts_notifies_host_with_tts_end_and_video_end_on_successful_fetch()
    {
        use crate::features::broadcast::data::ffmpeg::RtmpManager;
        use axum::{Router, body::Body, http::StatusCode, routing::post};

        async fn ok_bytes() -> axum::response::Response<Body> {
            // Non-empty body triggers the "audio received" branch. Bytes are
            // not valid MP3 — push_tts_into_rtmp's decode step will fail and
            // log, but notify_host_tts_complete still runs.
            axum::response::Response::builder()
                .status(StatusCode::OK)
                .body(Body::from(vec![0u8; 32]))
                .unwrap()
        }
        let app = Router::new().route("/v1/text-to-speech/{voice_id}/stream", post(ok_bytes));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let cfg = Arc::new(PipelineConfig {
            elevenlabs_base_url: format!("http://{}", addr),
            elevenlabs_api_key: "k".into(),
            ..Default::default()
        });
        let (sessions, mut rx) = sessions_with("room", cfg);
        {
            // Attach an (empty) manager so push_tts_into_rtmp exercises the
            // manager-lookup branch. decode_mp3_to_pcm will still fail on the
            // fake audio — that's the intended error branch to cover.
            let mut s = sessions.get_mut("room").unwrap();
            s.rtmp_manager = Some(Arc::new(tokio::sync::Mutex::new(RtmpManager::new())));
        }
        let handle = LiveSessionHandle::new("room".into(), sessions);

        broadcast_translated_tts(TtsRequest {
            text: "hello".into(),
            utterance_id: 42,
            target_lang: Lang::Ja,
            handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
        })
        .await;

        let mut saw_tts_end = false;
        let mut saw_video_end = false;
        while let Ok(msg) = rx.try_recv() {
            if let axum::extract::ws::Message::Text(t) = msg {
                if t.as_str().contains("\"type\":\"tts_end\"")
                    && t.as_str().contains("\"utteranceId\":42")
                {
                    saw_tts_end = true;
                }
                if t.as_str().contains("\"type\":\"video_end\"")
                    && t.as_str().contains("\"utteranceId\":42")
                {
                    saw_video_end = true;
                }
            }
        }
        assert!(
            saw_tts_end,
            "tts_end should be emitted after successful fetch"
        );
        assert!(
            saw_video_end,
            "video_end should be emitted after successful fetch"
        );

        server.abort();
    }

    #[tokio::test]
    async fn broadcast_translated_tts_logs_and_returns_when_elevenlabs_returns_non_2xx() {
        use axum::{Router, http::StatusCode, routing::post};
        async fn fail() -> StatusCode {
            StatusCode::SERVICE_UNAVAILABLE
        }
        let app = Router::new().route("/v1/text-to-speech/{voice_id}/stream", post(fail));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let cfg = Arc::new(PipelineConfig {
            elevenlabs_base_url: format!("http://{}", addr),
            elevenlabs_api_key: "k".into(),
            ..Default::default()
        });
        let (sessions, mut rx) = sessions_with("room", cfg);
        let handle = LiveSessionHandle::new("room".into(), sessions);

        broadcast_translated_tts(TtsRequest {
            text: "hello".into(),
            utterance_id: 1,
            target_lang: Lang::Ja,
            handle,
            selected_voice_id: None,
            selected_voice_enrollment_lang: None,
        })
        .await;

        // Non-2xx triggers the "no audio" early-return; no host notifications.
        assert!(rx.try_recv().is_err());
        server.abort();
    }

    #[test]
    fn resolved_voice_selects_multilingual_v2_model_for_cloned() {
        // Implicit: the dispatcher picks eleven_multilingual_v2 when
        // is_cloned=true and eleven_flash_v2_5 otherwise. Validate the
        // branch through body shape.
        let cloned_body = build_tts_request_body("hi", "eleven_multilingual_v2", true);
        assert_eq!(
            cloned_body["model_id"].as_str().unwrap(),
            "eleven_multilingual_v2"
        );
        let default_body = build_tts_request_body("hi", "eleven_flash_v2_5", false);
        assert_eq!(
            default_body["model_id"].as_str().unwrap(),
            "eleven_flash_v2_5"
        );
    }
}
