pub mod compound;
#[cfg(test)]
pub(crate) mod diagnose;
#[cfg(test)]
pub(crate) mod example_suite;
pub mod question_whitespace;
pub mod repeated_words;
pub mod simple_replace;
pub mod suppress_misspelled;
pub mod tagger;
#[cfg(test)]
pub(crate) mod test_support;

use og_rules::postponed_adjective::LangSynthesizer;
use og_tagger::GenericTagger;
use std::path::PathBuf;
use std::sync::Arc;

pub use compound::CompoundRule;
pub use question_whitespace::QuestionWhitespaceRule;
pub use repeated_words::FrenchRepeatedWordsRule;
pub use simple_replace::SimpleReplaceRule;

pub(crate) fn fr_resource_dir() -> Option<PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("fr/src/main/resources/org/languagetool/resource/fr"))
}

pub fn french_compound_rule(tagger: Option<Arc<GenericTagger>>) -> CompoundRule {
    CompoundRule::french(tagger)
}

pub fn french_simple_replace_rule() -> SimpleReplaceRule {
    SimpleReplaceRule::french()
}

pub fn french_question_whitespace_rule() -> QuestionWhitespaceRule {
    QuestionWhitespaceRule::new()
}

pub fn french_question_whitespace_strict_rule() -> QuestionWhitespaceRule {
    QuestionWhitespaceRule::strict()
}

pub fn french_repeated_words_rule(
    synthesizer: Option<Arc<LangSynthesizer>>,
) -> FrenchRepeatedWordsRule {
    FrenchRepeatedWordsRule::new(synthesizer)
}

pub(crate) fn context_window(text: &str, text_start: usize, from: usize, to: usize) -> String {
    let mut rel_lo = from.saturating_sub(40).max(text_start) - text_start;
    let mut rel_hi = (to + 40).min(text_start + text.len()) - text_start;
    while rel_lo > 0 && !text.is_char_boundary(rel_lo) {
        rel_lo -= 1;
    }
    while rel_hi < text.len() && !text.is_char_boundary(rel_hi) {
        rel_hi += 1;
    }
    text[rel_lo..rel_hi].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::{Rule as _, TextLevelRule as _};

    #[test]
    fn french_native_rules_use_java_ids() {
        assert_eq!(french_compound_rule(None).id(), "FR_COMPOUNDS");
        assert_eq!(
            french_simple_replace_rule().id(),
            "FR_SIMPLE_REPLACE_SIMPLE"
        );
        assert_eq!(french_question_whitespace_rule().id(), "FRENCH_WHITESPACE");
        assert_eq!(
            french_question_whitespace_strict_rule().id(),
            "FRENCH_WHITESPACE_STRICT"
        );
        assert_eq!(french_repeated_words_rule(None).id(), "FR_REPEATEDWORDS");
    }

    #[test]
    fn strict_whitespace_rule_is_picky_and_temp_off() {
        let strict = french_question_whitespace_strict_rule();
        assert!(strict.is_picky());
        assert!(!strict.is_default_on());
        assert!(!french_question_whitespace_rule().is_picky());
    }
}
