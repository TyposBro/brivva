//! Translation pipeline: STT → Translate → TTS
//!
//! Host audio flows through:
//! 1. STT Wrapper (stt-wrapper:8766/asr) — clean interim/final events
//! 2. NLLB (nllb:8000/translate) — REST translation per active language
//! 3. ElevenLabs TTS — streaming MP3 response (with optional cloned voice)

use axum::extract::ws::Message;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use crate::types::{Lang, Rooms, ServerMsg};

// ── Service URLs ──────────────────────────────────────────

static STT_URL: LazyLock<String> = LazyLock::new(|| {
    let host = std::env::var("STT_HOST").unwrap_or_else(|_| "localhost".to_string());
    let port = std::env::var("STT_PORT").unwrap_or_else(|_| "8766".to_string());
    format!("ws://{}:{}/asr", host, port)
});
static NLLB_URL: LazyLock<String> = LazyLock::new(|| {
    let host = std::env::var("NLLB_HOST").unwrap_or_else(|_| "localhost".to_string());
    format!("http://{}:8000/translate", host)
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

/// Emit a final transcript: broadcast to room and trigger translation pipeline
fn emit_final(rooms: &Rooms, room_id: &str, transcript: &str, uid: u64, source_lang: &Lang, style_params: Option<StyleParams>) {
    if let Some(room) = rooms.get(room_id) {
        let final_msg = to_ws(&ServerMsg::Final {
            transcript: transcript.to_string(),
            utterance_id: uid,
        });
        room.send_to_host(final_msg.clone());
        room.send_to_all_guests(final_msg);

        let active = room.active_langs();
        let sp = style_params.unwrap_or_default();
        println!("[PIPELINE] active langs: {:?}, style: {:.2}", active, sp.style);
        if !active.is_empty() {
            let rooms_clone = rooms.clone();
            let rid = room_id.to_string();
            let src = source_lang.clone();
            let text = transcript.to_string();
            tokio::spawn(async move {
                run_pipeline(&text, uid, &src, &active, &rooms_clone, &rid, &sp).await;
            });
        }
    }
}

/// Connect to STT wrapper and stream audio / receive clean events
pub async fn start_stt(
    room_id: String,
    rooms: Rooms,
    source_lang: Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    // Connect to STT wrapper with retries
    let mut ws_stream = None;
    for attempt in 1..=10 {
        match tokio_tungstenite::connect_async(&*STT_URL).await {
            Ok((stream, _)) => {
                println!("Connected to STT wrapper (attempt {})", attempt);
                ws_stream = Some(stream);
                break;
            }
            Err(e) => {
                eprintln!("STT connect attempt {}/10 failed: {}", attempt, e);
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            }
        }
    }
    let ws_stream = match ws_stream {
        Some(s) => s,
        None => {
            eprintln!("Failed to connect to STT wrapper after 10 attempts");
            return;
        }
    };

    let (mut stt_sink, mut stt_stream) = ws_stream.split();
    let mut audio_rx = audio_rx;

    // Task 1: Forward host audio → STT wrapper
    let send_task = tokio::spawn(async move {
        while let Some(data) = audio_rx.recv().await {
            if stt_sink
                .send(tungstenite::Message::Binary(data.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Task 2: Read clean interim/final events from STT wrapper
    let rooms_ref = rooms.clone();
    let rid = room_id.clone();
    let mut utterance_counter: u64 = 0;

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stt_stream.next().await {
            let text = match msg {
                tungstenite::Message::Text(t) => t.to_string(),
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
                None => break,
            };

            match event.event_type.as_str() {
                "final" => {
                    utterance_counter += 1;
                    let uid = utterance_counter;
                    println!("[FINAL #{}] {}", uid, event.text);
                    let style_params = event.style_params.clone();
                    drop(room);
                    emit_final(&rooms_ref, &rid, &event.text, uid, &source_lang, style_params);
                }
                "interim" => {
                    println!("[INTERIM] {}", event.text);
                    let msg = to_ws(&ServerMsg::Interim {
                        transcript: event.text,
                    });
                    room.send_to_host(msg.clone());
                    room.send_to_all_guests(msg);
                }
                "error" => {
                    eprintln!("[STT] error: {}", event.message);
                    break;
                }
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

// ── Translation Pipeline ──────────────────────────────────

#[derive(Serialize)]
struct NllbRequest {
    text: String,
    source_lang: String,
    target_lang: String,
}

#[derive(Deserialize)]
struct NllbResponse {
    translated_text: String,
    translate_ms: i64,
}

/// Run the full translation + TTS pipeline for one utterance
async fn run_pipeline(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_langs: &[Lang],
    rooms: &Rooms,
    room_id: &str,
    style_params: &StyleParams,
) {
    let client = reqwest::Client::new();
    let mut handles = Vec::new();

    // Get cloned voice ID if available
    let voice_clone_id = rooms.get(room_id).and_then(|r| r.voice_clone_id.clone());

    for lang in target_langs {
        if lang == source_lang {
            let transcript = transcript.to_string();
            let lang = lang.clone();
            let client = client.clone();
            let rooms = rooms.clone();
            let room_id = room_id.to_string();
            let voice_clone_id = voice_clone_id.clone();
            let sp = style_params.clone();

            handles.push(tokio::spawn(async move {
                do_tts_and_broadcast(
                    &client, &transcript, 0, utterance_id, &lang, &rooms, &room_id,
                    voice_clone_id.as_deref(), &sp,
                )
                .await;
            }));
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

        handles.push(tokio::spawn(async move {
            // 1. Translate
            let start = Instant::now();
            let nllb_resp = client
                .post(&*NLLB_URL)
                .json(&NllbRequest {
                    text: transcript.clone(),
                    source_lang: source.to_string(),
                    target_lang: target.to_string(),
                })
                .send()
                .await;

            let translated_text = match nllb_resp {
                Ok(resp) => match resp.json::<NllbResponse>().await {
                    Ok(r) => r.translated_text,
                    Err(e) => {
                        eprintln!("NLLB parse error for {}: {}", target, e);
                        return;
                    }
                },
                Err(e) => {
                    eprintln!("NLLB request error for {}: {}", target, e);
                    return;
                }
            };
            let translate_ms = start.elapsed().as_millis() as u64;
            println!(
                "[TRANSLATE] {} → {} = '{}' ({}ms)",
                source, target, translated_text, translate_ms
            );

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

            // 3. TTS + send audio
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
            )
            .await;
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
}

/// Call ElevenLabs TTS and send audio to guests
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
) {
    let tts_start = Instant::now();

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
    println!(
        "[TTS] requesting ElevenLabs voice={}{} for '{}' ({}) [style={:.2} stability={:.2} speed={:.2}]",
        &voice_id, if is_cloned { " (cloned)" } else { "" }, text, lang,
        style_params.style, style_params.stability, style_params.speed
    );

    let tts_body = serde_json::json!({
        "text": text,
        "model_id": "eleven_flash_v2_5",
        "voice_settings": {
            "stability": style_params.stability,
            "similarity_boost": style_params.similarity_boost,
            "style": style_params.style,
            "use_speaker_boost": style_params.use_speaker_boost
        }
    });

    let tts_resp = client
        .post(&url)
        .header("xi-api-key", &*ELEVENLABS_API_KEY)
        .header("Content-Type", "application/json")
        .json(&tts_body)
        .send()
        .await;

    // Buffer ALL audio
    let mut audio_buffer: Vec<u8> = Vec::new();

    match tts_resp {
        Ok(resp) if resp.status().is_success() => {
            println!("[TTS] buffering MP3 for {}", lang);
            let mut stream = resp.bytes_stream();
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => audio_buffer.extend_from_slice(&chunk),
                    Err(e) => {
                        eprintln!("TTS stream error for {}: {}", lang, e);
                        break;
                    }
                }
            }
        }
        Ok(resp) => eprintln!("TTS error: {} - {:?}", resp.status(), resp.text().await),
        Err(e) => eprintln!("TTS request error for {}: {}", lang, e),
    }

    let tts_ms = tts_start.elapsed().as_millis() as u64;
    println!("[TTS] buffered {}KB in {}ms for {}", audio_buffer.len() / 1024, tts_ms, lang);

    if audio_buffer.is_empty() {
        return;
    }

    // Send audio to guests
    if let Some(room) = rooms.get(room_id) {
        room.send_to_lang(lang, to_ws(&ServerMsg::TtsStart { utterance_id }));
        room.send_to_lang(lang, Message::Binary(audio_buffer.into()));
        room.send_to_lang(lang, to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms }));
        room.send_to_host(to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms }));
    }
}

// ── Voice Cloning ────────────────────────────────────────

/// Convert raw PCM (16kHz, 16-bit, mono) to WAV bytes
fn pcm_to_wav(pcm: &[u8]) -> Vec<u8> {
    let sample_rate: u32 = 16000;
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
                        room.voice_clone_id = Some(parsed.voice_id);
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
