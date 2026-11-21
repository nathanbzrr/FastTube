use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    #[default]
    PtBr,
    En,
}

impl UiLanguage {
    pub const ALL: [Self; 2] = [Self::PtBr, Self::En];

    pub fn label(self) -> &'static str {
        match self {
            Self::PtBr => "Português (Brasil)",
            Self::En => "English",
        }
    }

    #[allow(dead_code)]
    pub fn code(self) -> &'static str {
        match self {
            Self::PtBr => "pt-BR",
            Self::En => "en",
        }
    }

    pub fn region_code(self) -> &'static str {
        match self {
            Self::PtBr => "BR",
            Self::En => "US",
        }
    }

    #[inline]
    pub fn pick<'a>(self, pt: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::PtBr => pt,
            Self::En => en,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_pick_and_round_trip() {
        assert_eq!(UiLanguage::PtBr.code(), "pt-BR");
        assert_eq!(UiLanguage::En.code(), "en");
        assert_eq!(UiLanguage::PtBr.region_code(), "BR");
        assert_eq!(UiLanguage::En.region_code(), "US");
        assert_eq!(UiLanguage::PtBr.pick("Início", "Home"), "Início");
        assert_eq!(UiLanguage::En.pick("Início", "Home"), "Home");
        let encoded = serde_json::to_string(&UiLanguage::En).unwrap();
        assert_eq!(encoded, "\"en\"");
        let decoded: UiLanguage = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, UiLanguage::En);
    }
}
