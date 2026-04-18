//! Translation pipeline: STT → Translate → TTS
//!
//! Host audio flows through:
//! 1. STT (Soniox v4, direct WS) — delivers source transcript + per-target translations
//! 2. (Translation done inside Soniox — no separate translate step)
//! 3. ElevenLabs TTS — streaming MP3 response (with optional cloned voice)
//!
//! TODO: wire direct Soniox WS client. Current code still connects to legacy
//! stt-wrapper over WS at $STT_URL and performs Google Translate — both pruned
//! from infra. See SONIOX_API_KEY env and migrate STT_URL to wss://stt-rt.soniox.com.

use axum::extract::ws::Message;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use crate::types::{FrameBuffer, Lang, Rooms, ServerMsg, TimestampedFrame};

// ── Service URLs ──────────────────────────────────────────

static STT_URL: LazyLock<String> = LazyLock::new(|| {
    let host = std::env::var("STT_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = std::env::var("STT_PORT").unwrap_or_else(|_| "8766".to_string());
    format!("ws://{}:{}/asr", host, port)
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

#[derive(Debug, Deserialize)]
struct SttEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    style_params: Option<StyleParams>,
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

/// Max reconnect attempts for STT WebSocket mid-session
const STT_RECONNECT_MAX: u32 = 5;
/// Delay between STT reconnect attempts
const STT_RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Connect to STT wrapper and stream audio / receive clean events.
/// If the WebSocket disconnects mid-session, attempts to reconnect up to 5 times.
pub async fn start_stt(
    room_id: String,
    rooms: Rooms,
    source_lang: Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    // Wrap audio_rx in Arc<Mutex> so it can be reused across reconnections
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let mut utterance_counter: u64 = 0;
    let mut reconnect_count: u32 = 0;

    // Shared buffer: accumulate host audio chunks for source-language passthrough.
    // Persists across reconnections so we don't lose audio.
    let audio_acc: Arc<std::sync::Mutex<Vec<Vec<u8>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    loop {
        // Connect to STT wrapper (initial = 10 attempts, reconnects = 5)
        let max_attempts = if reconnect_count == 0 { 10 } else { STT_RECONNECT_MAX };
        let mut ws_stream = None;

        for attempt in 1..=max_attempts {
            if !rooms.contains_key(&room_id) {
                eprintln!("[STT] Room {} gone, stopping STT pipeline", room_id);
                return;
            }

            if reconnect_count > 0 {
                eprintln!(
                    "[STT] Disconnected, reconnecting (attempt {}/{})...",
                    attempt, STT_RECONNECT_MAX
                );
            }

            let stt_url = format!("{}?lang={}&sample_rate=44100", &*STT_URL, source_lang);
            match tokio_tungstenite::connect_async(&stt_url).await {
                Ok((stream, _)) => {
                    if reconnect_count > 0 {
                        eprintln!("[STT] Reconnected to STT wrapper (attempt {})", attempt);
                    } else {
                        println!("Connected to STT wrapper (attempt {})", attempt);
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
                    eprintln!("STT connect attempt {}/{} failed: {}", attempt, max_attempts, e);
                    tokio::time::sleep(delay).await;
                }
            }
        }

        let ws_stream = match ws_stream {
            Some(s) => s,
            None => {
                if reconnect_count > 0 {
                    eprintln!(
                        "[STT] Failed to reconnect after {} attempts, stopping pipeline",
                        STT_RECONNECT_MAX
                    );
                } else {
                    eprintln!("Failed to connect to STT wrapper after {} attempts", max_attempts);
                }
                return;
            }
        };

        let (mut stt_sink, mut stt_stream) = ws_stream.split();

        // Task 1: Forward host audio -> STT wrapper + accumulate for passthrough
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

        // Task 2: Read events from STT wrapper
        let rooms_ref = rooms.clone();
        let rid = room_id.clone();
        let source_lang_clone = source_lang.clone();
        let acc_rx = audio_acc.clone();
        let mut uc = utterance_counter;
        let mut utterance_start: Option<Instant> = None;

        // Track whether disconnect was unexpected (should trigger reconnect)
        let disconnected_unexpectedly = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let disc_flag = disconnected_unexpectedly.clone();

        let recv_task = tokio::spawn(async move {
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
                        eprintln!("[STT] WebSocket closed by server");
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                    _ => continue,
                };

                let event: SttEvent = match serde_json::from_str(&text) {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("[STT] parse error: {}", e);
                        continue;
                    }
                };

                let room = match rooms_ref.get(&rid) {
                    Some(r) => r,
                    None => break, // room gone = normal shutdown
                };

                match event.event_type.as_str() {
                    "final" => {
                        uc += 1;
                        let uid = uc;
                        println!("[FINAL #{}] {}", uid, event.text);
                        let style_params = event.style_params.clone();
                        let start = utterance_start.take().unwrap_or_else(Instant::now);
                        let host_audio = {
                            let mut acc = acc_rx.lock().unwrap();
                            let chunks: Vec<u8> = acc.drain(..).flatten().collect();
                            chunks
                        };
                        drop(room);
                        emit_final(&rooms_ref, &rid, &event.text, uid, &source_lang_clone, style_params, start, host_audio);
                    }
                    "interim" => {
                        if utterance_start.is_none() {
                            utterance_start = Some(Instant::now());
                        }
                        println!("[INTERIM] {}", event.text);
                        let msg = to_ws(&ServerMsg::Interim {
                            transcript: event.text,
                        });
                        room.send_to_host(msg.clone());
                        room.send_to_all_guests(msg);
                    }
                    "error" => {
                        eprintln!("[STT] error: {}", event.message);
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                    _ => {}
                }
            }
            uc
        });

        // Wait for either task to finish
        tokio::select! {
            _ = send_task => {},
            result = recv_task => {
                if let Ok(uc) = result {
                    utterance_counter = uc;
                }
            },
        }

        // Decide whether to reconnect
        if !disconnected_unexpectedly.load(std::sync::atomic::Ordering::Acquire) {
            // Normal shutdown (room removed, host disconnected, audio_rx closed)
            break;
        }

        if !rooms.contains_key(&room_id) {
            break;
        }

        reconnect_count += 1;
        if reconnect_count > STT_RECONNECT_MAX {
            eprintln!("[STT] Exceeded max reconnect attempts ({}), giving up", STT_RECONNECT_MAX);
            break;
        }

        eprintln!("[STT] Will attempt reconnect {}/{}", reconnect_count, STT_RECONNECT_MAX);
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
