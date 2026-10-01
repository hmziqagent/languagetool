use std::collections::HashMap;

use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement, rule::Rule,
};

pub struct RussianSpecificCaseRule {
    lc_to_proper_spelling: HashMap<String, String>,
    max_len: usize,
}

impl RussianSpecificCaseRule {
    pub fn new() -> Self {
        Self::from_data(include_str!(
            "../../../../languagetool/languagetool-language-modules/ru/src/main/resources/org/languagetool/resource/ru/specific_case.txt"
        ))
    }

    fn from_data(data: &str) -> Self {
        let mut lc_to_proper_spelling = HashMap::new();
        let mut max_len = 0usize;
        for line in data.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let phrase = line.trim();
            if phrase.is_empty() {
                continue;
            }
            max_len = max_len.max(phrase.split(' ').count());
            lc_to_proper_spelling.insert(phrase.to_lowercase(), phrase.to_string());
        }
        Self {
            lc_to_proper_spelling,
            max_len,
        }
    }

    fn all_words_uppercase(s: &str) -> bool {
        s.split(' ')
            .all(|w| w.chars().next().is_some_and(|c| c.is_uppercase()))
    }
}

impl Default for RussianSpecificCaseRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for RussianSpecificCaseRule {
    fn id(&self) -> &str {
        "RU_SPECIFIC_CASE"
    }

    fn description(&self) -> &str {
        "Написание специальных наименований в верхнем или нижнем регистре"
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn category(&self) -> Category {
        Category::new("CASING", "Capitalization")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Misspelling
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        let mut matches = Vec::new();
        for i in 0..tokens.len() {
            let mut words: Vec<&str> = Vec::new();
            let mut j = 0usize;
            while words.len() < self.max_len && i + j < tokens.len() {
                words.push(tokens[i + j].token().token());
                j += 1;
                let phrase = words.join(" ");
                let lc_phrase = phrase.to_lowercase();
                let Some(proper_spelling) = self.lc_to_proper_spelling.get(&lc_phrase) else {
                    continue;
                };
                let is_all_uppercase = !phrase.chars().any(|c| c.is_lowercase())
                    && phrase.chars().any(|c| c.is_uppercase());
                if is_all_uppercase || phrase == *proper_spelling {
                    continue;
                }
                if i > 0
                    && tokens[i - 1].is_sentence_start()
                    && !proper_spelling
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_uppercase())
                {
                    continue;
                }
                let message = if Self::all_words_uppercase(proper_spelling) {
                    "Для специальных наименований используйте начальную заглавную букву."
                } else {
                    "Для специальных наименований используйте предложенное написание заглавных и строчных букв."
                };
                let start = tokens[i].token().start();
                let end = tokens[i + j - 1].token().end();
                let ctx_start = start.saturating_sub(40);
                let ctx_end = std::cmp::min(end + 40, sentence.text().len());
                let context_text = sentence.text()[ctx_start..ctx_end].to_string();
                let rm_rule = RuleMatchRule::new(self.id(), self.description())
                    .with_category(self.category())
                    .with_issue_type(self.issue_type().as_str());
                matches.push(
                    RuleMatch::new(
                        message,
                        start,
                        end - start,
                        rm_rule,
                        RuleMatchContext::new(context_text, ctx_start, ctx_end - ctx_start),
                    )
                    .with_replacements(vec![SuggestedReplacement::new(proper_spelling)])
                    .with_sentence(sentence.text().to_string()),
                );
            }
        }
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::{AnalyzedToken, AnalyzedTokenReadings};

    fn sentence(words: &[&str]) -> AnalyzedSentence {
        let text = words.join(" ");
        let mut s = AnalyzedSentence::new(&text, 0, text.len());
        let mut tokens = vec![AnalyzedTokenReadings::new(
            AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
        )];
        let mut offset = 0;
        for word in words {
            let start = offset;
            let end = start + word.len();
            offset = end + 1;
            tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                *word, start, end,
            )));
        }
        s.set_tokens(tokens);
        s
    }

    #[test]
    fn loads_java_phrases_and_suggests_proper_spelling() {
        let rule = RussianSpecificCaseRule::from_data("Рытый Банк\nвитамин C\n");
        let matches = rule.match_sentence(&sentence(&["Река", "рытый", "банк"]));
        assert_eq!(matches.len(), 1, "lowercase phrase must be flagged");
        assert_eq!(matches[0].replacements()[0].value(), "Рытый Банк");
    }

    #[test]
    fn skips_sentence_start_lowercase_suggestion() {
        let rule = RussianSpecificCaseRule::from_data("витамин C\n");
        let matches = rule.match_sentence(&sentence(&["витамин", "c"]));
        assert!(
            matches.is_empty(),
            "lowercase-start suggestion at sentence start must be skipped"
        );
    }
}
