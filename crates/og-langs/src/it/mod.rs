use og_core::rule::Rule;
use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchContext,
    RuleMatchRule, SuggestedReplacement,
};

const BASE_IGNORES: &[&str] = &[
    "Phi", "Li", "Xiao", "Duran", "Wagga", "Abdullah", "Nwe", "Pago", "Cao",
];

const ITALIAN_IGNORES: &[&str] = &["cos\u{ec}", "passo", "piano", "via"];

pub struct ItalianWordRepeatRule {
    id: String,
    category: Category,
}

impl ItalianWordRepeatRule {
    pub fn new() -> Self {
        Self {
            id: "ITALIAN_WORD_REPEAT_RULE".to_string(),
            category: Category::new("MISC", "Miscellaneous"),
        }
    }

    fn word_repetition_of(
        &self,
        word: &str,
        tokens: &[&AnalyzedTokenReadings],
        position: usize,
    ) -> bool {
        position > 0
            && tokens[position - 1].token().token() == word
            && tokens[position].token().token() == word
    }

    fn ignore(&self, tokens: &[&AnalyzedTokenReadings], position: usize) -> bool {
        if BASE_IGNORES
            .iter()
            .chain(ITALIAN_IGNORES.iter())
            .any(|word| self.word_repetition_of(word, tokens, position))
        {
            return true;
        }
        false
    }
}

impl Default for ItalianWordRepeatRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for ItalianWordRepeatRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Word repetition"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Redundancy
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let tokens = sentence.non_whitespace_tokens();

        for i in 1..tokens.len() {
            let prev = tokens[i - 1];
            let curr = tokens[i];

            let prev_text = prev.token().token().to_lowercase();
            let curr_text = curr.token().token().to_lowercase();

            if prev_text == curr_text
                && curr_text.chars().any(|c| c.is_alphabetic())
                && !self.ignore(&tokens, i)
            {
                let start = curr.token().start();
                let end = curr.token().end();
                let context_start = start.saturating_sub(20);
                let context_end = std::cmp::min(end + 20, sentence.text().len());
                let context_text = sentence.text()[context_start..context_end].to_string();

                let rule = RuleMatchRule::new(&self.id, self.description())
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Redundancy.as_str());

                matches.push(
                    RuleMatch::new(
                        format!("Possible word repetition: '{}'", curr.token().token()),
                        start,
                        end - start,
                        rule,
                        RuleMatchContext::new(
                            context_text,
                            context_start,
                            context_end - context_start,
                        ),
                    )
                    .with_sentence(sentence.text().to_string())
                    .with_replacements(vec![SuggestedReplacement::new(curr.token().token())]),
                );
            }
        }

        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::AnalyzedToken;

    fn make_sentence(text: &str) -> AnalyzedSentence {
        let mut sentence = AnalyzedSentence::new(text, 0, text.len());
        let mut tokens = Vec::new();
        let mut byte_pos = 0;
        for part in text.split(|c: char| c.is_whitespace()) {
            if part.is_empty() {
                continue;
            }
            if let Some(offset) = text[byte_pos..].find(part) {
                let start = byte_pos + offset;
                let end = start + part.len();
                tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                    part, start, end,
                )));
                byte_pos = end;
            }
        }
        sentence.set_tokens(tokens);
        sentence
    }

    #[test]
    fn detects_real_repetition() {
        let rule = ItalianWordRepeatRule::new();
        let sentence = make_sentence("Questa \u{e8} una una prova");
        assert_eq!(rule.match_sentence(&sentence).len(), 1);
    }

    #[test]
    fn ignores_italian_idioms() {
        let rule = ItalianWordRepeatRule::new();
        for text in [
            "Mi \u{e8} sembrato cos\u{ec} cos\u{ec}.",
            "Andiamo piano piano verso la meta",
            "Il lessico si andava via via modificando.",
            "che passo passo vanno verso la rovina",
        ] {
            let sentence = make_sentence(text);
            assert!(
                rule.match_sentence(&sentence).is_empty(),
                "should ignore repetition in '{text}'"
            );
        }
    }
}
