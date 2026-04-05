//! Language-specific clause boundary markers for chunk detection.

use std::time::Duration;
use super::config::{LangDetectorConfig, ProgressiveLangConfig};

// ── English ─────────────────────────────────────────────

pub const ENGLISH_MARKERS: &[&str] = &[
    ", and ", ", but ", ", or ", ", so ", ", yet ", ", nor ",
    ", because ", ", since ", ", although ", ", while ", ", whereas ", ", unless ",
    ", which ", ", where ", ", when ",
    ", however ", ", therefore ", ", meanwhile ",
    "; ",
    ". And ", ". But ", ". So ", ". However ", ". Also ", ". Then ", ". Now ",
];

// ── Japanese ────────────────────────────────────────────

pub const JAPANESE_MARKERS: &[&str] = &[
    "けれども、", "けど、", "ですが、", "ますが、",
    "ので、", "から、", "ため、", "のに、", "ながら、",
    "しまして、", "まして、", "して、", "って、", "んで、",
    "そして ", "でも ", "だから ", "しかし ", "それから ",
    "ところが ", "それで ", "また ", "つまり ", "ただ ",
    "一方 ", "実は ", "ちなみに ",
    "ですね、", "なんですけど、", "ということで、",
];

// ── Korean ──────────────────────────────────────────────

pub const KOREAN_MARKERS: &[&str] = &[
    "는데요 ", "은데요 ", "인데요 ",
    "거든요 ", "니까요 ", "고요 ",
    "지만 ", "때문에 ", "면서 ", "어서 ", "아서 ", "하고 ",
    "는데 ", "은데 ", "인데 ",
    " 그리고 ", " 그런데 ", " 그래서 ", " 하지만 ",
    " 그래도 ", " 그러면 ", " 그러니까 ", " 또한 ", " 그다음에 ",
];

// ── Chinese ─────────────────────────────────────────────

pub const CHINESE_MARKERS: &[&str] = &[
    "\u{FF0C}但是", "\u{FF0C}因为", "\u{FF0C}所以", "\u{FF0C}然后",
    "\u{FF0C}而且", "\u{FF0C}不过", "\u{FF0C}可是", "\u{FF0C}虽然",
    "\u{FF0C}如果", "\u{FF0C}因此", "\u{FF0C}于是", "\u{FF0C}而",
    "\u{FF0C}同时", "\u{FF0C}另外", "\u{FF0C}接着",
    ",但是", ",因为", ",所以", ",然后", ",而且", ",不过", ",可是",
    "\u{FF0C}",
];

// ── Lookup ──────────────────────────────────────────────

pub fn detector_config(lang: &str) -> LangDetectorConfig {
    match lang {
        "en" => LangDetectorConfig {
            markers: ENGLISH_MARKERS,
            min_chars_after: 3,
            min_duration: Duration::from_millis(1500),
            max_duration: Duration::from_secs(3),
        },
        "ja" => LangDetectorConfig {
            markers: JAPANESE_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_secs(1),
            max_duration: Duration::from_millis(2500),
        },
        "ko" => LangDetectorConfig {
            markers: KOREAN_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_secs(1),
            max_duration: Duration::from_millis(2500),
        },
        "zh" => LangDetectorConfig {
            markers: CHINESE_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_secs(1),
            max_duration: Duration::from_secs(3),
        },
        _ => LangDetectorConfig {
            markers: &[],
            min_chars_after: 3,
            min_duration: Duration::from_millis(1500),
            max_duration: Duration::from_secs(3),
        },
    }
}

pub fn progressive_config(lang: &str) -> ProgressiveLangConfig {
    match lang {
        "en" => ProgressiveLangConfig {
            markers: ENGLISH_MARKERS,
            min_chars_after: 3,
            min_duration: Duration::from_millis(1000),
            max_duration: Duration::from_millis(2000),
        },
        "ja" => ProgressiveLangConfig {
            markers: JAPANESE_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_millis(800),
            max_duration: Duration::from_millis(2000),
        },
        "ko" => ProgressiveLangConfig {
            markers: KOREAN_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_millis(800),
            max_duration: Duration::from_millis(2000),
        },
        "zh" => ProgressiveLangConfig {
            markers: CHINESE_MARKERS,
            min_chars_after: 2,
            min_duration: Duration::from_millis(800),
            max_duration: Duration::from_millis(2000),
        },
        _ => ProgressiveLangConfig {
            markers: &[],
            min_chars_after: 3,
            min_duration: Duration::from_millis(1000),
            max_duration: Duration::from_millis(2000),
        },
    }
}
