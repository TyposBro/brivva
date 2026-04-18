//! Translation pipeline: Soniox STT + translation → ElevenLabs TTS.
//!
//! One host WS produces a single audio stream. We fan that stream out to N+1
//! Soniox WS sessions:
//! - 1 **source** session (no `translation` config) that emits Interim/Final
//!   transcripts back to the host UI.
//! - N **translate** sessions (one per target lang, `translation.one_way` set)
//!   that emit a Translation event and drive ElevenLabs TTS for that lang.
//!
//! Source-lang RTMP output doesn't need a translate session — the per-stream
//! delay buffer + mixer in `ffmpeg.rs` handles it as a pure passthrough.

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
// First client message = JSON config (API key + audio format + optional
// translation). Then the client sends binary audio frames. The server replies
// with JSON: a `tokens` array plus an optional `error_code`/`error_message`.
// A token with `text == "<end>"` is Soniox's endpoint-detection sentinel.
//
// Per-token `translation_status`:
//   - `Some("original")`    — source-lang text (emitted by translate sessions)
//   - `Some("translation")` — target-lang text (emitted by translate sessions)
//   - `None`                — transcription-only tokens (source session)

#[derive(Debug, Serialize)]
struct SonioxConfig<'a> {
    api_key: &'a str,
    model: &'a str,
    audio_format: &'a str,
    sample_rate: u32,
    num_channels: u32,
    language_hints: Vec<String>,
    enable_endpoint_detection: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    translation: Option<SonioxTranslation>,
}

#[derive(Debug, Serialize)]
struct SonioxTranslation {
    #[serde(rename = "type")]
    kind: &'static str,
    target_language: String,
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
    #[serde(default)]
    translation_status: Option<String>,
}

// ── Session modes ─────────────────────────────────────────

#[derive(Clone)]
enum SonioxMode {
    /// Pure transcription of the source lang. Emits Interim + Final to host.
    Source { lang: Lang },
    /// Translation source → target. Emits Translation + fires TTS.
    Translate { source_lang: Lang, target_lang: Lang },
}

impl SonioxMode {
    fn tag(&self) -> String {
        match self {
            SonioxMode::Source { lang } => format!("src:{}", lang),
            SonioxMode::Translate { source_lang, target_lang } => {
                format!("{}→{}", source_lang, target_lang)
            }
        }
    }

    fn build_config<'a>(&self, api_key: &'a str) -> SonioxConfig<'a> {
        let (hint_lang, translation) = match self {
            SonioxMode::Source { lang } => (lang.to_string(), None),
            SonioxMode::Translate { source_lang, target_lang } => (
                source_lang.to_string(),
                Some(SonioxTranslation {
                    kind: "one_way",
                    target_language: target_lang.to_string(),
                }),
            ),
        };
        SonioxConfig {
            api_key,
            model: SONIOX_MODEL,
            audio_format: "pcm_s16le",
            sample_rate: HOST_SAMPLE_RATE,
            num_channels: 1,
            language_hints: vec![hint_lang],
            enable_endpoint_detection: true,
            translation,
        }
    }

    /// Decide whether this session should process a token. Translate sessions
    /// only consume tokens with `translation_status == "translation"` (the
    /// `"original"` ones are handled by the dedicated Source session).
    fn accepts(&self, tok: &SonioxToken) -> bool {
        if tok.text == SONIOX_END_TOKEN {
            return true;
        }
        match self {
            SonioxMode::Source { .. } => true,
            SonioxMode::Translate { .. } => {
                matches!(tok.translation_status.as_deref(), Some("translation"))
            }
        }
    }
}

// ── STT pipelines entry ──────────────────────────────────

/// Max reconnect attempts for the Soniox WebSocket mid-session.
const STT_RECONNECT_MAX: u32 = 5;
/// Delay between Soniox reconnect attempts.
const STT_RECONNECT_DELAY: Duration = Duration::from_secs(1);
/// Soniox's end-of-utterance sentinel token (endpoint detection).
const SONIOX_END_TOKEN: &str = "<end>";

/// Fan host audio out to one Source session (transcript → host UI) and one
/// Translate session per target language (→ TTS).
pub async fn start_stt_pipelines(
    room_id: String,
    rooms: Rooms,
    source_lang: Lang,
    target_langs: Vec<Lang>,
    mut audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    if SONIOX_API_KEY.is_empty() {
        eprintln!("[STT] SONIOX_API_KEY not set — STT pipeline disabled");
        return;
    }

    // Dedupe + drop source from targets (no self-translation).
    let mut seen = std::collections::HashSet::new();
    let targets: Vec<Lang> = target_langs
        .into_iter()
        .filter(|l| *l != source_lang && seen.insert(l.clone()))
        .collect();

    // One mpsc per Soniox session. Producer fans every chunk to each.
    let (source_tx, source_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let mut target_senders: Vec<mpsc::UnboundedSender<Vec<u8>>> = Vec::new();
    let mut target_receivers: Vec<(Lang, mpsc::UnboundedReceiver<Vec<u8>>)> = Vec::new();
    for lang in &targets {
        let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
        target_senders.push(tx);
        target_receivers.push((lang.clone(), rx));
    }

    tokio::spawn(async move {
        while let Some(chunk) = audio_rx.recv().await {
            let _ = source_tx.send(chunk.clone());
            for tx in &target_senders {
                let _ = tx.send(chunk.clone());
            }
        }
    });

    // Source session: transcript-only.
    {
        let rid = room_id.clone();
        let rooms = rooms.clone();
        let src = source_lang.clone();
        tokio::spawn(async move {
            run_soniox_session(rid, rooms, SonioxMode::Source { lang: src }, source_rx).await;
        });
    }

    // Per-target translate sessions.
    for (target_lang, target_rx) in target_receivers {
        let rid = room_id.clone();
        let rooms = rooms.clone();
        let src = source_lang.clone();
        tokio::spawn(async move {
            run_soniox_session(
                rid,
                rooms,
                SonioxMode::Translate {
                    source_lang: src,
                    target_lang,
                },
                target_rx,
            )
            .await;
        });
    }
}

/// Single Soniox session loop. Reconnects on mid-session drops up to
/// STT_RECONNECT_MAX. Token handling branches on `mode`:
///
/// - `Source`: emit Interim to the host on every response, emit Final on
///   endpoint detection.
/// - `Translate`: collect only translation tokens, emit one `Translation`
///   event per utterance + fire TTS for this target lang.
async fn run_soniox_session(
    room_id: String,
    rooms: Rooms,
    mode: SonioxMode,
    audio_rx: mpsc::UnboundedReceiver<Vec<u8>>,
) {
    let tag = mode.tag();
    let audio_rx = Arc::new(tokio::sync::Mutex::new(audio_rx));
    let mut utterance_counter: u64 = 0;
    let mut reconnect_count: u32 = 0;

    loop {
        let max_attempts = if reconnect_count == 0 { 10 } else { STT_RECONNECT_MAX };
        let mut ws_stream = None;

        for attempt in 1..=max_attempts {
            if !rooms.contains_key(&room_id) {
                eprintln!("[STT {}] room gone, stopping", tag);
                return;
            }

            match tokio_tungstenite::connect_async(SONIOX_WS_URL).await {
                Ok((stream, _)) => {
                    if reconnect_count > 0 {
                        eprintln!("[STT {}] reconnected (attempt {})", tag, attempt);
                    } else {
                        println!("[STT {}] connected (attempt {})", tag, attempt);
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
                        "[STT {}] connect attempt {}/{} failed: {}",
                        tag, attempt, max_attempts, e
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }

        let ws_stream = match ws_stream {
            Some(s) => s,
            None => {
                eprintln!("[STT {}] giving up after {} attempts", tag, max_attempts);
                return;
            }
        };

        let (mut stt_sink, mut stt_stream) = ws_stream.split();

        // Mode-specific config.
        let config = mode.build_config(&SONIOX_API_KEY);
        let config_json = match serde_json::to_string(&config) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[STT {}] config serialize error: {}", tag, e);
                return;
            }
        };
        if let Err(e) = stt_sink
            .send(tungstenite::Message::Text(config_json.into()))
            .await
        {
            eprintln!("[STT {}] config send failed: {}", tag, e);
            reconnect_count += 1;
            if reconnect_count > STT_RECONNECT_MAX {
                break;
            }
            continue;
        }

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

        let rooms_ref = rooms.clone();
        let rid = room_id.clone();
        let mode_recv = mode.clone();
        let tag_recv = tag.clone();
        let mut uc = utterance_counter;

        let disconnected_unexpectedly = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let disc_flag = disconnected_unexpectedly.clone();

        let recv_task = tokio::spawn(async move {
            let mut final_text = String::new();

            while let Some(msg_result) = stt_stream.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("[STT {}] WebSocket read error: {}", tag_recv, e);
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                };

                let text = match msg {
                    tungstenite::Message::Text(t) => t.to_string(),
                    tungstenite::Message::Close(_) => {
                        eprintln!("[STT {}] Soniox closed the connection", tag_recv);
                        disc_flag.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                    _ => continue,
                };

                let resp: SonioxResponse = match serde_json::from_str(&text) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("[STT {}] parse error: {} — raw: {}", tag_recv, e, text);
                        continue;
                    }
                };

                if let Some(code) = resp.error_code {
                    eprintln!(
                        "[STT {}] Soniox error {}: {}",
                        tag_recv,
                        code,
                        resp.error_message.unwrap_or_default()
                    );
                    disc_flag.store(true, std::sync::atomic::Ordering::Release);
                    break;
                }

                if !rooms_ref.contains_key(&rid) {
                    break; // room gone = normal shutdown
                }

                let mut interim_tail = String::new();
                let mut endpoint_hit = false;
                for tok in resp.tokens.iter() {
                    if !mode_recv.accepts(tok) {
                        continue;
                    }
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

                // Interim: only the source session pushes this to the host UI.
                // Interim translations would spam the host — we commit on <end>.
                if let SonioxMode::Source { .. } = mode_recv {
                    let interim = format!("{}{}", final_text, interim_tail);
                    if !interim.is_empty() {
                        if let Some(room) = rooms_ref.get(&rid) {
                            room.send_to_host(to_ws(&ServerMsg::Interim {
                                transcript: interim,
                            }));
                        }
                    }
                }

                if endpoint_hit && !final_text.trim().is_empty() {
                    uc += 1;
                    let uid = uc;
                    let committed = std::mem::take(&mut final_text);
                    match &mode_recv {
                        SonioxMode::Source { .. } => {
                            println!("[FINAL src #{}] {}", uid, committed);
                            if let Some(room) = rooms_ref.get(&rid) {
                                room.send_to_host(to_ws(&ServerMsg::Final {
                                    transcript: committed,
                                    utterance_id: uid,
                                }));
                            }
                        }
                        SonioxMode::Translate { target_lang, .. } => {
                            println!("[FINAL {} #{}] {}", target_lang, uid, committed);
                            let voice_clone_id =
                                rooms_ref.get(&rid).and_then(|r| r.voice_clone_id.clone());
                            let rtmp_mgr = rooms_ref
                                .get(&rid)
                                .and_then(|r| r.rtmp_manager.clone());
                            if let Some(room) = rooms_ref.get(&rid) {
                                room.send_to_host(to_ws(&ServerMsg::Translation {
                                    text: committed.clone(),
                                    utterance_id: uid,
                                    target_lang: target_lang.to_string(),
                                    translate_ms: 0,
                                }));
                            }
                            // Fire burn-in caption immediately — the mixer's
                            // writer task handles min-dwell spacing.
                            if let Some(mgr) = rtmp_mgr.clone() {
                                let lang_s = target_lang.to_string();
                                let text = committed.clone();
                                tokio::spawn(async move {
                                    mgr.lock().await.push_caption(&lang_s, text);
                                });
                            }
                            let rooms = rooms_ref.clone();
                            let rid = rid.clone();
                            let target = target_lang.clone();
                            tokio::spawn(async move {
                                do_tts_and_broadcast(
                                    &committed,
                                    uid,
                                    &target,
                                    &rooms,
                                    &rid,
                                    voice_clone_id.as_deref(),
                                )
                                .await;
                            });
                        }
                    }
                } else if endpoint_hit {
                    // Endpoint on silence / keep-alive — just reset.
                    final_text.clear();
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
            break;
        }
        if !rooms.contains_key(&room_id) {
            break;
        }

        reconnect_count += 1;
        if reconnect_count > STT_RECONNECT_MAX {
            eprintln!("[STT {}] exceeded max reconnects", tag);
            break;
        }
        eprintln!(
            "[STT {}] will reconnect {}/{}",
            tag, reconnect_count, STT_RECONNECT_MAX
        );
        tokio::time::sleep(STT_RECONNECT_DELAY).await;
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
