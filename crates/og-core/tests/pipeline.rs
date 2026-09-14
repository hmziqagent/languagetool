//! End-to-end Checker pipeline tests covering the Phase 2 contracts:
//! `RuleContext` plumbing, `AnalyzedDocument` for text-level rules,
//! and rule reset lifecycle.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use og_core::checker::{
    SentenceRange, SentenceTokenizer as CoreSentenceTokenizer, WordTokenizer as CoreWordTokenizer,
};
use og_core::{
    AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, Category, CheckRequest, Checker,
    Language, Rule, RuleContext, RuleMatch, RuleMatchRule, TextLevelRule,
};

/// Minimal sentence tokenizer for tests: splits after ". ".
struct TestSentenceTokenizer;

impl CoreSentenceTokenizer for TestSentenceTokenizer {
    fn split(&self, text: &str) -> Vec<SentenceRange> {
        let mut ranges = Vec::new();
        let mut start = 0;
        for (i, _) in text.match_indices(". ").map(|(i, _)| (i, ())) {
            ranges.push(SentenceRange { start, end: i + 1 });
            start = i + 2;
        }
        ranges.push(SentenceRange {
            start,
            end: text.len(),
        });
        ranges
    }
}

/// Minimal word tokenizer for tests: whitespace-separated words.
struct TestWordTokenizer;

impl CoreWordTokenizer for TestWordTokenizer {
    fn tokenize(&self, text: &str, offset: usize) -> Vec<AnalyzedTokenReadings> {
        let mut tokens = Vec::new();
        let mut pos = 0;
        for part in text.split(' ') {
            if part.is_empty() {
                pos += 1;
                continue;
            }
            tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                part,
                offset + pos,
                offset + pos + part.len(),
            )));
            pos += part.len() + 1;
        }
        tokens
    }
}

/// Sentence-level rule that only fires in picky mode (uses the context).
struct PickyRule;

impl Rule for PickyRule {
    fn id(&self) -> &str {
        "TEST_PICKY"
    }
    fn description(&self) -> &str {
        "fires only in picky mode"
    }
    fn match_sentence(&self, _sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        Vec::new()
    }
    fn match_sentence_with_context(
        &self,
        _sentence: &AnalyzedSentence,
        ctx: &RuleContext,
    ) -> Vec<RuleMatch> {
        if !ctx.is_picky() {
            return Vec::new();
        }
        vec![RuleMatch::new(
            "picky",
            0,
            1,
            RuleMatchRule::new("TEST_PICKY", "picky rule"),
            og_core::RuleMatchContext::new("t", 0, 1),
        )]
    }
}

/// Text-level rule reading the AnalyzedDocument instead of raw text.
struct DocRule {
    reset_calls: AtomicUsize,
}

impl DocRule {
    fn new() -> Self {
        Self {
            reset_calls: AtomicUsize::new(0),
        }
    }
}

impl TextLevelRule for DocRule {
    fn id(&self) -> &str {
        "TEST_DOC"
    }
    fn description(&self) -> &str {
        "document-level test rule"
    }
    fn match_text(&self, _text: &str, _sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        Vec::new()
    }
    fn match_document(
        &self,
        doc: &og_core::AnalyzedDocument,
        _ctx: &RuleContext,
    ) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        for sentence in doc.sentences() {
            // Count real word tokens (skip the synthetic <S> start marker).
            let words = sentence
                .tokens()
                .iter()
                .filter(|t| {
                    !t.is_whitespace()
                        && !t.is_sentence_start()
                        && t.token().token().chars().any(|c| c.is_alphabetic())
                })
                .count();
            if words > 0 {
                matches.push(RuleMatch::new(
                    format!("sentence with {words} words"),
                    sentence.start(),
                    sentence.start() + 1,
                    RuleMatchRule::new("TEST_DOC", "doc rule"),
                    og_core::RuleMatchContext::new(sentence.text(), sentence.start(), 1),
                ));
            }
        }
        matches
    }
    fn reset(&self) {
        self.reset_calls.fetch_add(1, Ordering::SeqCst);
    }
}

fn checker_with(picky_rule: Arc<PickyRule>, doc_rule: Arc<DocRule>) -> Checker {
    Checker::new()
        .with_sentence_tokenizer(Arc::new(TestSentenceTokenizer))
        .with_word_tokenizer(Arc::new(TestWordTokenizer))
        .add_rule(picky_rule)
        .add_text_level_rule(doc_rule)
}

#[test]
fn picky_rule_skipped_in_normal_mode() {
    let checker = checker_with(Arc::new(PickyRule), Arc::new(DocRule::new()));
    let result = checker.check(&CheckRequest::new("Hello there.", Language::English));
    assert!(result.matches.iter().all(|m| m.rule().id() != "TEST_PICKY"));
}

#[test]
fn picky_rule_fires_when_level_is_picky() {
    let checker = checker_with(Arc::new(PickyRule), Arc::new(DocRule::new()));
    let mut request = CheckRequest::new("Hello there.", Language::English);
    request.level = Some("picky".to_string());
    let result = checker.check(&request);
    assert!(result.matches.iter().any(|m| m.rule().id() == "TEST_PICKY"));
}

#[test]
fn doc_rule_receives_analyzed_document_with_offsets() {
    let checker = checker_with(Arc::new(PickyRule), Arc::new(DocRule::new()));
    let result = checker.check(&CheckRequest::new("One two. Three.", Language::English));
    let doc_matches: Vec<&RuleMatch> = result
        .matches
        .iter()
        .filter(|m| m.rule().id() == "TEST_DOC")
        .collect();
    assert_eq!(doc_matches.len(), 2, "one match per sentence");
    assert_eq!(doc_matches[0].message(), "sentence with 2 words");
    assert_eq!(doc_matches[0].offset(), 0);
    assert_eq!(doc_matches[1].message(), "sentence with 1 words");
    assert_eq!(doc_matches[1].offset(), 9);
}

#[test]
fn rules_are_reset_once_per_check() {
    let doc_rule = Arc::new(DocRule::new());
    let checker = checker_with(Arc::new(PickyRule), doc_rule.clone());
    checker.check(&CheckRequest::new("Hi.", Language::English));
    assert_eq!(doc_rule.reset_calls.load(Ordering::SeqCst), 1);
    checker.check(&CheckRequest::new("Hi again.", Language::English));
    assert_eq!(doc_rule.reset_calls.load(Ordering::SeqCst), 2);
}

#[test]
fn sentence_ranges_cover_whole_text() {
    let checker = checker_with(Arc::new(PickyRule), Arc::new(DocRule::new()));
    let result = checker.check(&CheckRequest::new("A. B.", Language::English));
    let flat: Vec<usize> = result
        .sentence_ranges
        .clone()
        .into_iter()
        .flatten()
        .collect();
    assert!(flat.contains(&0));
    assert!(flat.contains(&5));
}

#[test]
fn category_helper_defaults_are_used() {
    // Rules that don't override category() run under MISC and stay enabled.
    let checker = checker_with(Arc::new(PickyRule), Arc::new(DocRule::new()));
    let result = checker.check(&CheckRequest::new("Word.", Language::English));
    assert!(result.matches.iter().any(|m| m.rule().id() == "TEST_DOC"));
    let _ = Category::new("MISC", "Miscellaneous");
}
