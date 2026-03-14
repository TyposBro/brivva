use worker::*;

const NOVA_PARAMS: &[(&str, &str)] = &[
    ("encoding", "linear16"),
    ("sample_rate", "16000"),
    ("channels", "1"),
    ("interim_results", "true"),
    ("punctuate", "true"),
    ("smart_format", "true"),
    ("endpointing", "300"),
    ("utterance_end_ms", "1000"),
];

pub struct NovaStt {
    pub ws: WebSocket,
}

impl NovaStt {
    pub async fn connect(env: &Env, _room_id: &str, source_lang: &str) -> Result<Self> {
        let (url, auth_headers) = Self::build_connection(env, source_lang)?;

        let mut req = Request::new(&url, Method::Get)?;
        let headers = req.headers_mut()?;
        headers.set("Upgrade", "websocket")?;
        for (key, value) in &auth_headers {
            headers.set(key, value)?;
        }

        let mut resp = Fetch::Request(req).send().await?;

        if resp.status_code() != 101 {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::RustError(format!("Nova-3 WS {}: {}", resp.status_code(), body)));
        }

        let ws = resp.websocket().ok_or_else(|| Error::RustError("No WebSocket in response".into()))?;
        ws.accept()?;

        Ok(Self { ws })
    }

    pub fn send_audio(&self, data: &[u8]) {
        let _ = self.ws.send_with_bytes(data);
    }

    pub fn close(&self) {
        let _ = self.ws.close::<&str>(None, None);
    }

    // --- internals ---

    fn build_connection(env: &Env, source_lang: &str) -> Result<(String, Vec<(String, String)>)> {
        if let Ok(api_key) = env.secret("DEEPGRAM_API_KEY").map(|s| s.to_string()) {
            return Self::build_direct_deepgram(source_lang, &api_key);
        }
        Self::build_cf_gateway(env)
    }

    fn build_direct_deepgram(source_lang: &str, api_key: &str) -> Result<(String, Vec<(String, String)>)> {
        let mut url = Url::parse("https://api.deepgram.com/v1/listen")
            .map_err(|e| Error::RustError(e.to_string()))?;

        url.query_pairs_mut().append_pair("model", "nova-3");
        url.query_pairs_mut().append_pair("language", source_lang);
        Self::apply_params(&mut url);

        let headers = vec![("Authorization".into(), format!("Token {}", api_key))];
        Ok((url.to_string(), headers))
    }

    fn build_cf_gateway(env: &Env) -> Result<(String, Vec<(String, String)>)> {
        let account_id = env.var("CF_ACCOUNT_ID")?.to_string();
        let gateway_id = env.var("CF_AI_GATEWAY_ID")?.to_string();
        let api_token = env.secret("CF_API_TOKEN")?.to_string();

        let base = format!(
            "https://gateway.ai.cloudflare.com/v1/{}/{}/workers-ai",
            account_id, gateway_id
        );
        let mut url = Url::parse(&base).map_err(|e| Error::RustError(e.to_string()))?;

        url.query_pairs_mut().append_pair("model", "@cf/deepgram/nova-3");
        url.query_pairs_mut().append_pair("language", "en");
        Self::apply_params(&mut url);

        let headers = vec![("cf-aig-authorization".into(), format!("Bearer {}", api_token))];
        Ok((url.to_string(), headers))
    }

    fn apply_params(url: &mut Url) {
        let mut pairs = url.query_pairs_mut();
        for (key, value) in NOVA_PARAMS {
            pairs.append_pair(key, value);
        }
    }
}
