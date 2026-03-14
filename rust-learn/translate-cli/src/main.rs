use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize, ser};
use std::fmt;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let res = translate(&args[1], Language::English, Language::Japanese, &args[2]).await;

    println!("{:?}", res);
}

#[derive(Deserialize, Debug)]
struct TranslationResponse {
    success: bool,
    result: TranslatedText,
}

#[derive(Deserialize, Debug)]
struct TranslatedText {
    translated_text: String,
}

#[derive(Debug, Serialize)]
enum Language {
    #[serde(rename = "en")]
    English,

    #[serde(rename = "ja")]
    Japanese,

    #[serde(rename = "zh")]
    Chinese,

    #[serde(rename = "ko")]
    Korean,
}

#[derive(Serialize, Debug)]
struct TranslationRequest {
    text: String,
    source_lang: Language,
    target_lang: Language,
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            Language::English => "en",
            Language::Chinese => "zh",
            Language::Japanese => "ja",
            Language::Korean => "ko",
        };
        write!(f, "{}", s)
    }
}

impl TranslationRequest {
    fn describe(&self) -> String {
        format!(
            "text: {}, source: {}, target: {}",
            self.text, self.source_lang, self.target_lang
        )
    }
}

#[derive(Debug)]
enum TranslationError {
    Http(reqwest::Error),
    InvalidLanguage(String),
}

impl From<reqwest::Error> for TranslationError {
    fn from(value: reqwest::Error) -> Self {
        TranslationError::Http(value)
    }
}

async fn translate(
    text: &str,
    source: Language,
    target: Language,
    token: &str,
) -> Result<String, TranslationError> {
    let req = TranslationRequest {
        text: text.to_string(),
        source_lang: source,
        target_lang: target,
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {}", token).parse().unwrap(),
    );
    let client = reqwest::Client::new();
    let res = client.post("https://api.cloudflare.com/client/v4/accounts/80a55132ae169d5b282ccf505bc66bf7/ai/run/@cf/meta/m2m100-1.2b")
    .headers(headers)
    .json(&req)
    .send()
    .await?
    .json::<TranslationResponse>()
    .await?;

    Ok(res.result.translated_text)
}

fn parse_language(s: &str) -> Result<Language, String> {
    match s {
        "en" => Ok(Language::English),
        "zh" => Ok(Language::Chinese),
        "ja" => Ok(Language::Japanese),
        "ko" => Ok(Language::Korean),
        _ => Err("Unknown language".to_string()),
    }
}
