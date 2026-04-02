//! Translation pipeline: STT → Translate → TTS → RTMP
//!
//! Host audio flows through:
//! 1. Deepgram Nova-3 (direct WebSocket, no Python wrapper)
//! 2. Google Cloud Translation API v2 — per target language, parallel
//! 3. ElevenLabs TTS — streaming MP3 response (with optional cloned voice)
//! 4. MP3 → PCM decode → queue to RTMP manager for synced playback
//! 5. Source-language passthrough: host audio queued directly to RTMP (no TTS)

use axum::extract::ws::Message;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tungstenite::client::IntoClientRequest;

use crate::types::{Lang, Sessions, ServerMsg};

// ── Service Keys ──────────────────────────────────────────

static DEEPGRAM_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("DEEPGRAM_API_KEY").unwrap_or_default()
});
static GOOGLE_TRANSLATE_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("GOOGLE_TRANSLATE_API_KEY").unwrap_or_default()
});
static ELEVENLABS_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("ELEVENLABS_API_KEY").unwrap_or_default()
});

// ── STT Style Params ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleParams {
    #[serde(default = "default_stability")]
    pub stability: f64,
    #[serde(default = "default_similarity")]
    pub similarity_boost: f64,
    #[serde(default)]
    pub style: f64,
    #[serde(default = "default_speed")]
    pub speed: f64,
    #[serde(default = "default_true")]
    pub use_speaker_boost: bool,
}

fn default_stability() -> f64 { 0.5 }
fn default_similarity() -> f64 { 0.75 }
fn default_speed() -> f64 { 1.0 }
fn default_true() -> bool { true }

impl Default for StyleParams {
    fn default() -> Self {
        Self {
            stability: 0.5,
            similarity_boost: 0.75,
            style: 0.0,
            speed: 1.0,
            use_speaker_boost: true,
        }
    }
}

// ── STT Connection (Direct Deepgram) ──────────────────────

fn emit_final(
    sessions: &Sessions,
    session_id: &str,
    transcript: &str,
    uid: u64,
    source_lang: &Lang,
    style_params: Option<StyleParams>,
    utterance_start: Instant,
    host_audio: Vec<u8>,
) {
    let utterance_end = Instant::now();

    if let Some(session) = sessions.get(session_id) {
        let final_msg = to_ws(&ServerMsg::Final {
            transcript: transcript.to_string(),
            utterance_id: uid,
        });
        session.send_to_host(final_msg);

        let active = session.active_langs();
        let sp = style_params.unwrap_or_default();
        let tier = session.tier;
        println!("[PIPELINE] active langs: {:?} tier={}", active, tier);
        if !active.is_empty() {
            let sessions_clone = sessions.clone();
            let sid = session_id.to_string();
            let src = source_lang.clone();
            let text = transcript.to_string();
            tokio::spawn(async move {
                run_pipeline(
                    &text, uid, &src, &active, &sessions_clone, &sid, &sp, tier,
                    utterance_start, utterance_end, host_audio,
                ).await;
            });
        }
    }
}

const STT_RECONNECT_MAX: u32 = 5;
const STT_RECONNECT_DELAY: Duration = Duration::from_secs(1);

pub async fn start_stt(
    session_id: String,
    sessions: Sessions,
    source_lang: Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let mut utterance_counter: u64 = 0;
    let mut reconnect_count: u32 = 0;

    // Shared buffer: accumulate host audio chunks for source-language passthrough
    let audio_acc: Arc<std::sync::Mutex<Vec<Vec<u8>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    // Adaptive endpointing parameters
    let mut endpointing: u32 = 400;
    let mut utterance_end_ms: u32 = 1500;
    let mut wpm_samples: Vec<u32> = Vec::new();
    let mut adapted = false;

    if DEEPGRAM_API_KEY.is_empty() {
        eprintln!("[STT] DEEPGRAM_API_KEY not set, STT disabled");
        return;
    }

    loop {
        let max_attempts = if reconnect_count == 0 { 10 } else { STT_RECONNECT_MAX };
        let mut ws_stream = None;

        for attempt in 1..=max_attempts {
            if !sessions.contains_key(&session_id) {
                eprintln!("[STT] Session {} gone, stopping", session_id);
                return;
            }

            // Connect directly to Deepgram Nova-3
            let url = crate::stt::build_deepgram_url(
                &source_lang.to_string(), 44100, endpointing, utterance_end_ms,
            );
            let request = match url.into_client_request() {
                Ok(mut req) => {
                    req.headers_mut().insert(
                        "Authorization",
                        format!("Token {}", &*DEEPGRAM_API_KEY).parse().unwrap(),
                    );
                    req
                }
                Err(e) => {
                    eprintln!("[STT] Failed to build request: {}", e);
                    return;
                }
            };

            match tokio_tungstenite::connect_async(request).await {
                Ok((stream, _)) => {
                    println!(
                        "[STT] Connected to Deepgram Nova-3 (attempt {}, endpointing={}, utterance_end_ms={})",
                        attempt, endpointing, utterance_end_ms
                    );
                    ws_stream = Some(stream);
                    break;
                }
                Err(e) => {
                    let delay = if reconnect_count == 0 {
                        Duration::from_secs(3)
                    } else {
                        STT_RECONNECT_DELAY
                    };
                    eprintln!("[STT] connect attempt {}/{} failed: {}", attempt, max_attempts, e);
                    tokio::time::sleep(delay).await;
                }
            }
        }

        let ws_stream = match ws_stream {
            Some(s) => s,
            None => {
                eprintln!("[STT] Failed to connect after {} attempts", max_attempts);
                return;
            }
        };

        let (stt_sink, mut stt_stream) = ws_stream.split();
        let stt_sink = Arc::new(tokio::sync::Mutex::new(stt_sink));

        // Control channel: recv_task can send Finalize/CloseStream via sink
        let sink_for_ctrl = stt_sink.clone();

        // Task 1: Forward host audio → Deepgram + accumulate for passthrough
        let acc_tx = audio_acc.clone();
        let audio_rx_clone = audio_rx.clone();
        let sink_for_audio = stt_sink.clone();
        let send_task = tokio::spawn(async move {
            let mut rx = audio_rx_clone.lock().await;
            while let Some(data) = rx.recv().await {
                if let Ok(mut acc) = acc_tx.lock() {
                    acc.push(data.clone());
                }
                let mut sink = sink_for_audio.lock().await;
                if sink.send(tungstenite::Message::Binary(data.into())).await.is_err() {
                    break;
                }
            }
            // Send CloseStream on shutdown
            let mut sink = sink_for_audio.lock().await;
            let _ = sink.send(tungstenite::Message::Text(
                r#"{"type":"CloseStream"}"#.to_string().into()
            )).await;
        });

        // Task 2: Read Deepgram responses, extract prosody/emotion, emit events
        let sessions_ref = sessions.clone();
        let sid = session_id.clone();
        let source_lang_clone = source_lang.clone();
        let acc_rx = audio_acc.clone();
        let mut uc = utterance_counter;
        let mut utterance_start: Option<Instant> = None;
        let mut chunk_detector = crate::stt::get_detector(&source_lang.to_string());

        let disconnected_unexpectedly = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let disc_flag = disconnected_unexpectedly.clone();

        // Capture adaptive state for this connection
        let mut local_wpm_samples = wpm_samples.clone();
        let mut local_adapted = adapted;
        let needs_adaptive_reconnect = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let adaptive_flag = needs_adaptive_reconnect.clone();
        let adaptive_endpointing = Arc::new(std::sync::Mutex::new(None::<(u32, u32)>));
        let adaptive_params = adaptive_endpointing.clone();

        let recv_task = tokio::spawn(async move {
            while let Some(msg_result) = stt_stream.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("[STT] read error: {}", e);
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                };

                let text = match msg {
                    tungstenite::Message::Text(t) => t.to_string(),
                    tungstenite::Message::Close(_) => {
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                    _ => continue,
                };

                let dg: crate::stt::DgResponse = match serde_json::from_str(&text) {
                    Ok(d) => d,
                    Err(_) => continue,
                };

                if !sessions_ref.contains_key(&sid) {
                    break;
                }

                match dg.msg_type.as_str() {
                    "Results" => {
                        let transcript = match dg.transcript() {
                            Some(t) => t,
                            None => continue,
                        };

                        if dg.speech_final || dg.is_final {
                            // ── Final event ──
                            uc += 1;
                            let uid = uc;
                            println!("[FINAL #{}] {}", uid, transcript);

                            let start = utterance_start.take().unwrap_or_else(Instant::now);
                            let host_audio: Vec<u8> = {
                                let mut acc = acc_rx.lock().unwrap();
                                acc.drain(..).flatten().collect()
                            };

                            // Prosody → emotion → style params
                            let mut prosody = crate::stt::extract_prosody(&host_audio, 44100);
                            let word_count = transcript.split_whitespace().count();
                            crate::stt::compute_speaking_rate(&mut prosody, word_count);
                            let emotion = crate::stt::classify_emotion(&prosody);
                            let (stability, similarity, style, speed) = crate::stt::map_style(emotion);

                            eprintln!(
                                "[EMOTION] {} (energy={:.4} pitch_std={:.1} rate={}wpm)",
                                emotion, prosody.energy_rms, prosody.pitch_std, prosody.speaking_rate_wpm
                            );

                            let sp = StyleParams {
                                stability, similarity_boost: similarity, style, speed,
                                use_speaker_boost: true,
                            };

                            emit_final(
                                &sessions_ref, &sid, &transcript, uid,
                                &source_lang_clone, Some(sp),
                                start, host_audio,
                            );
                            chunk_detector.reset();

                            // Adaptive endpointing: track WPM over first 5 finals
                            if !local_adapted && prosody.speaking_rate_wpm > 0 {
                                local_wpm_samples.push(prosody.speaking_rate_wpm);
                                if local_wpm_samples.len() >= 5 {
                                    let avg_wpm = local_wpm_samples.iter().sum::<u32>() as f32
                                        / local_wpm_samples.len() as f32;
                                    let (label, new_endp, new_utt_ms) =
                                        crate::stt::classify_speaking_speed(avg_wpm);
                                    local_adapted = true;

                                    if label != "normal" {
                                        eprintln!(
                                            "[ADAPTIVE] Avg WPM: {:.0}, classified: {}, reconnecting (endpointing={}, utterance_end_ms={})",
                                            avg_wpm, label, new_endp, new_utt_ms
                                        );
                                        if let Ok(mut params) = adaptive_params.lock() {
                                            *params = Some((new_endp, new_utt_ms));
                                        }
                                        adaptive_flag.store(true, std::sync::atomic::Ordering::Release);
                                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                                        // Send CloseStream
                                        let mut sink = sink_for_ctrl.lock().await;
                                        let _ = sink.send(tungstenite::Message::Text(
                                            r#"{"type":"CloseStream"}"#.to_string().into()
                                        )).await;
                                        break;
                                    } else {
                                        eprintln!(
                                            "[ADAPTIVE] Avg WPM: {:.0}, classified: {}, keeping defaults",
                                            avg_wpm, label
                                        );
                                    }
                                }
                            }
                        } else {
                            // ── Interim event ──
                            if utterance_start.is_none() {
                                utterance_start = Some(Instant::now());
                            }
                            println!("[INTERIM] {}", transcript);

                            if let Some(session) = sessions_ref.get(&sid) {
                                session.send_to_host(to_ws(&ServerMsg::Interim {
                                    transcript: transcript.clone(),
                                }));
                            }

                            // Clause boundary chunking: force finalize if needed
                            if chunk_detector.check(&transcript) {
                                eprintln!("[CHUNK] Forcing finalize at clause boundary");
                                let mut sink = sink_for_ctrl.lock().await;
                                let _ = sink.send(tungstenite::Message::Text(
                                    r#"{"type":"Finalize"}"#.to_string().into()
                                )).await;
                            }
                        }
                    }
                    "SpeechStarted" => {
                        eprintln!("[DG] VAD: speech started");
                    }
                    "UtteranceEnd" => {
                        eprintln!("[DG] VAD: utterance end");
                    }
                    "Metadata" => {
                        eprintln!("[DG] Session started (request_id={})", dg.request_id);
                    }
                    "Error" => {
                        eprintln!("[DG] Error: {}", dg.message);
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                    _ => {}
                }
            }
            (uc, local_wpm_samples, local_adapted)
        });

        tokio::select! {
            _ = send_task => {},
            result = recv_task => {
                if let Ok((uc, wpm, adapt)) = result {
                    utterance_counter = uc;
                    wpm_samples = wpm;
                    adapted = adapt;
                }
            },
        }

        // Check for adaptive reconnect (intentional, not an error)
        if needs_adaptive_reconnect.load(std::sync::atomic::Ordering::Acquire) {
            if let Ok(params) = adaptive_endpointing.lock() {
                if let Some((new_endp, new_utt_ms)) = *params {
                    endpointing = new_endp;
                    utterance_end_ms = new_utt_ms;
                }
            }
            // Don't increment reconnect_count for adaptive reconnects
            continue;
        }

        if !disconnected_unexpectedly.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        if !sessions.contains_key(&session_id) {
            break;
        }

        reconnect_count += 1;
        if reconnect_count > STT_RECONNECT_MAX {
            eprintln!("[STT] Exceeded max reconnects, giving up");
            break;
        }
        tokio::time::sleep(STT_RECONNECT_DELAY).await;
    }
}

// ── Translation Pipeline ────────────────────────────────

#[derive(Deserialize)]
struct GoogleTranslateResponse {
    data: GoogleTranslateData,
}

#[derive(Deserialize)]
struct GoogleTranslateData {
    translations: Vec<GoogleTranslation>,
}

#[derive(Deserialize)]
struct GoogleTranslation {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

async fn run_pipeline(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_langs: &[Lang],
    sessions: &Sessions,
    session_id: &str,
    style_params: &StyleParams,
    tier: u8,
    utterance_start: Instant,
    utterance_end: Instant,
    host_audio: Vec<u8>,
) {
    let client = reqwest::Client::new();
    let mut handles = Vec::new();

    let voice_clone_id = sessions.get(session_id).and_then(|s| s.voice_clone_id.clone());

    for lang in target_langs {
        if lang == source_lang {
            // Source-language passthrough: host audio queued directly to RTMP
            let rtmp_mgr = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone());
            if let Some(manager) = rtmp_mgr {
                if !host_audio.is_empty() {
                    let mut pcm = host_audio.clone();
                    let pcm_len = pcm.len();
                    let lang_str = lang.to_string();
                    let mgr = manager.clone();
                    handles.push(tokio::spawn(async move {
                        let utterance_dur = utterance_end.duration_since(utterance_start);
                        let max_dur = utterance_dur + Duration::from_millis(2000);
                        let max_bytes = (max_dur.as_secs_f64() * 88200.0) as usize;
                        if pcm.len() > max_bytes {
                            crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
                        }
                        let locked = mgr.lock().await;
                        locked.queue_audio(&lang_str, pcm, utterance_start);
                        eprintln!(
                            "[PASSTHROUGH] Queued host audio for {} ({}KB)",
                            lang_str, pcm_len / 1024
                        );
                    }));
                }
            }
            continue;
        }

        let transcript = transcript.to_string();
        let source = source_lang.clone();
        let target = lang.clone();
        let client = client.clone();
        let sessions = sessions.clone();
        let session_id = session_id.to_string();
        let voice_clone_id = voice_clone_id.clone();
        let sp = style_params.clone();

        handles.push(tokio::spawn(async move {
            // 1. Translate
            let start = Instant::now();
            let url = format!(
                "https://translation.googleapis.com/language/translate/v2?key={}",
                &*GOOGLE_TRANSLATE_API_KEY
            );
            let translate_resp = client
                .post(&url)
                .json(&serde_json::json!({
                    "q": transcript,
                    "source": source.to_string(),
                    "target": target.to_string(),
                    "format": "text",
                }))
                .send()
                .await;

            let translated_text = match translate_resp {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<GoogleTranslateResponse>().await {
                        Ok(r) => r.data.translations.into_iter().next()
                            .map(|t| t.translated_text)
                            .unwrap_or_default(),
                        Err(e) => {
                            eprintln!("[TRANSLATE] parse error for {}: {}", target, e);
                            return;
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    eprintln!("[TRANSLATE] error for {}: {} - {}", target, status, body);
                    return;
                }
                Err(e) => {
                    eprintln!("[TRANSLATE] request error for {}: {}", target, e);
                    return;
                }
            };
            let translate_ms = start.elapsed().as_millis() as u64;
            println!("[TRANSLATE] {} -> {} = '{}' ({}ms)", source, target, translated_text, translate_ms);

            // 2. Send translation to host
            if let Some(session) = sessions.get(&session_id) {
                session.send_to_host(to_ws(&ServerMsg::Translation {
                    lang: target.to_string(),
                    text: translated_text.clone(),
                    utterance_id,
                    translate_ms,
                }));
            }

            // 3. TTS (only for tier 2+)
            if tier >= 2 {
                do_tts(
                    &client, &translated_text, utterance_id, &target,
                    &sessions, &session_id, voice_clone_id.as_deref(), &sp,
                    utterance_start, utterance_end,
                ).await;
            }
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
}

async fn do_tts(
    client: &reqwest::Client,
    text: &str,
    utterance_id: u64,
    lang: &Lang,
    sessions: &Sessions,
    session_id: &str,
    voice_clone_id: Option<&str>,
    style_params: &StyleParams,
    utterance_start: Instant,
    utterance_end: Instant,
) {
    let tts_start = Instant::now();

    let tts_deadline = {
        let delay_ms: u64 = std::env::var("BROADCAST_DELAY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5000);
        let sync_deadline = Duration::from_millis(delay_ms.saturating_sub(500));
        let hard_cap = Duration::from_secs(10);
        sync_deadline.min(hard_cap)
    };

    let voice_id = match voice_clone_id {
        Some(id) => id.to_string(),
        None => lang.voice_id().to_string(),
    };
    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
        &voice_id
    );

    let is_cloned = voice_clone_id.is_some();
    let model_id = if is_cloned { "eleven_multilingual_v2" } else { "eleven_flash_v2_5" };
    println!(
        "[TTS] voice={}{} model={} lang={} text='{}' [deadline={}ms]",
        &voice_id, if is_cloned { " (cloned)" } else { "" }, model_id, lang, text,
        tts_deadline.as_millis()
    );

    let tts_body = serde_json::json!({
        "text": text,
        "model_id": model_id,
        "voice_settings": {
            "stability": style_params.stability,
            "similarity_boost": style_params.similarity_boost,
            "style": style_params.style,
            "use_speaker_boost": style_params.use_speaker_boost
        }
    });

    let lang_str = lang.to_string();
    let tts_result = tokio::time::timeout(tts_deadline, async {
        let mut audio_buffer: Vec<u8> = Vec::new();
        let resp = client
            .post(&url)
            .header("xi-api-key", &*ELEVENLABS_API_KEY)
            .header("Content-Type", "application/json")
            .json(&tts_body)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let mut stream = r.bytes_stream();
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => audio_buffer.extend_from_slice(&chunk),
                        Err(e) => {
                            eprintln!("[TTS] stream error for {}: {}", lang_str, e);
                            break;
                        }
                    }
                }
            }
            Ok(r) => eprintln!("[TTS] error: {} - {:?}", r.status(), r.text().await),
            Err(e) => eprintln!("[TTS] request error for {}: {}", lang_str, e),
        }
        audio_buffer
    })
    .await;

    let audio_buffer = match tts_result {
        Ok(buf) if !buf.is_empty() => buf,
        Ok(_) => return,
        Err(_) => {
            eprintln!(
                "[TTS] TIMEOUT: utterance {} for {} exceeded {}ms",
                utterance_id, lang, tts_deadline.as_millis()
            );
            return;
        }
    };

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    println!("[TTS] {}KB in {}ms for {}", audio_buffer.len() / 1024, tts_ms, lang);

    // Queue decoded PCM to RTMP
    let rtmp_mgr = sessions.get(session_id).and_then(|s| s.rtmp_manager.clone());
    if let Some(manager) = rtmp_mgr {
        match crate::ffmpeg::decode_mp3_to_pcm(&audio_buffer).await {
            Ok(mut pcm) => {
                let utterance_dur = utterance_end.duration_since(utterance_start);
                let max_dur = utterance_dur + Duration::from_millis(2000);
                let max_bytes = (max_dur.as_secs_f64() * 88200.0) as usize;
                if pcm.len() > max_bytes {
                    crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
                }
                let mgr = manager.lock().await;
                mgr.queue_audio(&lang.to_string(), pcm, utterance_start);
                eprintln!("[RTMP] Queued audio for {} (pipeline: {}ms)", lang, tts_start.elapsed().as_millis());
            }
            Err(e) => eprintln!("[RTMP] MP3->PCM decode failed: {}", e),
        }
    }

    // Send MP3 to host for monitoring
    if let Some(session) = sessions.get(session_id) {
        session.send_to_host(to_ws(&ServerMsg::TtsStart { lang: lang.to_string(), utterance_id }));
        session.send_to_host(Message::Binary(audio_buffer.into()));
        session.send_to_host(to_ws(&ServerMsg::TtsEnd { lang: lang.to_string(), utterance_id, tts_ms }));
    }
}

// ── Voice Cloning ────────────────────────────────────────

fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let sample_rate: u32 = 44100;
    let bits_per_sample: u16 = 16;
    let channels: u16 = 1;
    let byte_rate = sample_rate * (bits_per_sample as u32 / 8) * channels as u32;
    let block_align = channels * (bits_per_sample / 8);
    let data_size = pcm.len() as u32;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}

pub async fn clone_voice(pcm: Vec<u8>, sessions: &Sessions, session_id: &str) {
    let wav = pcm_to_wav(&pcm);
    println!("[VOICE_CLONE] starting ({} bytes PCM)", pcm.len());

    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .text("name", format!("brivva-{}", &session_id[..6.min(session_id.len())]))
        .part(
            "files",
            reqwest::multipart::Part::bytes(wav)
                .file_name("host_voice.wav")
                .mime_str("audio/wav")
                .unwrap(),
        );

    let resp = client
        .post("https://api.elevenlabs.io/v1/voices/add")
        .header("xi-api-key", &*ELEVENLABS_API_KEY)
        .multipart(form)
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            #[derive(Deserialize)]
            struct CloneResp { voice_id: String }
            match r.json::<CloneResp>().await {
                Ok(parsed) => {
                    println!("[VOICE_CLONE] success! voice_id={}", parsed.voice_id);
                    if let Some(mut session) = sessions.get_mut(session_id) {
                        session.voice_clone_id = Some(parsed.voice_id.clone());
                        session.send_to_host(to_ws(&ServerMsg::VoiceReady {
                            voice_id: parsed.voice_id,
                        }));
                    }
                }
                Err(e) => eprintln!("[VOICE_CLONE] parse error: {}", e),
            }
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            eprintln!("[VOICE_CLONE] error {}: {}", status, body);
        }
        Err(e) => eprintln!("[VOICE_CLONE] request error: {}", e),
    }
}

pub async fn delete_cloned_voice(voice_id: &str) {
    let client = reqwest::Client::new();
    let url = format!("https://api.elevenlabs.io/v1/voices/{}", voice_id);
    match client.delete(&url).header("xi-api-key", &*ELEVENLABS_API_KEY).send().await {
        Ok(r) if r.status().is_success() => println!("[VOICE_CLONE] deleted {}", voice_id),
        Ok(r) => eprintln!("[VOICE_CLONE] delete error: {}", r.status()),
        Err(e) => eprintln!("[VOICE_CLONE] delete error: {}", e),
    }
}

fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}
