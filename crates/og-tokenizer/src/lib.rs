//! Sentence splitting and word tokenization for the OpenGrammar engine.
//!
//! NOTE ON TRAIT DUALITY: this crate defines its own [`SentenceTokenizer`] /
//! [`WordTokenizer`] traits over the raw types ([`Sentence`]/[`Token`]),
//! while `og_core::checker` defines same-named traits over the analysis
//! types (`SentenceRange`/`AnalyzedTokenReadings`). Language modules bridge
//! between the two: they run these raw tokenizers, then adapt the output
//! for the `Checker` pipeline. Do not confuse the two trait families.

pub mod sentence_splitter;
pub mod word_tokenizer;

pub use sentence_splitter::*;
pub use word_tokenizer::*;

use og_core::{Sentence, Token};

/// Splits raw text into [`Sentence`]s with byte offsets into the source.
pub trait SentenceTokenizer: Send + Sync {
    fn split(&self, text: &str) -> Vec<Sentence>;
}

/// Splits raw text into [`Token`]s with byte offsets into the source.
pub trait WordTokenizer: Send + Sync {
    fn tokenize(&self, text: &str) -> Vec<Token>;
}
