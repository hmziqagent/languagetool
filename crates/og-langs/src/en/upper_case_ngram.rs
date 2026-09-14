//! Rust port of LanguageTool's `UpperCaseNgramRule`
//! (id `EN_UPPER_CASE_NGRAM`).
//!
//! Finds some words written uppercase that should be spelled lowercase.
//! Java compares the trigram probability of the uppercase vs. lowercase
//! form (ratio > 50). The backing `og_languagemodel::LanguageModel` is
//! case-insensitive (it lowercases at train and query time), so a case
//! probability ratio cannot be computed; this port instead requires the
//! all-lowercase trigram to be attested at least `MIN_OCCURRENCES` times
//! in the corpus. No-op when the model is untrained. Further divergences
//! (skipped Aho-Corasick exception phrases, spell-check pre-filter,
//! anti-pattern subset) are recorded in docs/plans/compatibility-matrix.md.

use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchContext,
    RuleMatchRule, SuggestedReplacement, rule::Rule,
};
use og_languagemodel::LanguageModel;
use std::sync::Arc;

/// How many corpus occurrences of the lowercase trigram qualify
/// (stand-in for Java's probability-ratio THRESHOLD = 50).
pub const MIN_OCCURRENCES: u64 = 5;

/// Words that are correct uppercase even mid-sentence (Java `exceptions`).
const EXCEPTIONS: &[&str] = &[
    "Bin",
    "Spot",
    "Go",
    "French",
    "Roman",
    "Hawking",
    "Square",
    "Japan",
    "Premier",
    "Allied",
    "Counsel",
    "Act",
    "Acts",
    "Associate",
    "Associates",
    "Bill",
    "Bills",
    "Board",
    "Center",
    "Channel",
    "Fantasy",
    "Judge",
    "Mass",
    "Staff",
    "Trust",
];

/// Words typically spelled lowercase even in titles.
const TYPICAL_LOWERCASE: &[&str] = &[
    "and", "or", "the", "of", "on", "with", "to", "it", "in", "for", "as", "at", "his", "her",
    "its", "into", "&", "/",
];

fn lowercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn starts_with_uppercase(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_uppercase())
}

fn is_all_uppercase(s: &str) -> bool {
    !s.chars().any(|c| c.is_lowercase())
}

fn has_pos_tag_starting_with(token: &AnalyzedTokenReadings, prefix: &str) -> bool {
    token.all_pos_tags().iter().any(|t| t.starts_with(prefix))
}

pub struct UpperCaseNgramRule {
    lm: Arc<LanguageModel>,
}

impl UpperCaseNgramRule {
    pub fn new(lm: Arc<LanguageModel>) -> Self {
        Self { lm }
    }

    fn is_short_word(token: &str) -> bool {
        token.trim().is_empty() || TYPICAL_LOWERCASE.contains(&token)
    }

    /// Rough guess whether the word at `i` is part of a title (first long
    /// word to the left or right is uppercase).
    fn maybe_title(tokens: &[&AnalyzedTokenReadings], i: usize) -> bool {
        (1..i)
            .rev()
            .find(|&j| !Self::is_short_word(tokens[j].token().token()))
            .is_some_and(|j| starts_with_uppercase(tokens[j].token().token()))
            || tokens
                .iter()
                .skip(i + 1)
                .find(|t| !Self::is_short_word(t.token().token()))
                .is_some_and(|t| starts_with_uppercase(t.token().token()))
    }
}

impl Rule for UpperCaseNgramRule {
    fn id(&self) -> &str {
        "EN_UPPER_CASE_NGRAM"
    }

    fn description(&self) -> &str {
        "Checks wrong uppercase spelling of words that are not proper nouns"
    }

    fn is_default_on(&self) -> bool {
        // Java does not setDefaultOff; the rule only exists when an ngram
        // model is configured, which we emulate by no-oping when untrained.
        true
    }

    fn category(&self) -> Category {
        Category::new("CASING", "Capitalization")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Other("typographical".to_string())
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        if self.lm.token_count() == 0 {
            return Vec::new();
        }
        let tokens = sentence.non_whitespace_tokens();
        let mut matches = Vec::new();
        if tokens.len() < 3 {
            return matches;
        }
        for i in 1..tokens.len() {
            let token_str = tokens[i].token().token();
            if i + 1 >= tokens.len() {
                // Java computes the trigram with the next token only; the
                // last token has no next token, so it can never be scored.
                continue;
            }
            let prev_upper = starts_with_uppercase(tokens[i - 1].token().token());
            let next_upper = starts_with_uppercase(tokens[i + 1].token().token());
            let prev_one_of = matches!(
                tokens[i - 1].token().token(),
                ":" | "née" | "of" | "\"" | "'"
            );
            let next_of_then_upper = i + 2 < tokens.len()
                && tokens[i + 1].token().token() == "of"
                && starts_with_uppercase(tokens[i + 2].token().token());
            if starts_with_uppercase(token_str)
                && !is_all_uppercase(token_str)
                // Java: hasPosTagStartingWith("VB") && !hasPosTagStartingWith("NNP")
                && has_pos_tag_starting_with(tokens[i], "VB")
                && !has_pos_tag_starting_with(tokens[i], "NNP")
                // probably a name like "Sex Pistols", but not at position 2
                && (!prev_upper || i == 2)
                && !next_upper
                && !prev_one_of
                && !next_of_then_upper
                && token_str != "I"
                && !EXCEPTIONS.contains(&token_str)
                && !Self::maybe_title(&tokens, i)
            {
                let lc = lowercase_first_char(token_str);
                let trigram = [
                    tokens[i - 1].token().token().to_lowercase(),
                    lc.clone(),
                    tokens[i + 1].token().token().to_lowercase(),
                ];
                let occurrences = self.lm.count(&[&trigram[0], &trigram[1], &trigram[2]]);
                if occurrences >= MIN_OCCURRENCES {
                    let message = "Only proper nouns start with an uppercase character (there are exceptions for headlines).";
                    let rm_rule = RuleMatchRule::new(self.id(), self.description())
                        .with_category(self.category());
                    let ctx_start = tokens[i].token().start().saturating_sub(40);
                    let ctx_end = (tokens[i].token().end() + 40).min(sentence.text().len());
                    matches.push(
                        RuleMatch::new(
                            message,
                            tokens[i].token().start(),
                            tokens[i].token().end() - tokens[i].token().start(),
                            rm_rule,
                            RuleMatchContext::new(
                                sentence.text()[ctx_start..ctx_end].to_string(),
                                ctx_start,
                                ctx_end - ctx_start,
                            ),
                        )
                        .with_replacements(vec![SuggestedReplacement::new(lc)])
                        .with_sentence(sentence.text().to_string()),
                    );
                }
            }
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};

    fn sentence_with_pos(words: &[(&str, &str)]) -> AnalyzedSentence {
        let text = words.iter().map(|(w, _)| *w).collect::<Vec<_>>().join(" ");
        let mut s = AnalyzedSentence::new(text.clone(), 0, text.len());
        let mut pos = 0usize;
        let tokens = words
            .iter()
            .map(|(w, tag)| {
                let mut t = AnalyzedToken::new(*w, pos, pos + w.len());
                pos += w.len() + 1;
                t.set_pos_tags(vec![tag.to_string()]);
                AnalyzedTokenReadings::new(t)
            })
            .collect();
        s.set_tokens(tokens);
        s
    }

    #[test]
    fn untrained_model_is_noop() {
        let rule = UpperCaseNgramRule::new(Arc::new(LanguageModel::new(3)));
        let s = sentence_with_pos(&[
            ("He", "PRP"),
            ("drinks", "VBZ"),
            ("Water", "NN"),
            ("daily", "RB"),
        ]);
        assert!(rule.match_sentence(&s).is_empty());
    }

    #[test]
    fn trained_model_flags_mid_sentence_verb() {
        let mut lm = LanguageModel::new(3);
        for _ in 0..MIN_OCCURRENCES {
            lm.add_text("he drinks water daily");
        }
        let rule = UpperCaseNgramRule::new(Arc::new(lm));
        let s = sentence_with_pos(&[
            ("He", "PRP"),
            ("drinks", "VBZ"),
            ("Water", "VBZ"),
            ("daily", "RB"),
        ]);
        let matches = rule.match_sentence(&s);
        assert!(!matches.is_empty(), "expected uppercase flag");
        assert_eq!(matches[0].replacements()[0].value(), "water");
    }

    #[test]
    fn sentence_start_not_flagged() {
        let mut lm = LanguageModel::new(3);
        for _ in 0..MIN_OCCURRENCES {
            lm.add_text("he drinks water daily");
        }
        let rule = UpperCaseNgramRule::new(Arc::new(lm));
        // "Drinks" is at sentence start (i == 0 is skipped)
        let s = sentence_with_pos(&[("Drinks", "VBZ"), ("water", "NN"), ("daily", "RB")]);
        assert!(rule.match_sentence(&s).is_empty());
    }

    #[test]
    fn noun_tagged_token_not_flagged() {
        let mut lm = LanguageModel::new(3);
        for _ in 0..MIN_OCCURRENCES {
            lm.add_text("he drinks water daily");
        }
        let rule = UpperCaseNgramRule::new(Arc::new(lm));
        let s = sentence_with_pos(&[
            ("He", "PRP"),
            ("drinks", "VBZ"),
            ("Water", "NN"),
            ("daily", "RB"),
        ]);
        assert!(rule.match_sentence(&s).is_empty());
    }
}
