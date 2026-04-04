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

/// Translate text with optional context prefix (for chunk-aware translation).
/// Context is prepended as "context ||| text" and stripped from the result.
/// Returns (translated_text, elapsed_ms).
pub async fn translate(
    text: &str,
    context: Option<&str>,
    source: &Lang,
    target: &Lang,
) -> Result<(String, u64), String> {
    // Build query with context prefix if provided
    let query_text = match context {
        Some(c) if !c.is_empty() => format!("{} ||| {}", c, text),
        _ => text.to_string(),
    };

    let url = format!(
        "https://translation.googleapis.com/language/translate/v2?key={}",
        &*TRANSLATE_API_KEY
    );

    let start = Instant::now();
    let resp = crate::HTTP_CLIENT
        .post(&url)
        .json(&serde_json::json!({
            "q": query_text,
            "source": source.to_string(),
            "target": target.to_string(),
            "format": "text",
        }))
        .send()
        .await;

    let full_translation = match resp {
        Ok(r) if r.status().is_success() => {
            match r.json::<GoogleTranslateResponse>().await {
                Ok(r) => r.data.translations.into_iter().next()
                    .map(|t| t.translated_text)
                    .unwrap_or_default(),
                Err(e) => return Err(format!("parse error for {}: {}", target, e)),
            }
        }
        Ok(r) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            return Err(format!("error for {}: {} - {}", target, status, body));
        }
        Err(e) => return Err(format!("request error for {}: {}", target, e)),
    };

    // Strip context prefix from translation
    let translated_text = if context.is_some() {
        if let Some(pos) = full_translation.find("|||") {
            full_translation[pos + 3..].trim().to_string()
        } else {
            full_translation
        }
    } else {
        full_translation
    };

    let elapsed_ms = start.elapsed().as_millis() as u64;
    Ok((translated_text, elapsed_ms))
}
