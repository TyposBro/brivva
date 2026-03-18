//! Translation pipeline: STT → Translate → TTS → Lip-sync
//!
//! Host audio flows through:
//! 1. STT Wrapper (stt-wrapper:8766/asr) — clean interim/final events
//! 2. NLLB (nllb:8000/translate) — REST translation per active language
//! 3. ElevenLabs TTS — streaming MP3 response
//! 4. MuseTalk (musetalk:8100/lipsync) — lip-synced video frames

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
static LIPSYNC_URL: LazyLock<String> = LazyLock::new(|| {
    let host = std::env::var("LIPSYNC_HOST").unwrap_or_else(|_| "localhost".to_string());
    format!("http://{}:8100", host)
});

// ── STT Events (from stt-wrapper) ─────────────────────────

#[derive(Debug, Deserialize)]
struct SttEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    message: String,
}

// ── STT Connection ────────────────────────────────────────

/// Emit a final transcript: broadcast to room and trigger translation pipeline
fn emit_final(rooms: &Rooms, room_id: &str, transcript: &str, uid: u64, source_lang: &Lang) {
    if let Some(room) = rooms.get(room_id) {
        let final_msg = to_ws(&ServerMsg::Final {
            transcript: transcript.to_string(),
            utterance_id: uid,
        });
        room.send_to_host(final_msg.clone());
        room.send_to_all_guests(final_msg);

        let active = room.active_langs();
        let latest_face = room.latest_face.clone();
        println!("[PIPELINE] active langs: {:?}, has_face: {}", active, latest_face.is_some());
        if !active.is_empty() {
            let rooms_clone = rooms.clone();
            let rid = room_id.to_string();
            let src = source_lang.clone();
            let text = transcript.to_string();
            tokio::spawn(async move {
                run_pipeline(&text, uid, &src, &active, &rooms_clone, &rid, latest_face.as_deref()).await;
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
                    drop(room);
                    emit_final(&rooms_ref, &rid, &event.text, uid, &source_lang);
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

#[derive(Serialize)]
struct ElevenLabsRequest {
    text: String,
    model_id: String,
}

/// Run the full translation + TTS + lip-sync pipeline for one utterance
async fn run_pipeline(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_langs: &[Lang],
    rooms: &Rooms,
    room_id: &str,
    latest_face: Option<&str>,
) {
    let client = reqwest::Client::new();
    let mut handles = Vec::new();

    for lang in target_langs {
        if lang == source_lang {
            let transcript = transcript.to_string();
            let lang = lang.clone();
            let client = client.clone();
            let rooms = rooms.clone();
            let room_id = room_id.to_string();
            let latest_face = latest_face.map(|s| s.to_string());

            handles.push(tokio::spawn(async move {
                do_tts_and_broadcast(
                    &client, &transcript, 0, utterance_id, &lang, &rooms, &room_id,
                    latest_face.as_deref(),
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
        let latest_face = latest_face.map(|s| s.to_string());

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

            // 3. TTS + stream audio + lip-sync
            do_tts_and_broadcast(
                &client,
                &translated_text,
                translate_ms,
                utterance_id,
                &target,
                &rooms,
                &room_id,
                latest_face.as_deref(),
            )
            .await;
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
}

/// Call ElevenLabs TTS, then run lip-sync, then send audio+video together (synced)
async fn do_tts_and_broadcast(
    client: &reqwest::Client,
    text: &str,
    _translate_ms: u64,
    utterance_id: u64,
    lang: &Lang,
    rooms: &Rooms,
    room_id: &str,
    face_base64: Option<&str>,
) {
    let tts_start = Instant::now();

    let voice_id = lang.voice_id();
    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}/stream?output_format=mp3_44100_128",
        voice_id
    );

    println!(
        "[TTS] requesting ElevenLabs voice={} for '{}' ({})",
        voice_id, text, lang
    );
    let tts_resp = client
        .post(&url)
        .header("xi-api-key", &*ELEVENLABS_API_KEY)
        .header("Content-Type", "application/json")
        .json(&ElevenLabsRequest {
            text: text.to_string(),
            model_id: "eleven_flash_v2_5".to_string(),
        })
        .send()
        .await;

    // Buffer ALL audio (don't stream to guests yet — wait for lip-sync)
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

    // Report TTS timing to host
    if let Some(room) = rooms.get(room_id) {
        room.send_to_host(to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms }));
    }

    if audio_buffer.is_empty() {
        return;
    }

    // Run lip-sync if we have a face frame, then send audio+video together
    if let Some(face) = face_base64 {
        do_lipsync_and_broadcast(
            client,
            &audio_buffer,
            face,
            utterance_id,
            lang,
            rooms,
            room_id,
        )
        .await;
    } else {
        // No face — send audio only (fallback)
        if let Some(room) = rooms.get(room_id) {
            room.send_to_lang(lang, to_ws(&ServerMsg::TtsStart { utterance_id }));
            room.send_to_lang(lang, Message::Binary(audio_buffer.into()));
            room.send_to_lang(lang, to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms }));
        }
    }
}

// ── Lip-sync ─────────────────────────────────────────────

#[derive(Serialize)]
struct MusetalkLipsyncRequest {
    audio_base64: String,
    face_image_base64: String,
}

#[derive(Deserialize)]
struct MusetalkLipsyncResponse {
    frames_base64: Vec<String>,
    fps: u32,
    lipsync_ms: i64,
}

/// Call MuseTalk lip-sync, then send audio + video frames together (synced)
async fn do_lipsync_and_broadcast(
    client: &reqwest::Client,
    audio_mp3: &[u8],
    face_base64: &str,
    utterance_id: u64,
    lang: &Lang,
    rooms: &Rooms,
    room_id: &str,
) {
    use base64::Engine;
    let audio_base64 = base64::engine::general_purpose::STANDARD.encode(audio_mp3);

    let url = format!("{}/lipsync", *LIPSYNC_URL);
    println!("[LIPSYNC] requesting lip-sync for utterance {}", utterance_id);

    let lipsync_start = Instant::now();

    let resp = client
        .post(&url)
        .json(&MusetalkLipsyncRequest {
            audio_base64,
            face_image_base64: face_base64.to_string(),
        })
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            match r.json::<MusetalkLipsyncResponse>().await {
                Ok(parsed) => {
                    let lipsync_ms = lipsync_start.elapsed().as_millis() as u64;
                    println!(
                        "[LIPSYNC] got {} frames @{}fps ({}ms)",
                        parsed.frames_base64.len(),
                        parsed.fps,
                        lipsync_ms
                    );

                    // Send audio + video together so guest can play them in sync
                    if let Some(room) = rooms.get(room_id) {
                        // 1. Audio: tts_start → full MP3 blob → tts_end
                        room.send_to_lang(lang, to_ws(&ServerMsg::TtsStart { utterance_id }));
                        room.send_to_lang(lang, Message::Binary(audio_mp3.to_vec().into()));
                        room.send_to_lang(lang, to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms: lipsync_ms }));

                        // 2. Video: video_start → frames → video_end
                        room.send_to_lang(lang, to_ws(&ServerMsg::VideoStart { utterance_id }));
                        for frame_b64 in &parsed.frames_base64 {
                            room.send_to_lang(
                                lang,
                                to_ws(&ServerMsg::VideoFrame {
                                    utterance_id,
                                    data: frame_b64.clone(),
                                }),
                            );
                        }
                        room.send_to_lang(lang, to_ws(&ServerMsg::VideoEnd { utterance_id, lipsync_ms }));

                        // Report to host
                        room.send_to_host(to_ws(&ServerMsg::VideoEnd { utterance_id, lipsync_ms }));
                    }
                }
                Err(e) => eprintln!("[LIPSYNC] parse error: {}", e),
            }
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            eprintln!("[LIPSYNC] error {}: {}", status, body);
            // Fallback: send audio without video
            if let Some(room) = rooms.get(room_id) {
                room.send_to_lang(lang, to_ws(&ServerMsg::TtsStart { utterance_id }));
                room.send_to_lang(lang, Message::Binary(audio_mp3.to_vec().into()));
                let tts_ms = lipsync_start.elapsed().as_millis() as u64;
                room.send_to_lang(lang, to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms }));
            }
        }
        Err(e) => {
            eprintln!("[LIPSYNC] request error: {}", e);
            // Fallback: send audio without video
            if let Some(room) = rooms.get(room_id) {
                room.send_to_lang(lang, to_ws(&ServerMsg::TtsStart { utterance_id }));
                room.send_to_lang(lang, Message::Binary(audio_mp3.to_vec().into()));
                let tts_ms = lipsync_start.elapsed().as_millis() as u64;
                room.send_to_lang(lang, to_ws(&ServerMsg::TtsEnd { utterance_id, tts_ms }));
            }
        }
    }
}

/// Helper to serialize a ServerMsg and wrap in a WS text frame
fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}
