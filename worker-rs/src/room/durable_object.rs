use std::cell::RefCell;
use std::rc::Rc;
use worker::*;
use super::broadcaster::Broadcaster;
use super::host_session::HostSession;
use super::guest_manager::GuestManager;
use crate::types::Lang;

#[durable_object]
pub struct RoomDO {
    env: Env,
    broadcaster: Rc<RefCell<Broadcaster>>,
    room_id: RefCell<String>,
}

impl DurableObject for RoomDO {
    fn new(state: State, env: Env) -> Self {
        let _ = state;
        Self {
            env,
            broadcaster: Rc::new(RefCell::new(Broadcaster::new())),
            room_id: RefCell::new(String::new()),
        }
    }

    async fn fetch(&self, req: Request) -> Result<Response> {
        let url = req.url()?;
        let params: std::collections::HashMap<String, String> =
            url.query_pairs().map(|(k, v)| (k.to_string(), v.to_string())).collect();

        let role = params.get("role").map(|s| s.as_str());
        let lang = params.get("lang").and_then(|s| Lang::from_str(s));
        let source_lang = params.get("sourceLang").cloned().unwrap_or_else(|| "en".into());

        if let Some(id) = params.get("roomId") {
            *self.room_id.borrow_mut() = id.clone();
        }

        let pair = WebSocketPair::new()?;
        let server = pair.server;
        let client = pair.client;

        server.accept()?;

        let room_id = self.room_id.borrow().clone();

        let error = match role {
            Some("host") => {
                HostSession::accept(
                    &server, &room_id, &source_lang, &self.env, &self.broadcaster,
                ).await
            }
            Some("guest") => {
                GuestManager::accept(&server, lang, &room_id, &self.broadcaster)
            }
            _ => {
                let _ = server.close::<&str>(None, Some("Unknown role"));
                None
            }
        };

        if let Some(msg) = error {
            let err_json = serde_json::json!({ "type": "error", "message": msg });
            let _ = server.send_with_str(err_json.to_string());
            let _ = server.close::<&str>(None, None);
        }

        Response::from_websocket(client)
    }
}
