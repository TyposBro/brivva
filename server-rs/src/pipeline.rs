//! Translation pipeline: STT → Translate → TTS
//!
//! Host audio flows through:
//! 1. WhisperLiveKit (localhost:8765/asr) — streaming STT via WebSocket
//! 2. NLLB (localhost:8000/translate) — REST translation per active language
//! 3. Kokoro (localhost:8880/v1/audio/speech) — REST TTS, streaming MP3 response

use axum::extract::ws::Message;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;

use crate::types::{Lang, Rooms, ServerMsg};

// ── STT (WhisperLiveKit) ──────────────────────────────────

const STT_URL: &str = "ws://localhost:8765/asr";

/// WhisperLiveKit response format
#[derive(Debug, Deserialize)]
struct SttResponse {
    #[serde(default)]
    status: String,
    #[serde(default)]
    lines: Vec<SttLine>,
    #[serde(default)]
    buffer_transcription: String,
    #[serde(rename = "type")]
    #[serde(default)]
    msg_type: String,
}

#[derive(Debug, Deserialize)]
struct SttLine {
    text: String,
}

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

/// Connect to WhisperLiveKit and return a sender for audio + handle responses
pub async fn start_stt(
    room_id: String,
    rooms: Rooms,
    source_lang: Lang,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    // Retry STT connection — WhisperLiveKit may still be loading the model
    let mut ws_stream = None;
    for attempt in 1..=10 {
        match tokio_tungstenite::connect_async(STT_URL).await {
            Ok((stream, _)) => {
                println!("Connected to STT (attempt {})", attempt);
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
            eprintln!("Failed to connect to STT after 10 attempts");
            return;
        }
    };

    let (mut stt_sink, mut stt_stream) = ws_stream.split();
    let mut audio_rx = audio_rx;

    // Task 1: Forward host audio → STT WebSocket
    let send_task = tokio::spawn(async move {
        // Wait for the config message before sending audio
        // WhisperLiveKit sends { "type": "config", ... } first
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

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

    // Task 2: Read STT responses → broadcast interims/finals, trigger pipeline
    //
    // WhisperLiveKit has two modes:
    // 1. buffer_transcription fills up → committed to lines → buffer clears
    // 2. Text goes directly into lines (with --no-vac / base models)
    //
    // Strategy: track BOTH buffer and lines text. Detect changes in either.
    let rooms_ref = rooms.clone();
    let rid = room_id.clone();
    let mut utterance_counter: u64 = 0;
    let mut last_interim = String::new();
    let mut prev_buffer_was_nonempty = false;
    let mut seen_line_texts: Vec<String> = Vec::new();

    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = stt_stream.next().await {
            let text = match msg {
                tungstenite::Message::Text(t) => t.to_string(),
                _ => continue,
            };

            let resp: SttResponse = match serde_json::from_str(&text) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[STT PARSE ERROR] {}", e);
                    continue;
                }
            };

            // Skip config messages
            if resp.msg_type == "config" || resp.msg_type == "ready_to_stop" {
                continue;
            }

            let room = match rooms_ref.get(&rid) {
                Some(r) => r,
                None => break, // room closed
            };

            // --- Path 1: buffer_transcription (interim text) ---
            if !resp.buffer_transcription.is_empty() {
                if resp.buffer_transcription != last_interim {
                    last_interim = resp.buffer_transcription.clone();
                    println!("[INTERIM] {}", last_interim);
                    let msg = to_ws(&ServerMsg::Interim {
                        transcript: last_interim.clone(),
                    });
                    room.send_to_host(msg.clone());
                    room.send_to_all_guests(msg);
                }
                prev_buffer_was_nonempty = true;
            }

            // Buffer went from non-empty → empty = text was committed (final)
            if resp.buffer_transcription.is_empty() && prev_buffer_was_nonempty {
                prev_buffer_was_nonempty = false;
                let transcript = last_interim.trim().to_string();
                last_interim.clear();

                if !transcript.is_empty() {
                    utterance_counter += 1;
                    let uid = utterance_counter;
                    println!("[FINAL #{}] {} (from buffer)", uid, transcript);
                    emit_final(&rooms_ref, &rid, &transcript, uid, &source_lang);
                }
            }

            // --- Path 2: lines text changed (direct-to-lines mode) ---
            for (i, line) in resp.lines.iter().enumerate() {
                let line_text = line.text.trim().to_string();
                if line_text.is_empty() {
                    continue;
                }

                // Extend seen_line_texts if needed
                while seen_line_texts.len() <= i {
                    seen_line_texts.push(String::new());
                }

                if seen_line_texts[i] != line_text {
                    seen_line_texts[i] = line_text.clone();
                    utterance_counter += 1;
                    let uid = utterance_counter;
                    println!("[FINAL #{}] {} (from line {})", uid, line_text, i);
                    emit_final(&rooms_ref, &rid, &line_text, uid, &source_lang);
                }
            }
        }
    });

    // Wait for either task to finish (host disconnected or STT closed)
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

// ── Translation Pipeline ──────────────────────────────────

const NLLB_URL: &str = "http://localhost:8000/translate";
const KOKORO_URL: &str = "http://localhost:8880/v1/audio/speech";

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
struct KokoroRequest {
    model: String,
    input: String,
    voice: String,
    response_format: String,
}

/// Run the full translation + TTS pipeline for one utterance across all active languages
async fn run_pipeline(
    transcript: &str,
    utterance_id: u64,
    source_lang: &Lang,
    target_langs: &[Lang],
    rooms: &Rooms,
    room_id: &str,
) {
    let client = reqwest::Client::new();

    // Translate to all target languages in parallel (tokio::join! equivalent)
    let mut translate_handles = Vec::new();

    for lang in target_langs {
        // Don't translate if source == target
        if lang == source_lang {
            // Still need to do TTS for same-language listeners
            let transcript = transcript.to_string();
            let lang = lang.clone();
            let client = client.clone();
            let rooms = rooms.clone();
            let room_id = room_id.to_string();

            translate_handles.push(tokio::spawn(async move {
                do_tts_and_broadcast(
                    &client,
                    &transcript,
                    0,
                    utterance_id,
                    &lang,
                    &rooms,
                    &room_id,
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

        translate_handles.push(tokio::spawn(async move {
            // 1. Translate
            let start = Instant::now();
            let nllb_resp = client
                .post(NLLB_URL)
                .json(&NllbRequest {
                    text: transcript.clone(),
                    source_lang: source.to_string(),
                    target_lang: target.to_string(),
                })
                .send()
                .await;

            let (translated_text, _translate_ms) = match nllb_resp {
                Ok(resp) => match resp.json::<NllbResponse>().await {
                    Ok(r) => (r.translated_text, r.translate_ms as u64),
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
            let translate_ms_actual = start.elapsed().as_millis() as u64;
            println!("[TRANSLATE] {} → {} = '{}' ({}ms)", source, target, translated_text, translate_ms_actual);

            // 2. Broadcast translation text to language group
            if let Some(room) = rooms.get(&room_id) {
                let msg = to_ws(&ServerMsg::Translation {
                    text: translated_text.clone(),
                    utterance_id,
                    translate_ms: translate_ms_actual,
                });
                room.send_to_lang(&target, msg.clone());
                room.send_to_host(msg);
            }

            // 3. TTS + stream audio
            do_tts_and_broadcast(
                &client,
                &translated_text,
                translate_ms_actual,
                utterance_id,
                &target,
                &rooms,
                &room_id,
            )
            .await;
        }));
    }

    // Wait for all languages to finish
    for handle in translate_handles {
        let _ = handle.await;
    }
}

/// Call Kokoro TTS and stream MP3 chunks to all guests in a language group
async fn do_tts_and_broadcast(
    client: &reqwest::Client,
    text: &str,
    _translate_ms: u64,
    utterance_id: u64,
    lang: &Lang,
    rooms: &Rooms,
    room_id: &str,
) {
    let tts_start = Instant::now();

    // Send tts_start
    if let Some(room) = rooms.get(room_id) {
        room.send_to_lang(lang, to_ws(&ServerMsg::TtsStart { utterance_id }));
    }

    // Call Kokoro
    println!("[TTS] requesting voice={} for '{}' ({})", lang.voice(), text, lang);
    let tts_resp = client
        .post(KOKORO_URL)
        .json(&KokoroRequest {
            model: "kokoro".to_string(),
            input: text.to_string(),
            voice: lang.voice().to_string(),
            response_format: "mp3".to_string(),
        })
        .send()
        .await;

    match tts_resp {
        Ok(resp) if resp.status().is_success() => {
            println!("[TTS] streaming MP3 chunks for {}", lang);
            // Stream MP3 chunks as they arrive
            let mut stream = resp.bytes_stream();
            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if let Some(room) = rooms.get(room_id) {
                            room.send_to_lang(
                                lang,
                                Message::Binary(chunk.to_vec().into()),
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("TTS stream error for {}: {}", lang, e);
                        break;
                    }
                }
            }
        }
        Ok(resp) => {
            eprintln!("Kokoro error: {}", resp.status());
        }
        Err(e) => {
            eprintln!("Kokoro request error for {}: {}", lang, e);
        }
    }

    // Send tts_end
    let tts_ms = tts_start.elapsed().as_millis() as u64;
    if let Some(room) = rooms.get(room_id) {
        let msg = to_ws(&ServerMsg::TtsEnd {
            utterance_id,
            tts_ms,
        });
        room.send_to_lang(lang, msg.clone());
        room.send_to_host(msg);
    }
}

/// Helper to serialize a ServerMsg and wrap in a WS text frame
fn to_ws(msg: &ServerMsg) -> Message {
    Message::Text(serde_json::to_string(msg).unwrap().into())
}
