use og_core::checker::{SentenceTokenizer, WordTokenizer};
use og_core::{AnalyzedToken, AnalyzedTokenReadings, SentenceRange};

pub struct SentenceSplitterAdapter;

impl SentenceTokenizer for SentenceSplitterAdapter {
    fn split(&self, text: &str) -> Vec<SentenceRange> {
        static SPLITTER: std::sync::OnceLock<og_tokenizer::DefaultSentenceSplitter> =
            std::sync::OnceLock::new();
        let splitter = SPLITTER.get_or_init(og_tokenizer::DefaultSentenceSplitter::new);
        use og_tokenizer::SentenceTokenizer as _;
        let sentences = splitter.split(text);
        sentences
            .into_iter()
            .map(|s| SentenceRange {
                start: s.start(),
                end: s.end(),
            })
            .collect()
    }
}

pub struct WordTokenizerAdapter;

impl WordTokenizer for WordTokenizerAdapter {
    fn tokenize(&self, text: &str, offset: usize) -> Vec<AnalyzedTokenReadings> {
        use og_tokenizer::WordTokenizer as _;
        static EN: std::sync::OnceLock<std::sync::Arc<og_tokenizer::DefaultWordTokenizer>> =
            std::sync::OnceLock::new();
        let tokenizer =
            EN.get_or_init(
                || match og_tagger::english_tagger::shared_english_tagger() {
                    Some(tagger) => {
                        std::sync::Arc::new(og_tokenizer::DefaultWordTokenizer::new_english(
                            std::sync::Arc::new(move |w: &str| tagger.is_word_tagged(w)),
                        ))
                    }
                    None => std::sync::Arc::new(og_tokenizer::DefaultWordTokenizer::new()),
                },
            );
        let tokens = tokenizer.split_underscores(tokenizer.tokenize(text));
        tokens
            .into_iter()
            .map(|t| {
                let at = AnalyzedToken::new(t.text(), t.start() + offset, t.end() + offset);
                AnalyzedTokenReadings::new(at)
            })
            .collect()
    }
}
