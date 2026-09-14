//! Sentence-splitting and token-span benchmarks (PLAN Phase 12, plans/08).
//!
//! Run with: `cargo bench -p og-tokenizer` (harness = false; std-only runner —
//! no criterion dependency, works fully offline). Pure in-process benches on
//! embedded text: no resource checkout needed.

use std::hint::black_box;
use std::time::Instant;

use og_tokenizer::{
    DefaultSentenceSplitter, DefaultWordTokenizer, SentenceTokenizer, WordTokenizer,
};

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

/// ~240 sentences of mixed prose (abbreviations, URLs, numbers, quotes) —
/// exercises abbreviation handling and URL/email span detection.
fn corpus() -> String {
    let paragraph = concat!(
        "Dr. Smith visited https://example.com/path?q=1 on Jan. 5th, 2024. ",
        "She said \"it's a well-known fact that e.g. vowels and consonants alternate.\" ",
        "The temperature reached 32.5 degrees; nobody complained. ",
        "Contact jane.doe@example.org or visit www.example.org for details. ",
        "It's the 3rd time we've tried, isn't it? Yes! ",
        "The U.S.A. delegation arrived at 9 a.m. sharp. ",
    );
    let mut text = String::new();
    for i in 0..10 {
        text.push_str(&format!(
            "Section {i}. {paragraph}{paragraph}{paragraph}{paragraph}"
        ));
    }
    text
}

fn main() {
    let text = corpus();
    println!(
        "# corpus: {} bytes, {} chars",
        text.len(),
        text.chars().count()
    );

    let splitter = DefaultSentenceSplitter::new();
    let sentences = splitter.split(&text);
    println!("# split into {} sentences", sentences.len());

    let first = sentences
        .first()
        .map(|s| s.text().to_string())
        .unwrap_or_default();

    bench("DefaultSentenceSplitter::split (full corpus)", 50, || {
        black_box(splitter.split(black_box(&text)));
    });

    let tokenizer = DefaultWordTokenizer::new();
    bench(
        "DefaultWordTokenizer::tokenize (full corpus, spans)",
        50,
        || {
            black_box(tokenizer.tokenize(black_box(&text)));
        },
    );
    bench(
        "DefaultWordTokenizer::tokenize (one sentence)",
        200_000,
        || {
            black_box(tokenizer.tokenize(black_box(&first)));
        },
    );

    // Span correctness sanity (evidence the spans the bench measures are real
    // byte offsets, not just token strings).
    let tokens = tokenizer.tokenize(&first);
    for t in &tokens {
        let span = &first[t.start()..t.end()];
        assert_eq!(span, t.text());
    }
    println!("# span sanity: {} tokens verified", tokens.len());
}
