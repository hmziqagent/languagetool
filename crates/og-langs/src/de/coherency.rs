use og_core::rule::TextLevelRule;
use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement,
};
use std::collections::{HashMap, HashSet};

/// Rust port of LanguageTool's German `WordCoherencyRule`
/// (rule id `DE_WORD_COHERENCY`, data `rules/de/coherency.txt`):
/// flags mixing two admitted spellings of the same word within one text
/// (e.g. "3-D" and "3D", "Abgeordnetenbiographie"/"Abgeordnetenbiografie").
///
/// Mirrors the English `WordCoherencyRule` in `crate::en`; kept separate
/// because the English struct hard-codes its resource path and rule id.
pub struct GermanWordCoherencyRule {
    word_map: HashMap<String, HashSet<String>>,
}

impl GermanWordCoherencyRule {
    pub fn new() -> Self {
        let mut rule = Self {
            word_map: HashMap::new(),
        };
        rule.load_data(
            &og_spell::read_lt_resource(
                "de/src/main/resources/org/languagetool/rules/de/coherency.txt",
            )
            .unwrap_or_default(),
        );
        rule
    }

    fn load_data(&mut self, data: &str) {
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.splitn(2, ';').collect();
            if parts.len() != 2 {
                continue;
            }
            let a = parts[0].trim().to_lowercase();
            let b = parts[1].trim().to_lowercase();
            self.word_map
                .entry(a.clone())
                .or_default()
                .insert(b.clone());
            self.word_map.entry(b).or_default().insert(a);
        }
    }
}

impl Default for GermanWordCoherencyRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for GermanWordCoherencyRule {
    fn id(&self) -> &str {
        "DE_WORD_COHERENCY"
    }

    fn description(&self) -> &str {
        "Einheitliche Schreibweise von Wörtern mit zwei zulässigen Varianten."
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn category(&self) -> Category {
        Category::new("MISC", "Miscellaneous")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Inconsistency
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let mut should_not_appear: HashMap<String, String> = HashMap::new();
        let mut pos = 0usize;

        for sentence in sentences {
            for token in sentence.non_whitespace_tokens() {
                let text = token.token().token();
                let lookup_word = text.to_lowercase();

                if let Some(first_spelling) = should_not_appear.get(&lookup_word) {
                    let message = format!(
                        "Vermischen Sie nicht zwei Schreibweisen desselben Wortes \
                         ('{first_spelling}' und '{lookup_word}')."
                    );
                    let from_pos = pos + token.token().start();
                    let to_pos = pos + token.token().end();

                    let mut replacement = first_spelling.clone();
                    if text.chars().any(char::is_uppercase) {
                        // Mirror the casing of the matched text: uppercase
                        // the first alphabetic character of the replacement
                        // (e.g. "3D" -> "3-D", not "3-d").
                        if let Some(pos) = replacement.find(|c: char| c.is_alphabetic()) {
                            let head = replacement[..pos].to_string();
                            let up: String = replacement[pos..]
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string())
                                .unwrap_or_default();
                            let tail: String = replacement[pos..].chars().skip(1).collect();
                            replacement = format!("{head}{up}{tail}");
                        }
                    }
                    if text.eq_ignore_ascii_case(&replacement) {
                        continue;
                    }

                    let rm_rule = RuleMatchRule::new(self.id(), self.description())
                        .with_category(self.category());
                    let ctx_start = from_pos.saturating_sub(40);
                    let ctx_end = std::cmp::min(to_pos + 40, pos + sentence.text().len());
                    let context_text = sentence.text()
                        [ctx_start.saturating_sub(pos)..ctx_end.saturating_sub(pos)]
                        .to_string();

                    matches.push(
                        RuleMatch::new(
                            &message,
                            from_pos,
                            to_pos - from_pos,
                            rm_rule,
                            RuleMatchContext::new(context_text, ctx_start, ctx_end - ctx_start),
                        )
                        .with_replacements(vec![SuggestedReplacement::new(&replacement)])
                        .with_sentence(sentence.text().to_string()),
                    );
                } else if let Some(variants) = self.word_map.get(&lookup_word) {
                    for variant in variants {
                        should_not_appear
                            .entry(variant.clone())
                            .or_insert_with(|| lookup_word.clone());
                    }
                }
            }
            pos += sentence.text().len();
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
        let mut byte_pos = 0;
        for part in text.split(' ') {
            if part.is_empty() {
                byte_pos += 1;
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
    fn detects_mixed_variants() {
        let rule = GermanWordCoherencyRule::new();
        let s1 = make_sentence("Das war 3-D");
        let s2 = make_sentence("und dann 3D");
        let full = "Das war 3-D und dann 3D";
        let matches = rule.match_text(full, &[s1, s2]);
        assert!(!matches.is_empty(), "should flag mixing 3-D and 3D");
        assert_eq!(matches[0].rule().id(), "DE_WORD_COHERENCY");
    }

    #[test]
    fn consistent_spelling_passes() {
        let rule = GermanWordCoherencyRule::new();
        let s1 = make_sentence("Das war 3-D");
        let s2 = make_sentence("und bleibt 3-D");
        let full = "Das war 3-D und bleibt 3-D";
        assert!(rule.match_text(full, &[s1, s2]).is_empty());
    }
}
