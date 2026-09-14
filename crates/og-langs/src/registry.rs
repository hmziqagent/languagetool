use og_core::Language;

use crate::engine::LanguageEngine;

pub struct LanguageRegistry {
    languages: Vec<Language>,
}

impl LanguageRegistry {
    pub fn new() -> Self {
        Self {
            languages: Language::all_languages(),
        }
    }

    pub fn all_languages(&self) -> &[Language] {
        &self.languages
    }

    pub fn get_language(&self, code: &str) -> Option<&Language> {
        self.languages.iter().find(|l| l.code() == code)
    }

    pub fn supported_codes(&self) -> Vec<&str> {
        self.languages.iter().map(|l| l.code()).collect()
    }

    /// Language codes the engine factory can construct (English and German
    /// per the migration order in docs/PLAN.md).
    pub fn engine_supported_codes(&self) -> Vec<&'static str> {
        vec!["en", "en-US", "en-GB", "de", "de-DE", "de-AT", "de-CH"]
    }

    /// Whether [`Self::create_engine`] can build an engine for `code`.
    pub fn is_engine_supported(&self, code: &str) -> bool {
        matches!(code.split('-').next().unwrap_or(code), "en" | "de")
    }

    /// Create a fully constructed [`LanguageEngine`] for a language code.
    /// Variant codes share the base-language engine. Returns `None` for
    /// languages without an implementation.
    pub fn create_engine(&self, code: &str) -> Option<LanguageEngine> {
        LanguageEngine::for_code(code)
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_engine_english_and_german() {
        let registry = LanguageRegistry::new();
        let en = registry
            .create_engine("en-US")
            .expect("English engine should exist");
        assert_eq!(en.language().code(), "en-US");
        let de = registry
            .create_engine("de-DE")
            .expect("German engine should exist");
        assert_eq!(de.language().code(), "de-DE");
        assert!(de.rule_count() > 0);
    }

    #[test]
    fn create_engine_unsupported_returns_none() {
        let registry = LanguageRegistry::new();
        assert!(registry.create_engine("fr").is_none());
        assert!(!registry.is_engine_supported("pl"));
        assert!(registry.is_engine_supported("de-CH"));
    }
}
