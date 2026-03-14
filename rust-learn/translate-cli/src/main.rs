use std::fmt;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // let text: String = match parse_language(&args[1]) {
    //     Ok(lang) => println!("Parsed: {}", lang),
    //     Err(e) => {
    //         println!("Error: {}", e)
    //     }
    // };

    let req = TranslationRequest {
        text: args[1].clone(),
        source_lang: Language::English,
        target_lang: Language::Japanese,
    };

    println!("{}", req.describe());
}

#[derive(Debug)]
enum Language {
    English,
    Japanese,
    Chinese,
    Korean,
}

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

fn parse_language(s: &str) -> Result<Language, String> {
    match s {
        "en" => Ok(Language::English),
        "zh" => Ok(Language::Chinese),
        "ja" => Ok(Language::Japanese),
        "ko" => Ok(Language::Korean),
        _ => Err("Unknown language".to_string()),
    }
}
