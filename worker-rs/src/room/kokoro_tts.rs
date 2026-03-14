use worker::*;
use crate::types::Lang;

pub struct KokoroTts;

impl KokoroTts {
    pub async fn synthesize(
        env: &Env,
        lang: Lang,
        text: &str,
        room_id: &str,
    ) -> Option<Response> {
        let kokoro_url = match env.var("KOKORO_URL") {
            Ok(url) => url.to_string(),
            Err(_) => {
                console_log!("[room:{}] KOKORO_URL not set", room_id);
                return None;
            }
        };

        let body = serde_json::json!({
            "model": "kokoro",
            "voice": lang.voice(),
            "input": text,
        });

        let url = format!("{}/v1/audio/speech", kokoro_url);

        let mut req = Request::new(&url, Method::Post).ok()?;
        let _ = req.headers_mut().map(|h| {
            let _ = h.set("Content-Type", "application/json");
        });

        // Set JSON body via JsValue
        let js_body = serde_wasm_bindgen::to_value(&body).ok()?;
        let req = Request::new_with_init(
            &url,
            RequestInit::new()
                .with_method(Method::Post)
                .with_body(Some(js_body)),
        ).ok()?;

        match Fetch::Request(req).send().await {
            Ok(resp) if resp.status_code() == 200 => Some(resp),
            Ok(resp) => {
                console_log!("[room:{}] TTS {} error: status {}", room_id, lang.as_str(), resp.status_code());
                None
            }
            Err(e) => {
                console_log!("[room:{}] TTS {} fetch error: {:?}", room_id, lang.as_str(), e);
                None
            }
        }
    }
}
