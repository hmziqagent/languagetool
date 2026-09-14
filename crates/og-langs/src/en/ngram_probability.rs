//! Native port of LanguageTool's `NgramProbabilityRule` /
//! `EnglishNgramProbabilityRule` and `UpperCaseNgramRule`.
//!
//! Backed by `og_languagemodel::LanguageModel` (count-based, corpus-trained).
//! Both rules are no-ops when the model is untrained (`token_count() == 0`),
//! matching Java behavior where the rules only exist when an ngram index is
//! configured. Divergences from the Java implementation are recorded in
//! docs/plans/compatibility-matrix.md.

use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement, rule::Rule,
};
use og_languagemodel::LanguageModel;
use std::sync::Arc;

/// Rust port of Java `NgramProbabilityRule` / `EnglishNgramProbabilityRule`
/// (id `NGRAM_RULE`, default OFF upstream — "too many false alarms").
///
/// Flags trigrams whose corpus probability falls below `min_probability`.
/// Java's suggestion machinery needs the POS synthesizer (not ported); as a
/// subset, a naive singular/plural variant of the middle token is suggested
/// when it is at least as probable.
pub struct EnglishNgramProbabilityRule {
    lm: Arc<LanguageModel>,
}

impl EnglishNgramProbabilityRule {
    pub fn new(lm: Arc<LanguageModel>) -> Self {
        Self { lm }
    }

    fn naive_variant(token: &str) -> Vec<String> {
        // Java generates alternatives via POS replacement + synthesizer
        // (VBG<->VB, NN<->NNS, ...). Without a synthesizer we only try the
        // trivial noun-number variant.
        let mut variants = Vec::new();
        let lower = token.to_lowercase();
        if let Some(stem) = lower.strip_suffix("es").filter(|s| s.len() >= 3) {
            variants.push(stem.to_string());
        }
        if let Some(stem) = lower
            .strip_suffix('s')
            .filter(|s| s.len() >= 2 && !s.ends_with('s'))
        {
            variants.push(stem.to_string());
        }
        if !lower.ends_with('s') {
            variants.push(format!("{lower}s"));
        }
        variants
    }
}

impl Rule for EnglishNgramProbabilityRule {
    fn id(&self) -> &str {
        "NGRAM_RULE"
    }

    fn description(&self) -> &str {
        "Statistics-based suggestions (rule for locating rare ngrams)"
    }

    fn is_default_on(&self) -> bool {
        false // Java: setDefaultOff(), too many false alarms
    }

    fn category(&self) -> Category {
        Category::new("TYPOS", "Possible Typo")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Other("misspelling".to_string())
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
        for i in 1..tokens.len() - 1 {
            let prev = tokens[i - 1].token();
            let cur = tokens[i].token();
            let next = tokens[i + 1].token();
            let trigram = [
                prev.token().to_lowercase(),
                cur.token().to_lowercase(),
                next.token().to_lowercase(),
            ];
            // Java flags prob < 1e-14, which with the Lucene index means
            // "unattested trigram". Our count model applies add-k smoothing
            // (every trigram gets a small positive probability), so the
            // absolute threshold is unusable; we flag trigrams that are
            // unattested while all three words are in the corpus vocabulary
            // (an unseen combination of known words — documented divergence).
            let occurrences = self.lm.count(&[&trigram[0], &trigram[1], &trigram[2]]);
            let all_known = trigram.iter().all(|w| self.lm.count(&[w.as_str()]) > 0);
            if occurrences == 0 && all_known {
                let ngram = format!("{} {} {}", prev.token(), cur.token(), next.token());
                // Suggest naive singular/plural variants of the middle token
                // that are attested in the same context (subset of Java's
                // synthesizer-based alternatives).
                let mut suggestions = Vec::new();
                for variant in Self::naive_variant(cur.token()) {
                    let alt = [trigram[0].clone(), variant.clone(), trigram[2].clone()];
                    if self.lm.count(&[&alt[0], &alt[1], &alt[2]]) > 0 {
                        suggestions.push(format!("{} {} {}", prev.token(), variant, next.token()));
                    }
                }
                let message = format!(
                    "The phrase '{ngram}' rarely occurs in the reference corpus ({occurrences} times)"
                );
                let start = prev.start();
                let end = next.end();
                let rm_rule = RuleMatchRule::new(self.id(), self.description())
                    .with_category(self.category());
                let ctx_start = start.saturating_sub(40);
                let ctx_end = (end + 40).min(sentence.text().len());
                let replacements: Vec<SuggestedReplacement> = suggestions
                    .iter()
                    .map(|s| SuggestedReplacement::new(s.clone()))
                    .collect();
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
                    .with_replacements(replacements)
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
    fn untrained_model_is_noop() {
        let rule = EnglishNgramProbabilityRule::new(Arc::new(LanguageModel::new(3)));
        let s = sentence_from_tokens(&["I", "didn't", "now", "where", "it", "came"]);
        assert!(rule.match_sentence(&s).is_empty());
    }

    #[test]
    fn trained_model_flags_rare_trigram() {
        let mut lm = LanguageModel::new(3);
        for _ in 0..3 {
            lm.add_text("she saw two cats here");
        }
        lm.add_text("the cat sat on the mat");
        let rule = EnglishNgramProbabilityRule::new(Arc::new(lm));
        // "two cat here" is unattested while all words are in the vocabulary
        let s = sentence_from_tokens(&["She", "saw", "two", "cat", "here"]);
        let matches = rule.match_sentence(&s);
        assert!(!matches.is_empty(), "expected rare trigram to be flagged");
        assert!(matches[0].message().contains("rarely occurs"));
        // suggestion should propose the attested "two cats here" (the
        // flagged trigram "two cat here"; "saw two cat" is also flagged
        // but has no attested variant)
        assert!(matches.iter().any(|m| {
            m.replacements()
                .iter()
                .any(|r| r.value() == "two cats here")
        }));
    }

    #[test]
    fn frequent_trigram_not_flagged() {
        let mut lm = LanguageModel::new(3);
        for _ in 0..3 {
            lm.add_text("she drinks coffee every day");
        }
        let rule = EnglishNgramProbabilityRule::new(Arc::new(lm));
        let s = sentence_from_tokens(&["She", "drinks", "coffee", "every", "day"]);
        assert!(rule.match_sentence(&s).is_empty());
    }
}
