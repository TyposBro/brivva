use std::collections::{HashMap, HashSet};
use worker::WebSocket;
use crate::types::{Lang, GuestCounts, ServerMsg};

struct Guest {
    ws: WebSocket,
    lang: Lang,
}

pub struct Broadcaster {
    host_ws: Option<WebSocket>,
    guests: HashMap<String, Guest>,
    lang_groups: HashMap<Lang, HashSet<String>>,
}

impl Broadcaster {
    pub fn new() -> Self {
        let mut lang_groups = HashMap::new();
        for lang in Lang::ALL {
            lang_groups.insert(lang, HashSet::new());
        }

        Self {
            host_ws: None,
            guests: HashMap::new(),
            lang_groups,
        }
    }

    // --- host ---

    pub fn has_host(&self) -> bool {
        self.host_ws.is_some()
    }

    pub fn set_host(&mut self, ws: WebSocket) {
        self.host_ws = Some(ws);
    }

    pub fn clear_host(&mut self) {
        self.host_ws = None;
    }

    // --- guests ---

    pub fn add_guest(&mut self, id: String, ws: WebSocket, lang: Lang) {
        self.lang_groups.entry(lang).or_default().insert(id.clone());
        self.guests.insert(id, Guest { ws, lang });
    }

    pub fn remove_guest(&mut self, id: &str) {
        if let Some(guest) = self.guests.remove(id) {
            if let Some(group) = self.lang_groups.get_mut(&guest.lang) {
                group.remove(id);
            }
        }
    }

    pub fn active_langs(&self) -> Vec<Lang> {
        Lang::ALL
            .iter()
            .copied()
            .filter(|lang| {
                self.lang_groups
                    .get(lang)
                    .map_or(false, |group| !group.is_empty())
            })
            .collect()
    }

    // --- send ---

    pub fn send_to_host(&self, data: &str) {
        if let Some(ws) = &self.host_ws {
            let _ = ws.send_with_str(data);
        }
    }

    pub fn send_to_lang_str(&self, lang: Lang, data: &str) {
        if let Some(ids) = self.lang_groups.get(&lang) {
            for id in ids {
                if let Some(guest) = self.guests.get(id) {
                    let _ = guest.ws.send_with_str(data);
                }
            }
        }
    }

    pub fn send_to_lang_bytes(&self, lang: Lang, data: &[u8]) {
        if let Some(ids) = self.lang_groups.get(&lang) {
            for id in ids {
                if let Some(guest) = self.guests.get(id) {
                    let _ = guest.ws.send_with_bytes(data);
                }
            }
        }
    }

    pub fn send_to_all_guests(&self, data: &str) {
        for guest in self.guests.values() {
            let _ = guest.ws.send_with_str(data);
        }
    }

    pub fn send_to_everyone(&self, data: &str) {
        self.send_to_host(data);
        self.send_to_all_guests(data);
    }

    pub fn push_guest_count(&self) {
        let counts = GuestCounts {
            en: self.lang_groups.get(&Lang::En).map_or(0, |g| g.len()),
            ja: self.lang_groups.get(&Lang::Ja).map_or(0, |g| g.len()),
            zh: self.lang_groups.get(&Lang::Zh).map_or(0, |g| g.len()),
        };

        let msg = serde_json::to_string(&ServerMsg::GuestCount { counts }).unwrap_or_default();
        self.send_to_host(&msg);
    }
}
