use worker::*;
use crate::types::Lang;

pub struct TranslationResult {
    pub lang: Lang,
    pub text: String,
    pub translate_ms: u64,
}

pub struct Translator;

impl Translator {
    pub async fn translate_all(
        env: &Env,
        transcript: &str,
        source_lang: &str,
        langs: &[Lang],
        room_id: &str,
    ) -> Vec<TranslationResult> {
        let mut results = Vec::with_capacity(langs.len());

        // Sequential for now — WASM Send constraints make join_all tricky
        for &lang in langs {
            let result = Self::translate(env, transcript, source_lang, lang, room_id).await;
            results.push(result);
        }

        results
    }

    async fn translate(
        env: &Env,
        transcript: &str,
        source_lang: &str,
        lang: Lang,
        room_id: &str,
    ) -> TranslationResult {
        let t0 = Date::now().as_millis();

        let input = serde_json::json!({
            "text": transcript,
            "source_lang": source_lang,
            "target_lang": lang.as_str(),
        });

        let result: std::result::Result<serde_json::Value, String> = async {
            let ai = env.ai("AI").map_err(|e| e.to_string())?;
            let resp: serde_json::Value = ai.run("@cf/meta/m2m100-1.2b", input)
                .await
                .map_err(|e| e.to_string())?;
            Ok(resp)
        }.await;

        let elapsed = Date::now().as_millis() - t0;

        match result {
            Ok(output) => {
                let text = output["translated_text"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                TranslationResult { lang, text, translate_ms: elapsed }
            }
            Err(e) => {
                console_log!("[room:{}] translate {} error: {}", room_id, lang.as_str(), e);
                TranslationResult { lang, text: String::new(), translate_ms: 0 }
            }
        }
    }
}
