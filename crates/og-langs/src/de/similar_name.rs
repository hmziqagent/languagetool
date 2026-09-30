use super::helpers::{is_pos_tag_unknown, levenshtein_at_most_one, starts_with_uppercase};
use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement, rule::TextLevelRule,
};
use std::collections::HashSet;

const MIN_LENGTH: usize = 4;

const PRONOUN_EXCEPTIONS: [&str; 12] = [
    "Dein", "Deine", "Deinen", "Deiner", "Deines", "Deinem", "Ihr", "Ihre", "Ihren", "Ihrer",
    "Ihres", "Ihrem",
];

fn has_partial_pos_tag(atr: &og_core::AnalyzedTokenReadings, part: &str) -> bool {
    atr.readings()
        .iter()
        .any(|r| r.pos_tags().iter().any(|t| t.contains(part)))
}

pub struct SimilarNameRule;

impl SimilarNameRule {
    fn similar_name<'a>(
        &self,
        name_here: &str,
        names_so_far: &'a HashSet<String>,
    ) -> Option<&'a str> {
        let mut best: Option<&'a str> = None;
        for name in names_so_far {
            if name == name_here {
                continue;
            }
            let len_diff = name.len().abs_diff(name_here.len());
            let name_ends_with_s = name.ends_with('s') && !name_here.ends_with('s');
            let other_name_ends_with_s = !name.ends_with('s') && name_here.ends_with('s');
            let name_ends_with_n = name.ends_with('n') && !name_here.ends_with('n');
            let other_name_ends_with_n = !name.ends_with('n') && name_here.ends_with('n');
            if name_ends_with_s
                || other_name_ends_with_s
                || name_ends_with_n
                || other_name_ends_with_n
            {
                continue;
            }
            if len_diff <= 1 && levenshtein_at_most_one(name, name_here) {
                best = Some(name);
            }
        }
        best
    }
}

impl Default for SimilarNameRule {
    fn default() -> Self {
        Self
    }
}

impl TextLevelRule for SimilarNameRule {
    fn id(&self) -> &str {
        "DE_SIMILAR_NAMES"
    }

    fn description(&self) -> &str {
        "Mögliche Tippfehler in Namen finden"
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        Category::new("TYPOS", "Mögliche Tippfehler")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Misspelling
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut names_so_far: HashSet<String> = HashSet::new();
        let mut rule_matches = Vec::new();
        for sentence in sentences {
            let tokens = sentence.non_whitespace_tokens();
            for atr in tokens.iter() {
                let word = atr.token().token();
                let is_maybe_name = word.chars().count() >= MIN_LENGTH
                    && ((has_partial_pos_tag(atr, "EIG:") && !has_partial_pos_tag(atr, ":COU"))
                        || is_pos_tag_unknown(atr))
                    && !PRONOUN_EXCEPTIONS.contains(&word);
                if is_maybe_name && starts_with_uppercase(word) {
                    if let Some(similar) = self.similar_name(word, &names_so_far) {
                        let msg = format!(
                            "'{word}' ähnelt dem vorher benutzten '{similar}', handelt es sich evtl. um einen Tippfehler?"
                        );
                        let rm_rule = RuleMatchRule::new(self.id(), self.description())
                            .with_category(self.category())
                            .with_issue_type(self.issue_type().as_str());
                        let from = atr.token().start();
                        let to = atr.token().end();
                        rule_matches.push(
                            RuleMatch::new(
                                msg,
                                from,
                                to - from,
                                rm_rule,
                                RuleMatchContext::new(sentence.text().to_string(), from, to - from),
                            )
                            .with_replacements(vec![SuggestedReplacement::new(similar)])
                            .with_sentence(sentence.text().to_string()),
                        );
                    }
                    names_so_far.insert(word.to_string());
                }
            }
        }
        rule_matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_one_detection() {
        assert!(levenshtein_at_most_one("Miller", "Müller"));
        assert!(!levenshtein_at_most_one("Meier", "Schulz"));
    }
}
