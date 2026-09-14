//! Rust port of LanguageTool's `ConfusionProbabilityRule` /
//! `EnglishConfusionProbabilityRule` (base id `CONFUSION_RULE`).
//!
//! Uses confusion sets (`confusion_sets.txt`) plus n-gram lookups to decide
//! which word suits the context best. No-op when the language model is
//! untrained (all lookups return zero probability, so no alternative ever
//! clears the factor threshold — same effect as Java's MIN_COVERAGE guard).

use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement, rule::Rule,
};
use og_languagemodel::LanguageModel;
use std::collections::HashMap;
use std::sync::Arc;

/// A two-word confusion set.
#[derive(Clone, Debug)]
pub(crate) struct ConfusionPair {
    pub term1: String,
    pub term2: String,
    /// How much more probable the alternative must be.
    pub factor: f64,
    /// false = only direction term1 -> term2 is flagged (l2 "a -> b" syntax).
    pub bidirectional: bool,
}

/// Parse LT confusion-set data (both the `a; b; factor` homophone format of
/// confusion_sets.txt and the directional `a -> b; factor` format of the
/// confusion_sets_l2_*.txt false-friend files).
pub(crate) fn parse_confusion_pairs(data: &str) -> HashMap<String, Vec<ConfusionPair>> {
    let mut map: HashMap<String, Vec<ConfusionPair>> = HashMap::new();
    for line in data.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let bidirectional = !line.contains("->");
        let fields: Vec<&str> = line
            .split(';')
            .map(|f| f.trim())
            .filter(|f| !f.is_empty())
            .collect();
        if fields.len() < 2 {
            continue;
        }
        let factor = fields
            .iter()
            .rev()
            .find_map(|f| f.parse::<f64>().ok())
            .unwrap_or(1.0);
        let terms: Vec<&str> = fields
            .iter()
            .filter(|f| f.parse::<f64>().is_err())
            .flat_map(|f| f.split("->"))
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        if terms.len() != 2 || terms[0].eq_ignore_ascii_case(terms[1]) {
            continue;
        }
        let pair = ConfusionPair {
            term1: terms[0].to_lowercase(),
            term2: terms[1].to_lowercase(),
            factor,
            bidirectional,
        };
        map.entry(pair.term1.clone())
            .or_default()
            .push(pair.clone());
        map.entry(pair.term2.clone()).or_default().push(pair);
    }
    map
}

/// Shared matching core for confusion-set rules (Java
/// `ConfusionProbabilityRule.match`). Exposed to the false-friend rules.
pub(crate) struct ConfusionCore {
    pub lm: Arc<LanguageModel>,
    pub word_to_pairs: HashMap<String, Vec<ConfusionPair>>,
}

impl ConfusionCore {
    pub fn new(lm: Arc<LanguageModel>, word_to_pairs: HashMap<String, Vec<ConfusionPair>>) -> Self {
        Self { lm, word_to_pairs }
    }

    /// Trigram probability of `word` in the context of `prev`/`next`
    /// (Java `LanguageModelUtils.get3gramProbabilityFor`).
    fn trigram_prob(&self, prev: &str, word: &str, next: &str) -> f64 {
        self.lm.get_probability(&[
            &prev.to_lowercase(),
            &word.to_lowercase(),
            &next.to_lowercase(),
        ])
    }

    pub fn match_sentence(
        &self,
        sentence: &AnalyzedSentence,
        rule_id: &str,
        rule_description: &str,
        category: Category,
    ) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        if self.lm.token_count() == 0 {
            return matches;
        }
        let tokens = sentence.non_whitespace_tokens();
        if tokens.len() < 3 {
            return matches;
        }
        // First and last real tokens are not scored: Java pads with
        // _START_/_END_ pseudo tokens for edge context (documented
        // divergence).
        for i in 1..tokens.len().saturating_sub(1) {
            let token_str = tokens[i].token().token();
            let pairs = match self.word_to_pairs.get(&token_str.to_lowercase()) {
                Some(p) => p,
                None => continue,
            };
            let prev = tokens[i - 1].token().token();
            let next = tokens[i + 1].token().token();
            for pair in pairs {
                let token_lower = token_str.to_lowercase();
                // The token itself and the other term of the pair:
                let (from_text, alternative) = if pair.term1 == token_lower {
                    (&pair.term1, &pair.term2)
                } else {
                    (&pair.term2, &pair.term1)
                };
                let p1 = self.trigram_prob(prev, &token_str.to_lowercase(), next);
                let p2 = self.trigram_prob(prev, alternative, next);
                if p2 > 0.0 && p2 > p1 * pair.factor {
                    if !pair.bidirectional && alternative == &pair.term1 {
                        // only direction term1 -> term2 is possible
                        continue;
                    }
                    let message = format!(
                        "Statistics-based suggestion: '{alternative}' might be correct here instead of '{from_text}'."
                    );
                    let rm_rule = RuleMatchRule::new(rule_id, rule_description)
                        .with_sub_id(format!("{alternative}_{from_text}"))
                        .with_category(category.clone());
                    let start = tokens[i].token().start();
                    let end = tokens[i].token().end();
                    let ctx_start = start.saturating_sub(40);
                    let ctx_end = (end + 40).min(sentence.text().len());
                    matches.push(
                        RuleMatch::new(
                            message,
                            start,
                            end - start,
                            rm_rule,
                            RuleMatchContext::new(
                                sentence.text()[ctx_start..ctx_end].to_string(),
                                ctx_start,
                                ctx_end - ctx_start,
                            ),
                        )
                        .with_replacements(vec![SuggestedReplacement::new(match_case(
                            token_str,
                            alternative,
                        ))])
                        .with_sentence(sentence.text().to_string()),
                    );
                    break;
                }
            }
        }
        matches
    }
}

/// Keep the suggestion's initial casing aligned with the text token.
fn match_case(template: &str, word: &str) -> String {
    let mut template_chars = template.chars();
    let mut word_chars = word.chars();
    match (template_chars.next(), word_chars.next()) {
        (Some(t), Some(w)) if t.is_uppercase() => {
            w.to_uppercase().collect::<String>() + word_chars.as_str()
        }
        _ => word.to_string(),
    }
}

/// Rust port of `EnglishConfusionProbabilityRule`
/// (confusion_sets.txt homophones, id `CONFUSION_RULE`).
pub struct EnglishConfusionProbabilityRule {
    core: ConfusionCore,
}

impl EnglishConfusionProbabilityRule {
    pub fn new(lm: Arc<LanguageModel>) -> Self {
        let data = og_spell::read_lt_resource(
            "en/src/main/resources/org/languagetool/resource/en/confusion_sets.txt",
        )
        .unwrap_or_default();
        Self {
            core: ConfusionCore::new(lm, parse_confusion_pairs(&data)),
        }
    }
}

impl Rule for EnglishConfusionProbabilityRule {
    fn id(&self) -> &str {
        "CONFUSION_RULE"
    }

    fn description(&self) -> &str {
        "Statistics-based suggestions for easily confused words"
    }

    fn is_default_on(&self) -> bool {
        // Java does not setDefaultOff; the rule only exists when a language
        // model is configured (no-op here when untrained).
        true
    }

    fn category(&self) -> Category {
        Category::new("TYPOS", "Possible Typo")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Other("misspelling".to_string())
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        self.core
            .match_sentence(sentence, self.id(), self.description(), self.category())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};

    fn sentence_from_tokens(words: &[&str]) -> AnalyzedSentence {
        let text = words.join(" ");
        let mut s = AnalyzedSentence::new(text.clone(), 0, text.len());
        let mut pos = 0usize;
        let tokens = words
            .iter()
            .map(|w| {
                let t = AnalyzedToken::new(*w, pos, pos + w.len());
                pos += w.len() + 1;
                AnalyzedTokenReadings::new(t)
            })
            .collect();
        s.set_tokens(tokens);
        s
    }

    #[test]
    fn parser_handles_both_formats() {
        let pairs = parse_confusion_pairs("there; their; 10\nbecome -> get; 100000;\n");
        assert!(pairs.contains_key("there"));
        assert!(pairs.contains_key("their"));
        assert!(pairs.contains_key("become"));
        let pair = &pairs["become"][0];
        assert!(!pair.bidirectional, "arrow syntax is directional");
        assert_eq!(pair.term2, "get");
        assert_eq!(pair.factor, 100000.0);
    }

    fn synthetic_core(lm: Arc<LanguageModel>) -> ConfusionCore {
        let pairs = parse_confusion_pairs("there; their; 10\n");
        ConfusionCore::new(lm, pairs)
    }

    #[test]
    fn untrained_model_is_noop() {
        let core = synthetic_core(Arc::new(LanguageModel::new(3)));
        let s = sentence_from_tokens(&["I", "think", "their", "is", "a", "problem"]);
        assert!(
            core.match_sentence(&s, "CONFUSION_RULE", "d", Category::new("TYPOS", "Typo"))
                .is_empty()
        );
    }

    #[test]
    fn trained_core_suggests_confusion_alternative() {
        let mut lm = LanguageModel::new(3);
        for _ in 0..20 {
            lm.add_text("i think there is a problem with this approach");
        }
        let core = synthetic_core(Arc::new(lm));
        let s = sentence_from_tokens(&["I", "think", "their", "is", "a", "problem"]);
        let matches =
            core.match_sentence(&s, "CONFUSION_RULE", "d", Category::new("TYPOS", "Typo"));
        assert!(!matches.is_empty(), "expected confusion suggestion");
        assert!(
            matches[0]
                .replacements()
                .iter()
                .any(|r| r.value() == "there")
        );
    }

    #[test]
    fn directional_pair_only_flags_one_direction() {
        let mut lm = LanguageModel::new(3);
        for _ in 0..20 {
            lm.add_text("she become a star");
        }
        let core = ConfusionCore::new(Arc::new(lm), parse_confusion_pairs("become -> get; 2;\n"));
        // "get" where "become" fits is NOT flagged (only become -> get):
        let s = sentence_from_tokens(&["she", "get", "a", "star"]);
        let matches =
            core.match_sentence(&s, "CONFUSION_RULE", "d", Category::new("TYPOS", "Typo"));
        assert!(matches.is_empty(), "reverse direction must not be flagged");
    }
}
