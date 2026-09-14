use og_core::Language;
use serde::Deserialize;

/// Parameters accepted by `/v2/check`, mirroring LanguageTool's HTTP API.
///
/// Every field is optional so the struct can be deserialized from a bare
/// query string or form body without extractor-level rejections; required
/// parameters (`text`/`data`, `language`) are validated by the handler,
/// which returns LT-shaped JSON error objects.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CheckParams {
    pub text: Option<String>,
    pub data: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "motherTongue")]
    pub mother_tongue: Option<String>,
    #[serde(rename = "enabledRules")]
    pub enabled_rules: Option<String>,
    #[serde(rename = "disabledRules")]
    pub disabled_rules: Option<String>,
    #[serde(rename = "enabledCategories")]
    pub enabled_categories: Option<String>,
    #[serde(rename = "disabledCategories")]
    pub disabled_categories: Option<String>,
    pub level: Option<String>,
    #[serde(default)]
    pub picky: bool,
}

impl CheckParams {
    pub fn get_text(&self) -> Option<&str> {
        self.text.as_deref().or(self.data.as_deref())
    }

    pub fn get_language(&self) -> Option<Language> {
        self.language.as_deref().and_then(Language::from_code)
    }

    pub fn get_mother_tongue(&self) -> Option<Language> {
        self.mother_tongue.as_deref().and_then(Language::from_code)
    }

    pub fn get_enabled_rules(&self) -> Vec<String> {
        split_csv(self.enabled_rules.as_deref())
    }

    pub fn get_disabled_rules(&self) -> Vec<String> {
        split_csv(self.disabled_rules.as_deref())
    }

    pub fn get_enabled_categories(&self) -> Vec<String> {
        split_csv(self.enabled_categories.as_deref())
    }

    pub fn get_disabled_categories(&self) -> Vec<String> {
        split_csv(self.disabled_categories.as_deref())
    }

    /// Fill absent fields from `other`. Used to merge query-string and
    /// form-body parameters the way Java servlet `getParameter` does
    /// (query string values take precedence over body values).
    pub fn or(self, other: CheckParams) -> CheckParams {
        CheckParams {
            text: self.text.or(other.text),
            data: self.data.or(other.data),
            language: self.language.or(other.language),
            mother_tongue: self.mother_tongue.or(other.mother_tongue),
            enabled_rules: self.enabled_rules.or(other.enabled_rules),
            disabled_rules: self.disabled_rules.or(other.disabled_rules),
            enabled_categories: self.enabled_categories.or(other.enabled_categories),
            disabled_categories: self.disabled_categories.or(other.disabled_categories),
            level: self.level.or(other.level),
            picky: self.picky || other.picky,
        }
    }
}

/// Parameters for the `/v2/words` user-dictionary endpoints.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WordParams {
    pub word: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

impl WordParams {
    /// The word, trimmed; `None` when absent or blank.
    pub fn get_word(&self) -> Option<String> {
        self.word
            .as_deref()
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(str::to_string)
    }

    /// Fill absent fields from `other` (query string wins over form body).
    pub fn or(self, other: WordParams) -> WordParams {
        WordParams {
            word: self.word.or(other.word),
            offset: self.offset.or(other.offset),
            limit: self.limit.or(other.limit),
        }
    }
}

fn split_csv(value: Option<&str>) -> Vec<String> {
    value
        .map(|s| s.split(',').map(|i| i.trim().to_string()).collect())
        .unwrap_or_default()
}
