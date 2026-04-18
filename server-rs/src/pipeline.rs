//! Translation pipeline: STT → Translate → TTS
//!
//! Host audio flows through:
//! 1. STT (Soniox v4, direct WS) — `wss://stt-rt.soniox.com/transcribe-websocket`
//!    - Raw 44.1 kHz PCM s16le frames pushed as binary WS messages
//!    - Source-language transcription only here; per-target translation is a
//!      separate concern (Phase 2+ — either open one Soniox WS per target with
//!      `translation.type=one_way` or use a dedicated translate provider).
//! 2. Translation — currently pass-through stub (see `run_pipeline`). The stub
//!    will be replaced by Soniox per-target WS or a translate provider.
//! 3. ElevenLabs TTS — streaming MP3 response (with optional cloned voice).

use axum::extract::ws::Message;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use crate::types::{FrameBuffer, Lang, Rooms, ServerMsg, TimestampedFrame};

// ── Service config ────────────────────────────────────────

const SONIOX_WS_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
/// Soniox real-time model. `stt-rt-preview` is the current public real-time model.
const SONIOX_MODEL: &str = "stt-rt-preview";
/// Host audio format — matches what the browser sends + what ffmpeg expects.
const HOST_SAMPLE_RATE: u32 = 44_100;

static SONIOX_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("SONIOX_API_KEY").unwrap_or_default()
});
static ELEVENLABS_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("ELEVENLABS_API_KEY").unwrap_or_default()
});

// ── STT Events (from stt-wrapper) ─────────────────────────

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

// ── Soniox v4 wire format ─────────────────────────────────
//
// First message from client: JSON config (api_key + audio format + model).
// Then: binary audio frames.
// Responses from server: JSON with `tokens` array (interim + final tokens)
// and optional `error_code`/`error_message`. A token with `text == "<end>"`
// signals end-of-utterance (endpoint detection).

#[derive(Debug, Serialize)]
struct SonioxConfig<'a> {
    api_key: &'a str,
    model: &'a str,
    audio_format: &'a str,
    sample_rate: u32,
    num_channels: u32,
    language_hints: Vec<String>,
    enable_endpoint_detection: bool,
}

#[derive(Debug, Deserialize)]
struct SonioxResponse {
    #[serde(default)]
    tokens: Vec<SonioxToken>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SonioxToken {
    #[serde(default)]
    text: String,
    #[serde(default)]
    is_final: bool,
}

// ── STT Connection ────────────────────────────────────────

/// Emit a final transcript: broadcast to room and trigger translation pipeline.
/// `host_audio` is the raw 44.1kHz PCM captured during this utterance, used for
/// source-language passthrough (skip TTS for RTMP streams in the host's language).
fn emit_final(
    rooms: &Rooms,
    room_id: &str,
    transcript: &str,
    uid: u64,
    source_lang: &Lang,
    style_params: Option<StyleParams>,
    utterance_start: Instant,
    host_audio: Vec<u8>,
) {
    let utterance_end = Instant::now();

    if let Some(room) = rooms.get(room_id) {
        let final_msg = to_ws(&ServerMsg::Final {
            transcript: transcript.to_string(),
            utterance_id: uid,
        });
        room.send_to_host(final_msg.clone());
        room.send_to_all_guests(final_msg);

        let active = room.active_langs();
        let sp = style_params.unwrap_or_default();
        let frame_buffer = room.frame_buffer.clone();
        println!("[PIPELINE] active langs: {:?}, style: {:.2}", active, sp.style);
        if !active.is_empty() {
            let rooms_clone = rooms.clone();
            let rid = room_id.to_string();
            let src = source_lang.clone();
            let text = transcript.to_string();
            tokio::spawn(async move {
                run_pipeline(
                    &text, uid, &src, &active, &rooms_clone, &rid, &sp,
                    utterance_start, utterance_end, &frame_buffer, host_audio,
                ).await;
            });
        }
    }
}

/// Max reconnect attempts for the Soniox WebSocket mid-session.
const STT_RECONNECT_MAX: u32 = 5;
/// Delay between Soniox reconnect attempts.
const STT_RECONNECT_DELAY: Duration = Duration::from_secs(1);
/// Soniox's end-of-utterance sentinel token (endpoint detection).
const SONIOX_END_TOKEN: &str = "<end>";

/// Connect to Soniox, stream host audio, receive transcription tokens.
/// Automatic reconnect on mid-session drop (up to STT_RECONNECT_MAX attempts).
pub async fn start_stt(
    room_id: String,
    rooms: Rooms,
    source_lang: Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    if SONIOX_API_KEY.is_empty() {
        eprintln!("[STT] SONIOX_API_KEY not set — STT pipeline disabled");
        return;
    }

    // Wrap audio_rx in Arc<Mutex> so it survives reconnects without losing frames.
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let mut utterance_counter: u64 = 0;
    let mut reconnect_count: u32 = 0;

    // Host audio accumulator for source-language passthrough (native voice → RTMP).
    let audio_acc: Arc<std::sync::Mutex<Vec<Vec<u8>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    loop {
        let max_attempts = if reconnect_count == 0 { 10 } else { STT_RECONNECT_MAX };
        let mut ws_stream = None;

        for attempt in 1..=max_attempts {
            if !rooms.contains_key(&room_id) {
                eprintln!("[STT] Room {} gone, stopping Soniox pipeline", room_id);
                return;
            }

            if reconnect_count > 0 {
                eprintln!(
                    "[STT] Reconnecting to Soniox (attempt {}/{})...",
                    attempt, STT_RECONNECT_MAX
                );
            }

            match tokio_tungstenite::connect_async(SONIOX_WS_URL).await {
                Ok((stream, _)) => {
                    if reconnect_count > 0 {
                        eprintln!("[STT] Reconnected to Soniox (attempt {})", attempt);
                    } else {
                        println!("[STT] Connected to Soniox (attempt {})", attempt);
                    }
                    ws_stream = Some(stream);
                    break;
                }
                Err(e) => {
                    let delay = if reconnect_count == 0 {
                        Duration::from_secs(3)
                    } else {
                        STT_RECONNECT_DELAY
                    };
                    eprintln!(
                        "[STT] connect attempt {}/{} failed: {}",
                        attempt, max_attempts, e
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }

        let ws_stream = match ws_stream {
            Some(s) => s,
            None => {
                eprintln!(
                    "[STT] Failed to reach Soniox after {} attempts, stopping pipeline",
                    max_attempts
                );
                return;
            }
        };

        let (mut stt_sink, mut stt_stream) = ws_stream.split();

        // Send Soniox config as the first WS message.
        let config = SonioxConfig {
            api_key: &SONIOX_API_KEY,
            model: SONIOX_MODEL,
            audio_format: "pcm_s16le",
            sample_rate: HOST_SAMPLE_RATE,
            num_channels: 1,
            language_hints: vec![source_lang.to_string()],
            enable_endpoint_detection: true,
        };
        let config_json = match serde_json::to_string(&config) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[STT] config serialize error: {}", e);
                return;
            }
        };
        if let Err(e) = stt_sink
            .send(tungstenite::Message::Text(config_json.into()))
            .await
        {
            eprintln!("[STT] Failed to send Soniox config: {}", e);
            reconnect_count += 1;
            if reconnect_count > STT_RECONNECT_MAX { break; }
            continue;
        }

        // Task 1: Forward host audio → Soniox + mirror into the passthrough accumulator.
        let acc_tx = audio_acc.clone();
        let audio_rx_clone = audio_rx.clone();
        let send_task = tokio::spawn(async move {
            let mut rx = audio_rx_clone.lock().await;
            while let Some(data) = rx.recv().await {
                if let Ok(mut acc) = acc_tx.lock() {
                    acc.push(data.clone());
                }
                if stt_sink
                    .send(tungstenite::Message::Binary(data.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Task 2: Read Soniox tokens, emit Interim/Final events to the room.
        let rooms_ref = rooms.clone();
        let rid = room_id.clone();
        let source_lang_clone = source_lang.clone();
        let acc_rx = audio_acc.clone();
        let mut uc = utterance_counter;
        let mut utterance_start: Option<Instant> = None;

        let disconnected_unexpectedly = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let disc_flag = disconnected_unexpectedly.clone();

        let recv_task = tokio::spawn(async move {
            // Running transcript of fully-committed text for the current utterance.
            let mut final_text = String::new();

            while let Some(msg_result) = stt_stream.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("[STT] WebSocket read error: {}", e);
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                };

                let text = match msg {
                    tungstenite::Message::Text(t) => t.to_string(),
                    tungstenite::Message::Close(_) => {
                        eprintln!("[STT] Soniox closed the connection");
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                    _ => continue,
                };

                let resp: SonioxResponse = match serde_json::from_str(&text) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("[STT] Soniox parse error: {} — raw: {}", e, text);
                        continue;
                    }
                };

                if let Some(code) = resp.error_code {
                    eprintln!(
                        "[STT] Soniox error {}: {}",
                        code,
                        resp.error_message.unwrap_or_default()
                    );
                    disc_flag.store(true, std::sync::atomic::Ordering::Release);
                    break;
                }

                let room = match rooms_ref.get(&rid) {
                    Some(r) => r,
                    None => break, // room gone = normal shutdown
                };

                // Collect non-final tokens as the current interim tail so we can
                // emit one Interim per response instead of per-token.
                let mut interim_tail = String::new();
                let mut endpoint_hit = false;
                for tok in resp.tokens.iter() {
                    if tok.text == SONIOX_END_TOKEN {
                        endpoint_hit = true;
                        continue;
                    }
                    if tok.is_final {
                        final_text.push_str(&tok.text);
                        if utterance_start.is_none() {
                            utterance_start = Some(Instant::now());
                        }
                    } else {
                        interim_tail.push_str(&tok.text);
                        if utterance_start.is_none() {
                            utterance_start = Some(Instant::now());
                        }
                    }
                }

                // Emit an Interim (only when there's something to show — drops empty keep-alives).
                let interim = format!("{}{}", final_text, interim_tail);
                if !interim.is_empty() {
                    let im = to_ws(&ServerMsg::Interim {
                        transcript: interim.clone(),
                    });
                    room.send_to_host(im.clone());
                    room.send_to_all_guests(im);
                }

                // Endpoint detected → commit the utterance.
                if endpoint_hit && !final_text.trim().is_empty() {
                    uc += 1;
                    let uid = uc;
                    let committed = std::mem::take(&mut final_text);
                    let start = utterance_start.take().unwrap_or_else(Instant::now);
                    let host_audio: Vec<u8> = {
                        let mut acc = acc_rx.lock().unwrap();
                        acc.drain(..).flatten().collect()
                    };
                    println!("[FINAL #{}] {}", uid, committed);
                    drop(room);
                    // Soniox doesn't emit style_params — TTS falls back to defaults.
                    emit_final(
                        &rooms_ref,
                        &rid,
                        &committed,
                        uid,
                        &source_lang_clone,
                        None,
                        start,
                        host_audio,
                    );
                } else if endpoint_hit {
                    // Endpoint with no committed text (noise / silence) — just reset.
                    utterance_start = None;
                    if let Ok(mut acc) = acc_rx.lock() {
                        acc.clear();
                    }
                }
            }
            uc
        });

        tokio::select! {
            _ = send_task => {},
            result = recv_task => {
                if let Ok(uc) = result {
                    utterance_counter = uc;
                }
            },
        }

        if !disconnected_unexpectedly.load(std::sync::atomic::Ordering::Acquire) {
            break; // normal shutdown (room removed, host disconnected, audio_rx closed)
        }

        if !rooms.contains_key(&room_id) {
            break;
        }

        reconnect_count += 1;
        if reconnect_count > STT_RECONNECT_MAX {
            eprintln!(
                "[STT] Exceeded max reconnect attempts ({}), giving up",
                STT_RECONNECT_MAX
            );
            break;
        }

        eprintln!(
            "[STT] Will attempt reconnect {}/{}",
            reconnect_count, STT_RECONNECT_MAX
        );
        tokio::time::sleep(STT_RECONNECT_DELAY).await;
    }
}

// ── Translation Pipeline ──
//
// Translation is performed inside the STT provider (Soniox v4 emits
// already-translated text per target). Stage below is currently a
// pass-through stub until the Soniox WS client lands — do NOT ship to prod.

/// Run the full translation + TTS pipeline for one utterance.
/// `host_audio` is raw 44.1kHz PCM of the host's voice during this utterance,
/// used for source-language passthrough on RTMP streams.
async fn run_pipeline(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_langs: &[Lang],
    rooms: &Rooms,
    room_id: &str,
    style_params: &StyleParams,
    utterance_start: Instant,
    utterance_end: Instant,
    frame_buffer: &FrameBuffer,
    host_audio: Vec<u8>,
) {
    let client = reqwest::Client::new();
    let mut handles = Vec::new();

    // Get cloned voice ID if available
    let voice_clone_id = rooms.get(room_id).and_then(|r| r.voice_clone_id.clone());

    // Grab the video frames for this utterance ONCE (shared across all languages)
    let frames = {
        if let Ok(buf) = frame_buffer.lock() {
            buf.iter()
                .filter(|f| f.timestamp >= utterance_start && f.timestamp <= utterance_end)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        }
    };
    let frames = Arc::new(frames);
    eprintln!(
        "[PIPELINE] utterance {} captured {} video frames ({:.0}ms window)",
        utterance_id,
        frames.len(),
        utterance_end.duration_since(utterance_start).as_millis()
    );

    for lang in target_langs {
        if lang == source_lang {
            // Source-language passthrough: host audio is already 44.1kHz PCM (matches FFmpeg).
            // Queue directly to RTMP streams. No TTS needed — it's the host's own voice.
            let rtmp_mgr = rooms.get(room_id).and_then(|r| r.rtmp_manager.clone());
            if let Some(manager) = rtmp_mgr {
                if !host_audio.is_empty() {
                    let mut pcm = host_audio.clone();
                    let pcm_len = pcm.len();
                    let lang_str = lang.to_string();
                    let mgr = manager.clone();
                    handles.push(tokio::spawn(async move {
                        // Apply same truncation as translated audio
                        let utterance_dur = utterance_end.duration_since(utterance_start);
                        let max_dur = utterance_dur + Duration::from_millis(2000);
                        let max_bytes = (max_dur.as_secs_f64() * 88200.0) as usize;
                        if pcm.len() > max_bytes {
                            crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
                        }
                        let locked = mgr.lock().await;
                        locked.queue_audio(&lang_str, pcm, utterance_start);
                        eprintln!(
                            "[PASSTHROUGH] Queued host audio for {} ({}KB, 44.1kHz native)",
                            lang_str,
                            pcm_len / 1024
                        );
                    }));
                } else {
                    eprintln!("[PASSTHROUGH] no host audio captured for source lang {}", lang);
                }
            }
            continue;
        }

        let transcript = transcript.to_string();
        let source = source_lang.clone();
        let target = lang.clone();
        let client = client.clone();
        let rooms = rooms.clone();
        let room_id = room_id.to_string();
        let voice_clone_id = voice_clone_id.clone();
        let sp = style_params.clone();
        let frames = frames.clone();

        handles.push(tokio::spawn(async move {
            // 1. Translate — STUB: pass-through source transcript.
            //    TODO(soniox): replace with Soniox v4 per-target translation
            //    (translations arrive on the STT WS, keyed by target lang).
            let _ = &source;
            let start = Instant::now();
            let translated_text = transcript.clone();
            let translate_ms = start.elapsed().as_millis() as u64;
            println!(
                "[TRANSLATE] {} → {} = '{}' ({}ms)",
                source, target, translated_text, translate_ms
            );
            println!("[METRIC] translate_ms={} lang={}", translate_ms, target);

            // 2. Broadcast translation text
            if let Some(room) = rooms.get(&room_id) {
                let msg = to_ws(&ServerMsg::Translation {
                    text: translated_text.clone(),
                    utterance_id,
                    translate_ms,
                });
                room.send_to_lang(&target, msg.clone());
                room.send_to_host(msg);
            }

            // 3. TTS + send synced audio+video
            do_tts_and_broadcast(
                &client,
                &translated_text,
                translate_ms,
                utterance_id,
                &target,
                &rooms,
                &room_id,
                voice_clone_id.as_deref(),
                &sp,
                &frames,
                utterance_start,
                utterance_end,
            )
            .await;
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
}

/// Call ElevenLabs TTS and send synced audio + video to guests.
/// Has a hard timeout at broadcast_delay - 500ms to prevent sync slips.
async fn do_tts_and_broadcast(
    client: &reqwest::Client,
    text: &str,
    _translate_ms: u64,
    utterance_id: u64,
    lang: &Lang,
    rooms: &Rooms,
    room_id: &str,
    voice_clone_id: Option<&str>,
    style_params: &StyleParams,
    utterance_frames: &[TimestampedFrame],
    utterance_start: Instant,
    utterance_end: Instant,
) {
    let tts_start = Instant::now();

    // Hard TTS deadline: min(broadcast_delay - 500ms, 5s absolute cap)
    let tts_deadline = {
        let delay_ms: u64 = std::env::var("BROADCAST_DELAY_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5000);
        let sync_deadline = Duration::from_millis(delay_ms.saturating_sub(500));
        let hard_cap = Duration::from_secs(5);
        sync_deadline.min(hard_cap)
    };

    // Use cloned voice if available, otherwise fall back to default per-language voice
    let voice_id = match voice_clone_id {
        Some(id) => id.to_string(),
        None => lang.voice_id().to_string(),
    };
    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
        &voice_id
    );

    let is_cloned = voice_clone_id.is_some();
    // Use higher-quality model for cloned voices, flash for defaults
    let model_id = if is_cloned { "eleven_multilingual_v2" } else { "eleven_flash_v2_5" };
    println!(
        "[TTS] requesting ElevenLabs voice={}{} model={} for '{}' ({}) [style={:.2} stability={:.2} speed={:.2}] [deadline={}ms]",
        &voice_id, if is_cloned { " (cloned)" } else { "" }, model_id, text, lang,
        style_params.style, style_params.stability, style_params.speed,
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

    // Wrap entire TTS call + streaming in a hard timeout.
    // If ElevenLabs exceeds the deadline, drop this utterance to silence.
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
                println!("[TTS] buffering MP3 for {}", lang_str);
                let mut stream = r.bytes_stream();
                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => audio_buffer.extend_from_slice(&chunk),
                        Err(e) => {
                            eprintln!("TTS stream error for {}: {}", lang_str, e);
                            break;
                        }
                    }
                }
            }
            Ok(r) => eprintln!("TTS error: {} - {:?}", r.status(), r.text().await),
            Err(e) => eprintln!("TTS request error for {}: {}", lang_str, e),
        }
        audio_buffer
    })
    .await;

    let audio_buffer = match tts_result {
        Ok(buf) if !buf.is_empty() => buf,
        Ok(_) => return, // empty buffer (TTS failed but didn't timeout)
        Err(_) => {
            let preview: String = text.chars().take(10).collect();
            eprintln!(
                "[TTS] Timeout for utterance \"{}...\", skipping",
                preview
            );
            eprintln!(
                "[TTS] TIMEOUT: utterance {} for {} exceeded {}ms deadline — dropping to silence",
                utterance_id, lang, tts_deadline.as_millis()
            );
            return;
        }
    };

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    println!("[TTS] buffered {}KB in {}ms for {}", audio_buffer.len() / 1024, tts_ms, lang);
    println!("[METRIC] tts_ms={} lang={} size_kb={}", tts_ms, lang, audio_buffer.len() / 1024);

    // Push to RTMP streams (decode MP3 → PCM, queue for synced playback)
    let rtmp_mgr = rooms.get(room_id).and_then(|r| r.rtmp_manager.clone());
    if let Some(manager) = rtmp_mgr {
        match crate::ffmpeg::decode_mp3_to_pcm(&audio_buffer).await {
            Ok(mut pcm) => {
                // Truncate TTS audio that exceeds source utterance + 2s margin.
                // Prevents long translations (e.g., German) from bleeding into next segment.
                let utterance_dur = utterance_end.duration_since(utterance_start);
                let max_dur = utterance_dur + Duration::from_millis(2000);
                let max_bytes = (max_dur.as_secs_f64() * 88200.0) as usize;
                if pcm.len() > max_bytes {
                    eprintln!(
                        "[RTMP] Truncating TTS audio: {:.0}ms → {:.0}ms for {}",
                        pcm.len() as f64 / 88.2,
                        max_bytes as f64 / 88.2,
                        lang
                    );
                    crate::ffmpeg::truncate_with_fadeout(&mut pcm, max_bytes);
                }
                let mgr = manager.lock().await;
                mgr.queue_audio(&lang.to_string(), pcm, utterance_start);
                let pipeline_ms = tts_start.elapsed().as_millis() as u64;
                eprintln!(
                    "[RTMP] Queued audio for {} (pipeline: {}ms, plays at utterance_start)",
                    lang, pipeline_ms
                );
                println!("[METRIC] pipeline_ms={} lang={}", pipeline_ms, lang);
            }
            Err(e) => eprintln!("[RTMP] MP3→PCM decode failed: {}", e),
        }
    }

    // Send synced audio + video to WebSocket guests (full audio, no truncation)
    if let Some(room) = rooms.get(room_id) {
        // Audio
        room.send_to_lang(lang, to_ws(&ServerMsg::TtsStart { utterance_id }));
        room.send_to_lang(lang, Message::Binary(audio_buffer.into()));
        room.send_to_lang(lang, to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms }));
        room.send_to_host(to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms }));

        // Synced video frames (captured during the utterance)
        if !utterance_frames.is_empty() {
            room.send_to_lang(lang, to_ws(&ServerMsg::VideoStart {
                utterance_id,
                frame_count: utterance_frames.len() as u32,
            }));
            for frame in utterance_frames {
                room.send_to_lang(lang, to_ws(&ServerMsg::VideoFrame {
                    data: frame.data.clone(),
                }));
            }
            room.send_to_lang(lang, to_ws(&ServerMsg::VideoEnd { utterance_id }));
            eprintln!(
                "[TTS] sent {} synced video frames for utterance {} ({})",
                utterance_frames.len(), utterance_id, lang
            );
        }
    }
}

// ── Voice Cloning ────────────────────────────────────────

/// Convert raw PCM (44.1kHz, 16-bit, mono) to WAV bytes
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
    wav.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
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

/// Clone the host's voice using ElevenLabs Instant Voice Cloning
pub async fn clone_voice(pcm: Vec<u8>, rooms: &Rooms, room_id: &str) {
    let wav = pcm_to_wav(&pcm);
    let room_id_short = &room_id[..6.min(room_id.len())];

    println!("[VOICE_CLONE] starting clone for room {} ({} bytes PCM, {} bytes WAV)", room_id_short, pcm.len(), wav.len());

    let client = reqwest::Client::new();

    // ElevenLabs Add Voice endpoint (Instant Voice Clone)
    let form = reqwest::multipart::Form::new()
        .text("name", format!("brivva-host-{}", room_id_short))
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
            struct CloneResp {
                voice_id: String,
            }
            match r.json::<CloneResp>().await {
                Ok(parsed) => {
                    println!("[VOICE_CLONE] success! voice_id={}", parsed.voice_id);
                    if let Some(mut room) = rooms.get_mut(room_id) {
                        room.voice_clone_id = Some(parsed.voice_id.clone());
                        // Notify host that voice is ready
                        room.send_to_host(to_ws(&ServerMsg::VoiceReady {
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
            // Notify host of failure
            if let Some(room) = rooms.get(room_id) {
                room.send_to_host(to_ws(&ServerMsg::Error {
                    message: format!("Voice clone failed: {}", status),
                }));
            }
        }
        Err(e) => {
            eprintln!("[VOICE_CLONE] request error: {}", e);
            if let Some(room) = rooms.get(room_id) {
                room.send_to_host(to_ws(&ServerMsg::Error {
                    message: format!("Voice clone error: {}", e),
                }));
            }
        }
    }
}

/// Delete a cloned voice from ElevenLabs on room close
pub async fn delete_cloned_voice(voice_id: &str) {
    let client = reqwest::Client::new();
    let url = format!("https://api.elevenlabs.io/v1/voices/{}", voice_id);
    match client
        .delete(&url)
        .header("xi-api-key", &*ELEVENLABS_API_KEY)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            println!("[VOICE_CLONE] deleted cloned voice {}", voice_id);
        }
        Ok(r) => eprintln!("[VOICE_CLONE] delete error {}: {:?}", r.status(), r.text().await),
        Err(e) => eprintln!("[VOICE_CLONE] delete request error: {}", e),
    }
}

/// Helper to serialize a ServerMsg and wrap in a WS text frame
fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}
