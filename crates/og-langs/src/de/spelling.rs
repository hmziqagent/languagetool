use og_core::rule::Rule;
use og_core::{AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule};
use og_spell::HunspellDictionary;
use std::path::PathBuf;

/// German spell checker rule backed by the real igerman98 Hunspell
/// dictionaries shipped upstream (`resource/de/hunspell/de_DE.dic` + `.aff`),
/// including affix expansion and compound-part handling from
/// `og_spell::hunspell` (COMPOUNDBEGIN/MIDDLE/END continuation flags,
/// FORBIDDENWORD, NEEDAFFIX, KEEPCASE — see the compatibility matrix).
///
/// Divergences from Java's `GermanSpellerRule` (Morfologik-based):
/// no suggestion generation (HunspellDictionary has no `suggest` API).
pub struct GermanSpellingRule {
    dict: Option<HunspellDictionary>,
    id: String,
}

impl GermanSpellingRule {
    /// Open the de_DE Hunspell dictionary from the LT resource root.
    /// Returns a rule that matches nothing when the dictionaries are absent.
    pub fn from_resources() -> Self {
        Self::with_variant("de_DE")
    }

    /// Open a specific Hunspell variant (`de_DE`, `de_AT`, `de_CH`).
    pub fn with_variant(variant: &str) -> Self {
        let dict = find_lt_resource_dir().and_then(|root| {
            let dir = root.join("de/src/main/resources/org/languagetool/resource/de/hunspell");
            let dic = dir.join(format!("{variant}.dic"));
            let aff = dir.join(format!("{variant}.aff"));
            match HunspellDictionary::open(&dic, &aff) {
                Ok(d) => Some(d),
                Err(e) => {
                    eprintln!("Warning: cannot open German Hunspell {variant}: {e}");
                    None
                }
            }
        });
        Self {
            dict,
            id: "DE_SPELL".to_string(),
        }
    }

    pub fn is_known_word(&self, word: &str) -> bool {
        match &self.dict {
            Some(d) => d.is_known(word) || d.is_known(&word.to_lowercase()),
            None => true, // no dictionary loaded — never flag
        }
    }

    pub fn is_active(&self) -> bool {
        self.dict.is_some()
    }
}

fn find_lt_resource_dir() -> Option<PathBuf> {
    og_spell::resource::lt_resource_root()
}

impl Rule for GermanSpellingRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Mögliche Rechtschreibfehler"
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn category(&self) -> Category {
        Category::new("TYPOS", "Mögliche Tippfehler")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Misspelling
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let Some(dict) = &self.dict else {
            return Vec::new();
        };
        let mut matches = Vec::new();

        for token_readings in sentence.tokens() {
            if token_readings.is_whitespace() {
                continue;
            }
            let text = token_readings.token().token();
            if !text.chars().any(|c| c.is_alphabetic()) {
                continue;
            }
            // Skip mixed alphanumeric tokens (URLs, abbreviations like "z.B.").
            if text.contains('.') || text.contains('/') || text.contains('@') {
                continue;
            }
            // German-specific: trailing punctuation glued by the tokenizer.
            let word = text
                .trim_end_matches(|c: char| !c.is_alphanumeric())
                .trim_start_matches(|c: char| !c.is_alphanumeric());
            if word.is_empty() {
                continue;
            }
            if dict.is_known(word) || dict.is_known(&word.to_lowercase()) {
                continue;
            }

            let start = token_readings.token().start();
            let end = token_readings.token().end();
            let context_start = start.saturating_sub(40);
            let context_end = std::cmp::min(end + 40, sentence.text().len());
            let context_text = sentence.text()[context_start..context_end].to_string();

            let rule = RuleMatchRule::new(&self.id, self.description())
                .with_category(self.category())
                .with_issue_type(IssueType::Misspelling.as_str());

            matches.push(
                RuleMatch::new(
                    format!("Mögliche Rechtschreibfehler: '{text}'"),
                    start,
                    end - start,
                    rule,
                    RuleMatchContext::new(context_text, context_start, context_end - context_start),
                )
                .with_sentence(sentence.text().to_string()),
            );
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::{AnalyzedToken, AnalyzedTokenReadings};

    fn make_sentence(text: &str) -> AnalyzedSentence {
        let mut sentence = AnalyzedSentence::new(text, 0, text.len());
        let mut tokens = Vec::new();
        let mut pos = 0;
        for part in text.split(' ') {
            if part.is_empty() {
                pos += 1;
                continue;
            }
            let start = pos;
            let end = start + part.len();
            pos = end + 1;
            tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                part, start, end,
            )));
        }
        sentence.set_tokens(tokens);
        sentence
    }

    #[test]
    fn without_dictionary_rule_is_silent() {
        // No LT resource root -> from_resources degrades to inactive.
        let rule = GermanSpellingRule {
            dict: None,
            id: "DE_SPELL".to_string(),
        };
        assert!(!rule.is_active());
        let matches = rule.match_sentence(&make_sentence("Hausx"));
        assert!(matches.is_empty());
    }

    #[test]
    fn data_backed_flags_misspelling_accepts_compound() {
        if og_spell::resource::lt_resource_root().is_none() {
            eprintln!("Skipping: LT resource dir not found");
            return;
        }
        let rule = GermanSpellingRule::from_resources();
        assert!(rule.is_active());
        // Plain word, novel compound (via affix continuation flags), misspelling.
        assert!(rule.is_known_word("Haus"));
        assert!(rule.is_known_word("Abiturientinnenecke"));
        assert!(!rule.is_known_word("Hausx"));

        let matches = rule.match_sentence(&make_sentence("Das Hausx ist groß"));
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].rule().id(), "DE_SPELL");
        assert!(matches[0].message().contains("Hausx"));
    }
}
