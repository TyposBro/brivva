//! Google Cloud Translation API v2 integration.

use std::sync::LazyLock;
use std::time::Instant;
use crate::types::Lang;

static TRANSLATE_API_KEY: LazyLock<String> = LazyLock::new(|| {
    std::env::var("TRANSLATE_API_KEY").unwrap_or_default()
});

#[derive(serde::Deserialize)]
struct GoogleTranslateResponse {
    data: GoogleTranslateData,
}
#[derive(serde::Deserialize)]
struct GoogleTranslateData {
    translations: Vec<GoogleTranslation>,
}
#[derive(serde::Deserialize)]
struct GoogleTranslation {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

// ── Trait (IoC) ──────────────────────────────────────────

/// Abstraction over translation providers.
/// Implementations: `GoogleTranslator` (current), easily swappable for DeepL, etc.
pub trait Translator: Send + Sync {
    fn translate(
        &self,
        text: &str,
        context: Option<&str>,
        source: &Lang,
        target: &Lang,
    ) -> impl std::future::Future<Output = Result<(String, u64), String>> + Send;
}

/// Google Cloud Translation API v2 implementation.
pub struct GoogleTranslator;

impl Translator for GoogleTranslator {
    async fn translate(
        &self,
        text: &str,
        context: Option<&str>,
        source: &Lang,
        target: &Lang,
    ) -> Result<(String, u64), String> {
        let has_context = context.is_some_and(|c| !c.is_empty());
        let query_text = build_query_text(text, context);
        let start = Instant::now();
        let full = send_translate_request(&query_text, source, target).await?;
        Ok((strip_context_prefix(&full, has_context), start.elapsed().as_millis() as u64))
    }
}

// ── Public facade ────────────────────────────────────────

/// Translate text. Delegates to `GoogleTranslator`.
pub async fn translate(
    text: &str,
    context: Option<&str>,
    source: &Lang,
    target: &Lang,
) -> Result<(String, u64), String> {
    GoogleTranslator.translate(text, context, source, target).await
}

// ── Helpers ──────────────────────────────────────────────

fn build_query_text(text: &str, context: Option<&str>) -> String {
    match context {
        Some(c) if !c.is_empty() => format!("{} ||| {}", c, text),
        _ => text.to_string(),
    }
}

async fn send_translate_request(
    query_text: &str,
    source: &Lang,
    target: &Lang,
) -> Result<String, String> {
    let url = translate_url();

    let resp = crate::HTTP_CLIENT
        .post(&url)
        .json(&serde_json::json!({
            "q": query_text,
            "source": source.to_string(),
            "target": target.to_string(),
            "format": "text",
        }))
        .send()
        .await
        .map_err(|e| format!("request error for {}: {}", target, e))?;

    parse_translate_response(resp, target).await
}

fn translate_url() -> String {
    format!(
        "https://translation.googleapis.com/language/translate/v2?key={}",
        &*TRANSLATE_API_KEY
    )
}

async fn parse_translate_response(
    resp: reqwest::Response,
    target: &Lang,
) -> Result<String, String> {
    if !resp.status().is_success() {
        return Err(format_http_error(resp, target).await);
    }
    extract_translation(resp, target).await
}

async fn extract_translation(resp: reqwest::Response, target: &Lang) -> Result<String, String> {
    resp.json::<GoogleTranslateResponse>()
        .await
        .map(|r| {
            r.data.translations.into_iter().next()
                .map(|t| t.translated_text)
                .unwrap_or_default()
        })
        .map_err(|e| format!("parse error for {}: {}", target, e))
}

async fn format_http_error(resp: reqwest::Response, target: &Lang) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    format!("error for {}: {} - {}", target, status, body)
}

fn strip_context_prefix(text: &str, had_context: bool) -> String {
    if !had_context {
        return text.to_string();
    }
    match text.find("|||") {
        Some(pos) => text[pos + 3..].trim().to_string(),
        None => text.to_string(),
    }
}
