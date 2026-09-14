//! Per-check context handed to rules alongside the text they analyze.
//!
//! Mirrors the per-request state that Java LT rules read from `JLanguageTool`
//! (language, mother tongue, picky/"level" mode) without forcing rules to
//! carry it themselves. Rules that need none of it can keep implementing only
//! [`crate::Rule::match_sentence`] — the context-aware default methods
//! delegate there.

use crate::checker::CheckRequest;
use crate::language::Language;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleContext {
    language: Language,
    #[serde(default)]
    mother_tongue: Option<Language>,
    picky: bool,
}

impl RuleContext {
    pub fn new(language: Language) -> Self {
        Self {
            language,
            mother_tongue: None,
            picky: false,
        }
    }

    pub fn with_mother_tongue(mut self, mother_tongue: Option<Language>) -> Self {
        self.mother_tongue = mother_tongue;
        self
    }

    pub fn with_picky(mut self, picky: bool) -> Self {
        self.picky = picky;
        self
    }

    /// Derive the context from a check request: same language, mother tongue,
    /// and picky semantics as `CheckRequest::is_rule_enabled` (explicit
    /// `level=picky` counts as picky).
    pub fn from_check_request(request: &CheckRequest) -> Self {
        let picky = request.picky || request.level.as_deref() == Some("picky");
        Self {
            language: request.language.clone(),
            mother_tongue: request.mother_tongue.clone(),
            picky,
        }
    }

    pub fn language(&self) -> &Language {
        &self.language
    }

    pub fn mother_tongue(&self) -> Option<&Language> {
        self.mother_tongue.as_ref()
    }

    pub fn is_picky(&self) -> bool {
        self.picky
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_defaults_to_not_picky() {
        let ctx = RuleContext::new(Language::English);
        assert!(!ctx.is_picky());
        assert!(ctx.mother_tongue().is_none());
    }

    #[test]
    fn context_builders_set_fields() {
        let ctx = RuleContext::new(Language::English)
            .with_mother_tongue(Some(Language::German))
            .with_picky(true);
        assert_eq!(ctx.language(), &Language::English);
        assert_eq!(ctx.mother_tongue(), Some(&Language::German));
        assert!(ctx.is_picky());
    }

    #[test]
    fn context_from_request_honors_picky_level() {
        let request = CheckRequest::new("hi", Language::English);
        let ctx = RuleContext::from_check_request(&request);
        assert!(!ctx.is_picky());

        let mut request = CheckRequest::new("hi", Language::English);
        request.level = Some("picky".to_string());
        let ctx = RuleContext::from_check_request(&request);
        assert!(ctx.is_picky());
    }

    #[test]
    fn context_from_request_carries_mother_tongue() {
        let mut request = CheckRequest::new("hi", Language::English);
        request.mother_tongue = Some(Language::German);
        let ctx = RuleContext::from_check_request(&request);
        assert_eq!(ctx.mother_tongue(), Some(&Language::German));
    }
}
