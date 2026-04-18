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

use crate::types::{Lang, Rooms, ServerMsg};

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

/// Emit a final transcript: echo to the host WS and spawn per-target TTS.
/// Original audio has already flowed to RTMP continuously via the mixer, so
/// the pipeline no longer needs host PCM or utterance timestamps here.
fn emit_final(
    rooms: &Rooms,
    room_id: &str,
    transcript: &str,
    uid: u64,
    source_lang: &Lang,
) {
    if let Some(room) = rooms.get(room_id) {
        room.send_to_host(to_ws(&ServerMsg::Final {
            transcript: transcript.to_string(),
            utterance_id: uid,
        }));

        let active = room.active_langs();
        println!("[PIPELINE] active langs: {:?}", active);
        if !active.is_empty() {
            let rooms_clone = rooms.clone();
            let rid = room_id.to_string();
            let src = source_lang.clone();
            let text = transcript.to_string();
            tokio::spawn(async move {
                run_pipeline(&text, uid, &src, &active, &rooms_clone, &rid).await;
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

        // Task 1: Forward host audio → Soniox. Host PCM also flows to the RTMP
        // mixer via handler.rs::push_host_audio, independent of STT.
        let audio_rx_clone = audio_rx.clone();
        let send_task = tokio::spawn(async move {
            let mut rx = audio_rx_clone.lock().await;
            while let Some(data) = rx.recv().await {
                if stt_sink
                    .send(tungstenite::Message::Binary(data.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Task 2: Read Soniox tokens, emit Interim/Final events to the host.
        let rooms_ref = rooms.clone();
        let rid = room_id.clone();
        let source_lang_clone = source_lang.clone();
        let mut uc = utterance_counter;

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
                    } else {
                        interim_tail.push_str(&tok.text);
                    }
                }

                // Emit an Interim (only when there's something to show — drops empty keep-alives).
                let interim = format!("{}{}", final_text, interim_tail);
                if !interim.is_empty() {
                    room.send_to_host(to_ws(&ServerMsg::Interim {
                        transcript: interim,
                    }));
                }

                // Endpoint detected → commit the utterance.
                if endpoint_hit && !final_text.trim().is_empty() {
                    uc += 1;
                    let uid = uc;
                    let committed = std::mem::take(&mut final_text);
                    println!("[FINAL #{}] {}", uid, committed);
                    drop(room);
                    emit_final(&rooms_ref, &rid, &committed, uid, &source_lang_clone);
                }
                // Endpoints with no text (silence) are no-ops; the mixer has
                // already been forwarding the original audio.
                let _ = endpoint_hit;
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
// Each translated target spawns one task: translate (stub) + TTS. The host's
// original audio/video flows to RTMP continuously via the per-stream delay
// buffers in ffmpeg.rs, so there's no source-language passthrough branch and
// no host_audio / utterance_* timestamps to thread through.

/// Run the per-target TTS pipeline for one utterance.
async fn run_pipeline(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_langs: &[Lang],
    rooms: &Rooms,
    room_id: &str,
) {
    let mut handles = Vec::new();
    let voice_clone_id = rooms.get(room_id).and_then(|r| r.voice_clone_id.clone());

    for lang in target_langs {
        // Source-language RTMP output is handled by the per-stream delay
        // buffer + mixer (passthrough, no TTS). Skip TTS for source here.
        if lang == source_lang {
            continue;
        }

        let transcript = transcript.to_string();
        let target = lang.clone();
        let rooms = rooms.clone();
        let room_id = room_id.to_string();
        let voice_clone_id = voice_clone_id.clone();

        handles.push(tokio::spawn(async move {
            // Translation stub: pass-through until Soniox per-target WS lands.
            let translated_text = transcript;
            let translate_ms: u64 = 0;

            if let Some(room) = rooms.get(&room_id) {
                room.send_to_host(to_ws(&ServerMsg::Translation {
                    text: translated_text.clone(),
                    utterance_id,
                    target_lang: target.to_string(),
                    translate_ms,
                }));
            }

            do_tts_and_broadcast(
                &translated_text,
                utterance_id,
                &target,
                &rooms,
                &room_id,
                voice_clone_id.as_deref(),
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
/// Call ElevenLabs TTS and push the resulting PCM into the target stream's
/// mixer queue. No per-utterance timing — the mixer plays it in order and
/// the 5 s queue cap keeps slow targets from falling too far behind.
async fn do_tts_and_broadcast(
    text: &str,
    utterance_id: u64,
    lang: &Lang,
    rooms: &Rooms,
    room_id: &str,
    voice_clone_id: Option<&str>,
) {
    let tts_start = Instant::now();
    // Hard cap of 5s per utterance — ElevenLabs hanging on one line
    // shouldn't block the whole pipeline.
    let tts_deadline = Duration::from_secs(5);

    let is_cloned = voice_clone_id.is_some();
    let voice_id = voice_clone_id
        .map(str::to_string)
        .unwrap_or_else(|| lang.voice_id().to_string());
    let model_id = if is_cloned { "eleven_multilingual_v2" } else { "eleven_flash_v2_5" };
    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
        &voice_id
    );
    println!(
        "[TTS] voice={}{} model={} lang={} deadline={}ms text='{}'",
        &voice_id, if is_cloned { " (cloned)" } else { "" }, model_id, lang,
        tts_deadline.as_millis(), text
    );

    let client = reqwest::Client::new();
    let tts_body = serde_json::json!({ "text": text, "model_id": model_id });
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
        Ok(_) => return,
        Err(_) => {
            eprintln!(
                "[TTS] TIMEOUT utterance {} lang={} — dropped to silence (>{:?})",
                utterance_id, lang, tts_deadline
            );
            return;
        }
    };

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    println!(
        "[TTS] buffered {}KB in {}ms lang={}",
        audio_buffer.len() / 1024,
        tts_ms,
        lang
    );

    // RTMP path: decode MP3 → PCM, append to this stream's TTS queue. No
    // timestamp alignment — the per-stream mixer plays it in arrival order,
    // overlaid on the delayed host audio at emit time.
    let rtmp_mgr = rooms.get(room_id).and_then(|r| r.rtmp_manager.clone());
    if let Some(manager) = rtmp_mgr {
        match crate::ffmpeg::decode_mp3_to_pcm(&audio_buffer).await {
            Ok(pcm) => {
                let mgr = manager.lock().await;
                mgr.push_tts(&lang.to_string(), pcm);
            }
            Err(e) => eprintln!("[RTMP] MP3→PCM decode failed: {}", e),
        }
    }

    // Host-side latency markers.
    if let Some(room) = rooms.get(room_id) {
        room.send_to_host(to_ws(&ServerMsg::TtsEnd {
            utterance_id,
            target_lang: lang.to_string(),
            tts_ms,
        }));
        room.send_to_host(to_ws(&ServerMsg::VideoEnd { utterance_id }));
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
