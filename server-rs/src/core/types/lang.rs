use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ja,
    Zh,
    Ko,
    Ru,
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Lang::En => write!(f, "en"),
            Lang::Ja => write!(f, "ja"),
            Lang::Zh => write!(f, "zh"),
            Lang::Ko => write!(f, "ko"),
            Lang::Ru => write!(f, "ru"),
        }
    }
}

impl Lang {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "en" => Some(Lang::En),
            "ja" => Some(Lang::Ja),
            "zh" => Some(Lang::Zh),
            "ko" => Some(Lang::Ko),
            "ru" => Some(Lang::Ru),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_parse_en() {
        assert_eq!(Lang::from_str("en"), Some(Lang::En));
    }

    #[test]
    fn should_parse_ja() {
        assert_eq!(Lang::from_str("ja"), Some(Lang::Ja));
    }

    #[test]
    fn should_parse_zh() {
        assert_eq!(Lang::from_str("zh"), Some(Lang::Zh));
    }

    #[test]
    fn should_parse_ko() {
        assert_eq!(Lang::from_str("ko"), Some(Lang::Ko));
    }

    #[test]
    fn should_parse_ru() {
        assert_eq!(Lang::from_str("ru"), Some(Lang::Ru));
    }

    #[test]
    fn should_return_none_for_unknown_code() {
        assert_eq!(Lang::from_str("fr"), None);
    }

    #[test]
    fn should_display_roundtrip() {
        for (code, lang) in [
            ("en", Lang::En), ("ja", Lang::Ja), ("zh", Lang::Zh),
            ("ko", Lang::Ko), ("ru", Lang::Ru),
        ] {
            assert_eq!(lang.to_string(), code);
            assert_eq!(Lang::from_str(&lang.to_string()), Some(lang));
        }
    }
}
