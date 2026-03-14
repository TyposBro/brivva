use std::cell::RefCell;
use std::rc::Rc;
use futures::StreamExt;
use worker::*;
use crate::types::{Lang, ServerMsg};
use super::broadcaster::Broadcaster;

pub struct GuestManager;

impl GuestManager {
    pub fn accept(
        ws: &WebSocket,
        lang: Option<Lang>,
        room_id: &str,
        broadcaster: &Rc<RefCell<Broadcaster>>,
    ) -> Option<String> {
        if !broadcaster.borrow().has_host() {
            return Some("Room not found".into());
        }

        let lang = match lang {
            Some(l) => l,
            None => return Some("Invalid language".into()),
        };

        let guest_id = generate_id();

        {
            let mut b = broadcaster.borrow_mut();
            b.add_guest(guest_id.clone(), ws.clone(), lang);

            let msg = serde_json::to_string(&ServerMsg::RoomJoined {
                room_id: room_id.to_string(),
                lang,
            })
            .unwrap_or_default();
            let _ = ws.send_with_str(msg);

            b.push_guest_count();
        }

        // Spawn close listener
        let broadcaster = broadcaster.clone();
        let ws_clone = ws.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let mut events = match ws_clone.events() {
                Ok(e) => e,
                Err(_) => return,
            };

            while let Some(event) = events.next().await {
                match event {
                    Ok(WebsocketEvent::Close(_)) | Err(_) => {
                        let mut b = broadcaster.borrow_mut();
                        b.remove_guest(&guest_id);
                        b.push_guest_count();
                        break;
                    }
                    _ => {}
                }
            }
        });

        None
    }
}

fn generate_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).unwrap_or(());
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}
