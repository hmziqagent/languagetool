// N-gram language model support for traditional statistical features
// (Phase 6/7 deliverable backing EnglishNgramProbabilityRule and
// UpperCaseNgramRule — see docs/plans/05-native-rules.md).
//
// No AI/LLM — this is a plain count-based n-gram model with add-k smoothing
// and stupid-backoff to shorter contexts.
//
// Divergence from Java LT (recorded in docs/plans/compatibility-matrix.md):
// LT ships a Lucene-indexed n-gram database built from large corpora; that
// binary format is not parsed here. Instead the model is trained at runtime
// from a plain-text corpus (file or in-memory), which keeps the statistical
// API shape identical while the backing data is whatever corpus the host
// provides.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

/// Default n-gram order (LT's rules mostly consult bigrams).
pub const DEFAULT_ORDER: usize = 2;

/// Count-based n-gram language model with add-k smoothing and backoff.
pub struct LanguageModel {
    order: usize,
    /// n-gram (for n in 1..=order) -> occurrence count, keyed by the joined
    /// token sequence.
    counts: HashMap<Vec<String>, u64>,
    /// context (n-1 tokens) -> number of n-grams observed with that context.
    context_totals: HashMap<Vec<String>, u64>,
    /// Distinct tokens observed (vocabulary, for smoothing).
    vocab: HashSet<String>,
    /// Total token count.
    tokens: u64,
    /// Add-k smoothing constant.
    smoothing: f64,
}

impl LanguageModel {
    /// Create an empty model of the given order (>= 1).
    pub fn new(order: usize) -> Self {
        Self {
            order: order.max(1),
            counts: HashMap::new(),
            context_totals: HashMap::new(),
            vocab: HashSet::new(),
            tokens: 0,
            smoothing: 0.1,
        }
    }

    /// N-gram order of this model.
    pub fn order(&self) -> usize {
        self.order
    }

    /// Number of distinct tokens seen during training.
    pub fn vocabulary_size(&self) -> usize {
        self.vocab.len()
    }

    /// Total token count seen during training.
    pub fn token_count(&self) -> u64 {
        self.tokens
    }

    /// Raw occurrence count of an exact n-gram (no smoothing).
    pub fn count(&self, words: &[&str]) -> u64 {
        self.owned_count(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>())
    }

    fn owned_count(&self, gram: &[String]) -> u64 {
        self.counts.get(gram).copied().unwrap_or(0)
    }

    /// Train on a raw text: lowercase-split into word tokens.
    pub fn add_text(&mut self, text: &str) {
        let tokens: Vec<String> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .map(|t| t.to_lowercase())
            .collect();
        if tokens.is_empty() {
            return;
        }
        self.add_tokens(&tokens);
    }

    /// Train on pre-tokenized input. Tokens are lowercased so that
    /// case-insensitive lookups match regardless of corpus casing.
    pub fn add_tokens(&mut self, tokens: &[String]) {
        if tokens.is_empty() {
            return;
        }
        let tokens: Vec<String> = tokens.iter().map(|t| t.to_lowercase()).collect();
        for token in &tokens {
            self.vocab.insert(token.clone());
        }
        for n in 1..=self.order {
            for gram in tokens.windows(n) {
                *self.counts.entry(gram.to_vec()).or_insert(0) += 1;
                // Context totals are recorded for every n >= 2 so that
                // queries shorter than the model order (and higher orders)
                // find their context instead of always backing off.
                if n >= 2 {
                    let ctx = &gram[..n - 1];
                    *self.context_totals.entry(ctx.to_vec()).or_insert(0) += 1;
                }
            }
        }
        self.tokens += tokens.len() as u64;
    }

    /// Train from a plain-text corpus file.
    pub fn from_corpus_file(path: &Path, order: usize) -> std::io::Result<Self> {
        let text = fs::read_to_string(path)?;
        let mut model = Self::new(order);
        model.add_text(&text);
        Ok(model)
    }

    /// Probability of the LAST word given the preceding words in `words`
    /// (an n-gram), using add-k smoothing with stupid-backoff to shorter
    /// contexts. `words` longer than the model order use the trailing
    /// (order) tokens; empty input returns 0.0.
    ///
    /// This is the drop-in replacement for the stub `get_probability` that
    /// rules like EnglishNgramProbabilityRule compare against a threshold.
    pub fn get_probability(&self, words: &[&str]) -> f64 {
        if words.is_empty() || self.tokens == 0 {
            return 0.0;
        }
        let owned: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        let start = owned.len().saturating_sub(self.order);
        let gram: Vec<String> = owned[start..].to_vec();
        self.gram_probability(&gram)
    }

    fn gram_probability(&self, gram: &[String]) -> f64 {
        let n = gram.len();
        if n == 0 {
            return 0.0;
        }
        let context: Vec<String> = gram[..n - 1].to_vec();

        if n == 1 {
            // Unigram: add-k over vocabulary.
            let count = self.owned_count(gram) as f64;
            let v = self.vocab.len().max(1) as f64;
            return (count + self.smoothing) / (self.tokens as f64 + self.smoothing * v);
        }

        if let Some(total) = self.context_totals.get(&context).copied() {
            let count = self.owned_count(gram) as f64;
            let v = self.vocab.len().max(1) as f64;
            // Seen context: smoothed MLE.
            return (count + self.smoothing) / (total as f64 + self.smoothing * v);
        }

        // Stupid backoff: scaled probability of the shorter n-gram.
        0.4 * self.gram_probability(&gram[1..])
    }
}

impl Default for LanguageModel {
    fn default() -> Self {
        Self::new(DEFAULT_ORDER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trained() -> LanguageModel {
        let mut lm = LanguageModel::new(2);
        lm.add_text("the cat sat on the mat the cat ran");
        lm
    }

    #[test]
    fn counts_are_stored() {
        let lm = trained();
        assert_eq!(lm.count(&["the", "cat"]), 2);
        assert_eq!(lm.count(&["cat", "sat"]), 1);
        assert_eq!(lm.count(&["dog", "barks"]), 0);
        assert_eq!(lm.token_count(), 9);
        assert!(lm.vocabulary_size() >= 6);
    }

    #[test]
    fn seen_bigram_beats_unseen() {
        let lm = trained();
        let seen = lm.get_probability(&["the", "cat"]);
        let unseen = lm.get_probability(&["zebra", "quantum"]);
        assert!(
            seen > unseen,
            "seen bigram ({seen}) should outrank unseen ({unseen})"
        );
    }

    #[test]
    fn case_insensitive_lookup() {
        let lm = trained();
        let (a, b) = (
            lm.get_probability(&["The", "Cat"]),
            lm.get_probability(&["the", "cat"]),
        );
        assert!((a - b).abs() < f64::EPSILON);
    }

    #[test]
    fn backoff_used_for_unseen_contexts() {
        let lm = trained();
        // Unseen context backs off to the unigram of "cat" (> 0).
        let p = lm.get_probability(&["zebra", "cat"]);
        assert!(p > 0.0);
    }

    #[test]
    fn empty_and_untrained_return_zero() {
        assert_eq!(trained().get_probability(&[]), 0.0);
        assert_eq!(LanguageModel::new(2).get_probability(&["a", "b"]), 0.0);
    }

    #[test]
    fn longer_sequences_use_trailing_window() {
        let lm = trained();
        let p1 = lm.get_probability(&["sat", "on", "the", "cat"]);
        let p2 = lm.get_probability(&["the", "cat"]);
        assert!((p1 - p2).abs() < f64::EPSILON);
    }

    #[test]
    fn unigram_counts_are_not_double_counted() {
        let mut lm = LanguageModel::new(2);
        lm.add_text("the cat the cat the");
        // "the" occurs 3 times, "cat" twice — exactly.
        assert_eq!(lm.count(&["the"]), 3);
        assert_eq!(lm.count(&["cat"]), 2);
        assert_eq!(lm.token_count(), 5);
        assert_eq!(lm.vocabulary_size(), 2);
    }

    #[test]
    fn probabilities_never_exceed_one() {
        let lm = trained();
        // Every bigram and unigram of the training text must be a legal
        // probability (<= 1.0).
        let words = ["the", "cat", "sat", "on", "the", "mat", "ran"];
        for w in words {
            assert!(
                lm.get_probability(&[w]) <= 1.0 + 1e-9,
                "unigram P({w}) > 1.0"
            );
        }
        for pair in words.windows(2) {
            let p = lm.get_probability(&[pair[0], pair[1]]);
            assert!(p <= 1.0 + 1e-9, "bigram P({pair:?}) = {p} > 1.0");
            assert!(p > 0.0);
        }
    }

    #[test]
    fn add_tokens_is_lowercased_for_consistent_lookup() {
        let mut lm = LanguageModel::new(2);
        lm.add_tokens(&["The".to_string(), "CAT".to_string()]);
        assert_eq!(lm.count(&["the", "cat"]), 1);
        assert_eq!(
            lm.get_probability(&["the", "cat"]),
            lm.get_probability(&["THE", "Cat"])
        );
    }

    #[test]
    fn trains_from_corpus_file() {
        let dir = std::env::temp_dir().join("og-lm-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("corpus.txt");
        std::fs::write(&path, "a rose is a rose is a rose").unwrap();
        let lm = LanguageModel::from_corpus_file(&path, 2).unwrap();
        assert_eq!(lm.count(&["a", "rose"]), 3);
        assert!(lm.get_probability(&["a", "rose"]) > lm.get_probability(&["rose", "zebra"]));
    }
}
