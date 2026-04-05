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

#[cfg(test)]
mod tests {
    use super::*;

    // ── detector_config ──────────────────────────────────

    #[test]
    fn should_return_english_markers_for_en() {
        let config = detector_config("en");

        assert!(!config.markers.is_empty());
        assert!(config.markers.contains(&", and "));
    }

    #[test]
    fn should_set_en_min_duration_to_1500ms() {
        let config = detector_config("en");

        assert_eq!(config.min_duration, Duration::from_millis(1500));
    }

    #[test]
    fn should_set_en_max_duration_to_3s() {
        let config = detector_config("en");

        assert_eq!(config.max_duration, Duration::from_secs(3));
    }

    #[test]
    fn should_set_en_min_chars_after_to_3() {
        let config = detector_config("en");

        assert_eq!(config.min_chars_after, 3);
    }

    #[test]
    fn should_return_japanese_markers_for_ja() {
        let config = detector_config("ja");

        assert!(!config.markers.is_empty());
        assert!(config.markers.contains(&"けれども、"));
    }

    #[test]
    fn should_set_ja_min_chars_after_to_2() {
        let config = detector_config("ja");

        assert_eq!(config.min_chars_after, 2);
    }

    #[test]
    fn should_set_ja_min_duration_to_1s() {
        let config = detector_config("ja");

        assert_eq!(config.min_duration, Duration::from_secs(1));
    }

    #[test]
    fn should_set_ja_max_duration_to_2500ms() {
        let config = detector_config("ja");

        assert_eq!(config.max_duration, Duration::from_millis(2500));
    }

    #[test]
    fn should_return_korean_markers_for_ko() {
        let config = detector_config("ko");

        assert!(!config.markers.is_empty());
        assert!(config.markers.contains(&"지만 "));
    }

    #[test]
    fn should_set_ko_min_duration_to_1s() {
        let config = detector_config("ko");

        assert_eq!(config.min_duration, Duration::from_secs(1));
    }

    #[test]
    fn should_return_chinese_markers_for_zh() {
        let config = detector_config("zh");

        assert!(!config.markers.is_empty());
    }

    #[test]
    fn should_set_zh_max_duration_to_3s() {
        let config = detector_config("zh");

        assert_eq!(config.max_duration, Duration::from_secs(3));
    }

    #[test]
    fn should_return_empty_markers_for_unknown_language() {
        let config = detector_config("fr");

        assert!(config.markers.is_empty());
    }

    #[test]
    fn should_use_default_timing_for_unknown_language() {
        let config = detector_config("unknown");

        assert_eq!(config.min_duration, Duration::from_millis(1500));
        assert_eq!(config.max_duration, Duration::from_secs(3));
        assert_eq!(config.min_chars_after, 3);
    }

    // ── progressive_config ───────────────────────────────

    #[test]
    fn should_return_en_progressive_with_lower_thresholds() {
        let config = progressive_config("en");

        assert!(!config.markers.is_empty());
        assert_eq!(config.min_duration, Duration::from_millis(1000));
        assert_eq!(config.max_duration, Duration::from_millis(2000));
        assert_eq!(config.min_chars_after, 3);
    }

    #[test]
    fn should_return_ko_progressive_with_800ms_min() {
        let config = progressive_config("ko");

        assert!(!config.markers.is_empty());
        assert_eq!(config.min_duration, Duration::from_millis(800));
        assert_eq!(config.max_duration, Duration::from_millis(2000));
        assert_eq!(config.min_chars_after, 2);
    }

    #[test]
    fn should_return_ja_progressive_with_800ms_min() {
        let config = progressive_config("ja");

        assert_eq!(config.min_duration, Duration::from_millis(800));
        assert_eq!(config.min_chars_after, 2);
    }

    #[test]
    fn should_return_zh_progressive_config() {
        let config = progressive_config("zh");

        assert!(!config.markers.is_empty());
        assert_eq!(config.min_duration, Duration::from_millis(800));
    }

    #[test]
    fn should_return_empty_markers_for_unknown_progressive() {
        let config = progressive_config("unknown");

        assert!(config.markers.is_empty());
        assert_eq!(config.min_duration, Duration::from_millis(1000));
        assert_eq!(config.max_duration, Duration::from_millis(2000));
    }

    #[test]
    fn should_have_progressive_min_less_than_detector_min_for_en() {
        let detector = detector_config("en");
        let progressive = progressive_config("en");

        assert!(progressive.min_duration < detector.min_duration);
    }

    #[test]
    fn should_have_progressive_max_less_than_detector_max_for_en() {
        let detector = detector_config("en");
        let progressive = progressive_config("en");

        assert!(progressive.max_duration < detector.max_duration);
    }
}
