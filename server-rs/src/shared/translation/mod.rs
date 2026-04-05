//! Google Cloud Translation API v2 integration.

use std::time::Instant;
use crate::core::types::Lang;

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
        api_key: &str,
        client: &reqwest::Client,
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
        api_key: &str,
        client: &reqwest::Client,
    ) -> Result<(String, u64), String> {
        let has_context = context.is_some_and(|c| !c.is_empty());
        let query_text = build_query_text(text, context);
        let start = Instant::now();
        let full = send_translate_request(client, api_key, &query_text, source, target).await?;
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
    api_key: &str,
    client: &reqwest::Client,
) -> Result<(String, u64), String> {
    GoogleTranslator.translate(text, context, source, target, api_key, client).await
}

// ── Helpers ──────────────────────────────────────────────

fn build_query_text(text: &str, context: Option<&str>) -> String {
    match context {
        Some(c) if !c.is_empty() => format!("{} ||| {}", c, text),
        _ => text.to_string(),
    }
}

async fn send_translate_request(
    client: &reqwest::Client,
    api_key: &str,
    query_text: &str,
    source: &Lang,
    target: &Lang,
) -> Result<String, String> {
    let url = translate_url(api_key);

    let resp = client
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

fn translate_url(api_key: &str) -> String {
    format!(
        "https://translation.googleapis.com/language/translate/v2?key={}",
        api_key
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── build_query_text ────────────────────────────���───

    #[test]
    fn should_prepend_context_with_separator() {
        let result = build_query_text("hello", Some("context"));

        assert_eq!(result, "context ||| hello");
    }

    #[test]
    fn should_return_text_as_is_without_context() {
        let result = build_query_text("hello", None);

        assert_eq!(result, "hello");
    }

    #[test]
    fn should_return_text_as_is_with_empty_context() {
        let result = build_query_text("hello", Some(""));

        assert_eq!(result, "hello");
    }

    // ── strip_context_prefix ────────────────────────────

    #[test]
    fn should_strip_context_prefix() {
        let result = strip_context_prefix("ctx ||| result", true);

        assert_eq!(result, "result");
    }

    #[test]
    fn should_return_full_text_when_no_separator_found() {
        let result = strip_context_prefix("no separator here", true);

        assert_eq!(result, "no separator here");
    }

    #[test]
    fn should_return_full_text_when_no_context_was_used() {
        let result = strip_context_prefix("anything ||| extra", false);

        assert_eq!(result, "anything ||| extra");
    }

    // ── build_query_text edge cases ──────────────────────

    #[test]
    fn should_preserve_special_characters_in_text() {
        let result = build_query_text("hello & goodbye <world>", None);

        assert_eq!(result, "hello & goodbye <world>");
    }

    #[test]
    fn should_preserve_separator_in_plain_text() {
        let result = build_query_text("has ||| inside", None);

        assert_eq!(result, "has ||| inside");
    }

    #[test]
    fn should_handle_unicode_context() {
        let result = build_query_text("hello", Some("こんにちは"));

        assert_eq!(result, "こんにちは ||| hello");
    }

    // ── strip_context_prefix edge cases ──────────────────

    #[test]
    fn should_strip_prefix_with_multiple_separators() {
        let result = strip_context_prefix("a ||| b ||| c", true);

        assert_eq!(result, "b ||| c");
    }

    #[test]
    fn should_handle_separator_at_start_of_string() {
        let result = strip_context_prefix("||| only text", true);

        assert_eq!(result, "only text");
    }

    #[test]
    fn should_handle_separator_at_end_of_string() {
        let result = strip_context_prefix("text |||", true);

        assert_eq!(result, "");
    }

    // ── translate_url ────────────────────────────────────

    #[test]
    fn should_build_url_with_api_key() {
        let url = translate_url("test-key-123");

        assert_eq!(
            url,
            "https://translation.googleapis.com/language/translate/v2?key=test-key-123"
        );
    }
}
