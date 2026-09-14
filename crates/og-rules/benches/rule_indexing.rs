//! Rule-indexing and XML-compile benchmarks (PLAN Phase 12, plans/08).
//!
//! Run with: `cargo bench -p og-rules` (harness = false; std-only runner —
//! no criterion dependency, works fully offline).
//!
//! Data-backed: compiles the real English grammar.xml (5,545 rules) through
//! the shared regex cache, then measures first-token index build and
//! serial-vs-parallel indexed matching. Prints SKIP lines (exit 0) when the
//! LT resource root is absent.

use std::hint::black_box;
use std::time::Instant;

use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};
use og_rules::PatternRuleEngine;
use og_spell::resource::lt_resource_root;
use og_tokenizer::{
    DefaultSentenceSplitter, DefaultWordTokenizer, SentenceTokenizer, WordTokenizer,
};
use og_xml::XmlCompiler;

fn bench(name: &str, iters: usize, mut f: impl FnMut()) {
    f(); // warmup
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    let elapsed = start.elapsed();
    println!(
        "{:<58} {:>14.1} ns/op  ({} iters, {:?} total)",
        name,
        elapsed.as_nanos() as f64 / iters as f64,
        iters,
        elapsed
    );
}

/// Tokenize a sentence into analyzed readings with sentence-local byte
/// offsets (same shape the checker pipeline produces).
fn analyze(text: &str) -> AnalyzedSentence {
    let tokenizer = DefaultWordTokenizer::new();
    let mut sentence = AnalyzedSentence::new(text, 0, text.len());
    let tokens = tokenizer
        .tokenize(text)
        .into_iter()
        .map(|t| AnalyzedTokenReadings::new(AnalyzedToken::from_token(&t)))
        .collect();
    sentence.set_tokens(tokens);
    sentence
}

fn main() {
    let Some(root) = lt_resource_root() else {
        println!("# SKIP all rule-indexing benches (no LT resource root)");
        return;
    };
    let grammar_path = root.join("en/src/main/resources/org/languagetool/rules/en/grammar.xml");
    if !grammar_path.exists() {
        println!("# SKIP all rule-indexing benches (no en grammar.xml)");
        return;
    }
    let xml = std::fs::read_to_string(&grammar_path).expect("read grammar.xml");

    // XML -> CompiledRuleSet (includes the process-wide regex cache; a
    // warm in-process re-compile hits the cache, a cold one does not).
    let t = Instant::now();
    let rule_set = XmlCompiler::new()
        .compile_file(&xml)
        .expect("compile grammar.xml");
    let cold = t.elapsed();
    let t = Instant::now();
    let _ = XmlCompiler::new().compile_file(&xml).expect("re-compile");
    let warm = t.elapsed();
    println!(
        "# XmlCompiler::compile_file: cold {:?} / warm (regex cache hit) {:?} for {} rules",
        cold,
        warm,
        rule_set.rules.len()
    );

    let rules: Vec<&og_xml::compiler::CompiledRule> = rule_set.rules.iter().collect();

    // First-token index build.
    let mut engine = PatternRuleEngine::new();
    bench("PatternRuleEngine::build_index (5,545 rules)", 10, || {
        black_box(engine.build_index(black_box(&rules)));
    });

    // Matching throughput on a small document (12 sentences, typical prose
    // with a few errors the grammar rules can fire on).
    let doc = concat!(
        "Their is a problem with you re spelling. ",
        "I has a apple and the dogs barks loudly at the neighbor. ",
        "She did not went to the school yesterday. ",
        "This are the things what makes me happy. ",
        "He don't know nothing about it. ",
        "We was walking in the park when it start to rain. ",
        "The result of the experiment was more better than expected. ",
        "Its time to leave, isn't it? ",
        "Between you and I, this is the best option. ",
        "There is many reasons why this happen. ",
        "The childrens plays in the garden every days. ",
        "I could of finished it if he would of helped. ",
    );
    let splitter = DefaultSentenceSplitter::new();
    let sentences: Vec<AnalyzedSentence> = splitter
        .split(doc)
        .iter()
        .map(|s| analyze(s.text()))
        .collect();
    let matches = engine.match_indexed_rules(&rules, &sentences[0]);
    println!(
        "# sanity: {} matches on sentence 1 (rules actually fire)",
        matches.len()
    );

    bench("match_indexed_rules serial (12 sentences)", 20, || {
        for s in &sentences {
            black_box(engine.match_indexed_rules(black_box(&rules), black_box(s)));
        }
    });
    bench(
        "match_indexed_rules_parallel rayon (12 sentences)",
        20,
        || {
            black_box(
                engine.match_indexed_rules_parallel(black_box(&rules), black_box(&sentences)),
            );
        },
    );
}
