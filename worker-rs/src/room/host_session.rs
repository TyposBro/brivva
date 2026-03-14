use std::cell::RefCell;
use std::rc::Rc;
use futures::StreamExt;
use worker::*;
use crate::types::{ServerMsg, NovaMessage};
use super::broadcaster::Broadcaster;
use super::nova_stt::NovaStt;
use super::translator::Translator;
use super::kokoro_tts::KokoroTts;

pub struct HostSession;

impl HostSession {
    pub async fn accept(
        host_ws: &WebSocket,
        room_id: &str,
        source_lang: &str,
        env: &Env,
        broadcaster: &Rc<RefCell<Broadcaster>>,
    ) -> Option<String> {
        if broadcaster.borrow().has_host() {
            return Some("Room already has a host".into());
        }

        broadcaster.borrow_mut().set_host(host_ws.clone());

        // Connect to Deepgram Nova-3 STT
        let nova = match NovaStt::connect(env, room_id, source_lang).await {
            Ok(n) => n,
            Err(e) => {
                broadcaster.borrow_mut().clear_host();
                return Some(format!("STT connection failed: {}", e));
            }
        };

        // Confirm room creation
        let msg = serde_json::to_string(&ServerMsg::RoomCreated {
            room_id: room_id.to_string(),
        })
        .unwrap_or_default();
        let _ = host_ws.send_with_str(msg);

        // Spawn host WS event loop: audio → Nova, text → commands
        Self::spawn_host_listener(
            host_ws.clone(),
            nova.ws.clone(),
            broadcaster.clone(),
        );

        // Spawn Nova WS event loop: STT results → broadcast + translate
        Self::spawn_nova_listener(
            nova.ws,
            broadcaster.clone(),
            env.clone(),
            room_id.to_string(),
            source_lang.to_string(),
        );

        None
    }

    // --- event loops ---

    fn spawn_host_listener(
        host_ws: WebSocket,
        nova_ws: WebSocket,
        broadcaster: Rc<RefCell<Broadcaster>>,
    ) {
        wasm_bindgen_futures::spawn_local(async move {
            let mut events = match host_ws.events() {
                Ok(e) => e,
                Err(_) => return,
            };

            while let Some(event) = events.next().await {
                match event {
                    Ok(WebsocketEvent::Message(msg)) => {
                        if let Some(bytes) = msg.bytes() {
                            let _ = nova_ws.send_with_bytes(&bytes);
                        } else if let Some(text) = msg.text() {
                            if is_host_end(&text) {
                                stop(&nova_ws, &broadcaster);
                                break;
                            }
                        }
                    }
                    Ok(WebsocketEvent::Close(_)) => {
                        stop(&nova_ws, &broadcaster);
                        break;
                    }
                    Err(_) => break,
                }
            }
        });
    }

    fn spawn_nova_listener(
        nova_ws: WebSocket,
        broadcaster: Rc<RefCell<Broadcaster>>,
        env: Env,
        room_id: String,
        source_lang: String,
    ) {
        let pending: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

        wasm_bindgen_futures::spawn_local(async move {
            let mut events = match nova_ws.events() {
                Ok(e) => e,
                Err(_) => return,
            };

            while let Some(event) = events.next().await {
                match event {
                    Ok(WebsocketEvent::Message(msg)) => {
                        if let Some(text) = msg.text() {
                            handle_nova_message(
                                &text,
                                &broadcaster,
                                &pending,
                                &env,
                                &room_id,
                                &source_lang,
                            )
                            .await;
                        }
                    }
                    Ok(WebsocketEvent::Close(_)) => break,
                    Err(e) => {
                        console_log!("[room:{}] Nova error: {:?}", room_id, e);
                        break;
                    }
                }
            }
        });
    }
}

// --- message handling ---

fn is_host_end(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("type")?.as_str().map(|s| s == "host:end"))
        .unwrap_or(false)
}

async fn handle_nova_message(
    data: &str,
    broadcaster: &Rc<RefCell<Broadcaster>>,
    pending: &Rc<RefCell<String>>,
    env: &Env,
    room_id: &str,
    source_lang: &str,
) {
    let msg: NovaMessage = match serde_json::from_str(data) {
        Ok(m) => m,
        Err(_) => return,
    };

    if msg.msg_type != "Results" {
        return;
    }

    let transcript = msg
        .channel
        .and_then(|c| c.alternatives.into_iter().next())
        .map(|a| a.transcript.trim().to_string())
        .unwrap_or_default();

    let is_final = msg.is_final.unwrap_or(false);
    let speech_final = msg.speech_final.unwrap_or(false);

    if !is_final && !speech_final {
        if transcript.is_empty() {
            return;
        }
        *pending.borrow_mut() = transcript.clone();

        let msg = serde_json::to_string(&ServerMsg::Interim { transcript }).unwrap_or_default();
        broadcaster.borrow().send_to_everyone(&msg);
    } else {
        let final_transcript = if transcript.is_empty() {
            pending.borrow().clone()
        } else {
            transcript
        };
        *pending.borrow_mut() = String::new();

        if final_transcript.is_empty() {
            return;
        }

        let utterance_id = Date::now().as_millis();

        let msg = serde_json::to_string(&ServerMsg::Final {
            transcript: final_transcript.clone(),
            utterance_id,
        })
        .unwrap_or_default();
        broadcaster.borrow().send_to_everyone(&msg);

        run_pipeline(broadcaster, env, &final_transcript, utterance_id, room_id, source_lang).await;
    }
}

// --- translation pipeline ---

async fn run_pipeline(
    broadcaster: &Rc<RefCell<Broadcaster>>,
    env: &Env,
    transcript: &str,
    utterance_id: u64,
    room_id: &str,
    source_lang: &str,
) {
    let active_langs = broadcaster.borrow().active_langs();
    if active_langs.is_empty() {
        return;
    }

    let translations =
        Translator::translate_all(env, transcript, source_lang, &active_langs, room_id).await;

    for t in translations {
        if t.text.is_empty() {
            continue;
        }

        // Broadcast translated text to language group
        let translation_msg = serde_json::to_string(&ServerMsg::Translation {
            text: Some(t.text.clone()),
            utterance_id,
            translate_ms: t.translate_ms,
        })
        .unwrap_or_default();
        broadcaster.borrow().send_to_lang_str(t.lang, &translation_msg);

        // Send timing to host (no text)
        let host_msg = serde_json::to_string(&ServerMsg::Translation {
            text: None,
            utterance_id,
            translate_ms: t.translate_ms,
        })
        .unwrap_or_default();
        broadcaster.borrow().send_to_host(&host_msg);

        // TTS: synthesize + stream audio
        let tts_t0 = Date::now().as_millis();

        if let Some(mut resp) = KokoroTts::synthesize(env, t.lang, &t.text, room_id).await {
            let start_msg =
                serde_json::to_string(&ServerMsg::TtsStart { utterance_id }).unwrap_or_default();
            broadcaster.borrow().send_to_lang_str(t.lang, &start_msg);

            // Stream audio bytes
            if let Ok(body) = resp.bytes().await {
                broadcaster.borrow().send_to_lang_bytes(t.lang, &body);
            }

            let tts_ms = Date::now().as_millis() - tts_t0;
            let end_msg =
                serde_json::to_string(&ServerMsg::TtsEnd { utterance_id, tts_ms }).unwrap_or_default();
            broadcaster.borrow().send_to_lang_str(t.lang, &end_msg);
            broadcaster.borrow().send_to_host(&end_msg);
        }
    }
}

fn stop(nova_ws: &WebSocket, broadcaster: &Rc<RefCell<Broadcaster>>) {
    let msg = serde_json::to_string(&ServerMsg::RoomClosed).unwrap_or_default();
    broadcaster.borrow().send_to_all_guests(&msg);
    let _ = nova_ws.close::<&str>(None, None);
    broadcaster.borrow_mut().clear_host();
}
