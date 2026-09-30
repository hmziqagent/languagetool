use super::helpers::{has_pos_tag_starting_with, is_pos_tag_unknown};
use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchContext,
    RuleMatchRule, rule::TextLevelRule,
};
use std::collections::HashMap;

const MIN_WORD_COUNT: usize = 100;

#[derive(Clone, Copy, PartialEq)]
pub enum WordClass {
    Verb,
    Noun,
    Adjective,
}

pub struct StyleTooOftenUsedWordRule {
    word_class: WordClass,
    min_percent: i32,
}

impl StyleTooOftenUsedWordRule {
    pub fn verb() -> Self {
        Self {
            word_class: WordClass::Verb,
            min_percent: 5,
        }
    }

    pub fn noun() -> Self {
        Self {
            word_class: WordClass::Noun,
            min_percent: 5,
        }
    }

    pub fn adjective() -> Self {
        Self {
            word_class: WordClass::Adjective,
            min_percent: 5,
        }
    }

    fn id_for(&self) -> &'static str {
        match self.word_class {
            WordClass::Verb => "TOO_OFTEN_USED_VERB_DE",
            WordClass::Noun => "TOO_OFTEN_USED_NOUN_DE",
            WordClass::Adjective => "TOO_OFTEN_USED_ADJECTIVE_DE",
        }
    }

    fn description_for(&self) -> &'static str {
        match self.word_class {
            WordClass::Verb => "Statistische Stilanalyse: Zu häufig genutztes Verb",
            WordClass::Noun => "Statistische Stilanalyse: Zu häufig genutztes Substantiv",
            WordClass::Adjective => "Statistische Stilanalyse: Zu häufig genutztes Adjektiv",
        }
    }

    fn limit_message(&self, limit: i32) -> String {
        match self.word_class {
            WordClass::Verb => format!(
                "Das Verb wird häufiger verwendet als {limit}% aller Verben. Möglicherweise ist es besser es durch ein Synonym zu ersetzen."
            ),
            WordClass::Noun => format!(
                "Das Substantiv wird häufiger verwendet als {limit}% aller Substantive. Möglicherweise ist es besser es durch ein Synonym zu ersetzen."
            ),
            WordClass::Adjective => format!(
                "Das Adjektiv wird häufiger verwendet als {limit}% aller Adjektive. Möglicherweise ist es besser es durch ein Synonym zu ersetzen."
            ),
        }
    }

    fn pos_prefix(&self) -> &'static str {
        match self.word_class {
            WordClass::Verb => "VER:",
            WordClass::Noun => "SUB:",
            WordClass::Adjective => "ADJ:",
        }
    }

    fn is_to_counted_word(&self, token: &AnalyzedTokenReadings) -> bool {
        has_pos_tag_starting_with(token, self.pos_prefix())
    }

    fn is_exception(&self, tokens: &[&AnalyzedTokenReadings], n: usize) -> bool {
        let token = tokens[n];
        match self.word_class {
            WordClass::Verb => {
                has_pos_tag_starting_with(token, "VER:MOD")
                    || has_pos_tag_starting_with(token, "VER:AUX")
                    || has_pos_tag_starting_with(token, "ART")
                    || has_pos_tag_starting_with(token, "ADJ")
            }
            WordClass::Noun => {
                if has_pos_tag_starting_with(token, "PRO:")
                    || token.token().token() == "Ich"
                    || token.token().token() == "Aber"
                    || token.token().token() == "Ja"
                {
                    return true;
                }
                n < tokens.len() - 1
                    && matches!(token.token().token(), "Frau" | "Herr")
                    && (has_pos_tag_starting_with(tokens[n + 1], "EIG:")
                        || is_pos_tag_unknown(tokens[n + 1]))
            }
            WordClass::Adjective => {
                has_pos_tag_starting_with(token, "PRO:")
                    || has_pos_tag_starting_with(token, "ADV:")
                    || has_pos_tag_starting_with(token, "ZUS")
            }
        }
    }

    fn lemma_for_prefix<'t>(&self, token: &'t AnalyzedTokenReadings) -> Option<&'t str> {
        let prefix = self.pos_prefix();
        token
            .readings()
            .iter()
            .find(|r| r.pos_tags().iter().any(|t| t.starts_with(prefix)))
            .and_then(|r| r.lemma())
    }
}

impl TextLevelRule for StyleTooOftenUsedWordRule {
    fn id(&self) -> &str {
        self.id_for()
    }

    fn description(&self) -> &str {
        self.description_for()
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        Category::new("CREATIVE_WRITING", "Stiltipps für kreatives Schreiben")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut word_map: HashMap<&str, usize> = HashMap::new();
        let mut sentences_tokens: Vec<Vec<&AnalyzedTokenReadings>> =
            Vec::with_capacity(sentences.len());
        for sentence in sentences {
            let tokens = sentence.non_whitespace_tokens();
            for n in 1..tokens.len() {
                let token = tokens[n];
                if token.token().token().chars().any(|c| c.is_alphabetic())
                    && self.is_to_counted_word(token)
                    && !self.is_exception(&tokens, n)
                    && let Some(lemma) = self.lemma_for_prefix(token)
                {
                    *word_map.entry(lemma).or_insert(0) += 1;
                }
            }
            sentences_tokens.push(tokens);
        }
        let num_words: usize = word_map.values().sum();
        if num_words < MIN_WORD_COUNT {
            return Vec::new();
        }
        let too_often: Vec<&str> = word_map
            .iter()
            .filter(|(_, count)| (*count * 100) / num_words >= self.min_percent as usize)
            .map(|(&lemma, _)| lemma)
            .collect();
        if too_often.is_empty() {
            return Vec::new();
        }
        let mut rule_matches = Vec::new();
        for (tokens, sentence) in sentences_tokens.iter().zip(sentences) {
            for n in 1..tokens.len() {
                let token = tokens[n];
                if token.token().token().chars().any(|c| c.is_alphabetic())
                    && self.is_to_counted_word(token)
                    && !self.is_exception(tokens, n)
                    && let Some(lemma) = self.lemma_for_prefix(token)
                    && too_often.contains(&lemma)
                {
                    let rm_rule = RuleMatchRule::new(self.id(), self.description())
                        .with_category(self.category())
                        .with_issue_type(self.issue_type().as_str());
                    let from = token.token().start();
                    let to = token.token().end();
                    rule_matches.push(
                        RuleMatch::new(
                            self.limit_message(self.min_percent),
                            from,
                            to - from,
                            rm_rule,
                            RuleMatchContext::new(sentence.text().to_string(), from, to - from),
                        )
                        .with_sentence(sentence.text().to_string()),
                    );
                }
            }
        }
        rule_matches
    }
}
