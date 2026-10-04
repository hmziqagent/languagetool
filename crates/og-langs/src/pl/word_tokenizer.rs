use og_core::Token;
use std::sync::{Arc, OnceLock};

use super::tagger::PolishTagger;

const PREFIXES: [&str; 29] = [
    "arcy", "neo", "pre", "anty", "eks", "bez", "beze", "ekstra", "hiper", "infra", "kontr",
    "maksi", "midi", "między", "mini", "nad", "nade", "około", "ponad", "post", "pro", "przeciw",
    "pseudo", "super", "śród", "ultra", "wice", "wokół", "wokoło",
];

pub struct PolishWordTokenizer {
    tagger: Arc<PolishTagger>,
    inner: og_tokenizer::DefaultWordTokenizer,
}

impl PolishWordTokenizer {
    pub fn new(tagger: Arc<PolishTagger>) -> Self {
        Self {
            tagger,
            inner: og_tokenizer::DefaultWordTokenizer::new_generic(),
        }
    }

    pub fn shared() -> Arc<PolishWordTokenizer> {
        static CACHE: OnceLock<Arc<PolishWordTokenizer>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                let tagger = PolishTagger::shared_arc().expect("pl resource dir with polish.dict");
                Arc::new(PolishWordTokenizer::new(tagger))
            })
            .clone()
    }

    fn is_compound_parts(&self, parts: &[&str]) -> bool {
        let tagger = self.tagger.as_ref();
        let analyzed: Vec<Vec<og_core::AnalyzedToken>> = parts
            .iter()
            .map(|p| tagger.analyzed_tokens_at(p, 0, 0))
            .collect();
        let has_pos = |i: usize, tag: &str| {
            analyzed[i]
                .iter()
                .any(|r| r.pos_tags().first().is_some_and(|t| t == tag))
        };
        let has_partial = |i: usize, tag: &str| {
            analyzed[i]
                .iter()
                .any(|r| r.pos_tags().first().is_some_and(|t| t.contains(tag)))
        };
        match parts.len() {
            2 => {
                (has_pos(0, "adja") && has_partial(1, "adj:"))
                    || (has_partial(0, "subst:") && has_partial(1, "subst:"))
                    || (has_partial(0, "num:") && has_partial(1, "num:"))
            }
            3 => has_pos(0, "adja") && has_pos(1, "adja") && has_partial(2, "adj:"),
            _ => false,
        }
    }

    fn expand_token(&self, text: &str, start: usize, out: &mut Vec<Token>) {
        if text.chars().count() <= 1 || !text.contains('-') {
            out.push(Token::new(text, start, start + text.len()));
            return;
        }
        if let Some(stem) = text.strip_prefix('-') {
            out.push(Token::new("-", start, start + 1));
            self.expand_token(stem, start + 1, out);
            return;
        }
        if let Some(stem) = text.strip_suffix('-') {
            out.push(Token::new(stem, start, start + stem.len()));
            out.push(Token::new("-", start + stem.len(), start + stem.len() + 1));
            return;
        }
        let parts: Vec<&str> = text.split('-').collect();
        if PREFIXES.contains(&parts[0]) {
            out.push(Token::new(text, start, start + text.len()));
            return;
        }
        if last_part_starts_with_digit(&parts) {
            push_dashes_between(&parts, start, out);
            return;
        }
        let joined_tagged = !self.tagger.analyzed_tokens_at(text, 0, 0).is_empty();
        if !joined_tagged && self.is_compound_parts(&parts) {
            push_dashes_between(&parts, start, out);
            return;
        }
        out.push(Token::new(text, start, start + text.len()));
    }
}

fn last_part_starts_with_digit(parts: &[&str]) -> bool {
    parts
        .last()
        .and_then(|p| p.chars().next())
        .is_some_and(|c| c.is_ascii_digit())
}

fn push_dashes_between(parts: &[&str], start: usize, out: &mut Vec<Token>) {
    let mut offset = start;
    for (i, part) in parts.iter().enumerate() {
        out.push(Token::new(*part, offset, offset + part.len()));
        offset += part.len();
        if i != parts.len() - 1 {
            out.push(Token::new("-", offset, offset + 1));
            offset += 1;
        }
    }
}

impl og_tokenizer::WordTokenizer for PolishWordTokenizer {
    fn tokenize(&self, text: &str) -> Vec<Token> {
        let base = self.inner.tokenize(text);
        let mut out = Vec::with_capacity(base.len());
        for token in base {
            self.expand_token(token.text(), token.start(), &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_tokenizer::WordTokenizer as _;

    fn tokens(text: &str) -> Vec<String> {
        PolishWordTokenizer::shared()
            .tokenize(text)
            .into_iter()
            .map(|t| t.text().to_string())
            .collect()
    }

    #[test]
    fn java_pins_two_noun_compound_is_split() {
        assert_eq!(tokens("dziecko-geniusz"), vec!["dziecko", "-", "geniusz"]);
    }

    #[test]
    fn java_pins_adjectival_compound_is_split() {
        assert_eq!(
            tokens("polsko-indonezyjski"),
            vec!["polsko", "-", "indonezyjski"]
        );
        assert_eq!(
            tokens("polsko-francusko-niemiecki"),
            vec!["polsko", "-", "francusko", "-", "niemiecki"]
        );
    }

    #[test]
    fn java_pins_numeral_compound_is_split() {
        assert_eq!(
            tokens("osiemnaście-dwadzieścia"),
            vec!["osiemnaście", "-", "dwadzieścia"]
        );
    }

    #[test]
    fn java_pins_morphological_ending_is_not_split() {
        assert_eq!(tokens("SMS-y"), vec!["SMS-y"]);
        assert_eq!(tokens("Rabke-Zdroju"), vec!["Rabke-Zdroju"]);
        assert_eq!(tokens("niemiecki-lotniczy"), vec!["niemiecki-lotniczy"]);
    }

    #[test]
    fn java_pins_prefix_words_stay_whole() {
        assert_eq!(tokens("super-nowoczesny"), vec!["super-nowoczesny"]);
        assert_eq!(tokens("arcy-nowoczesny"), vec!["arcy-nowoczesny"]);
        assert_eq!(tokens("Bez-piesny"), vec!["Bez-piesny"]);
    }

    #[test]
    fn java_pins_number_ranges_split_at_dash() {
        assert_eq!(tokens("1-10"), vec!["1", "-", "10"]);
        assert_eq!(tokens("65-67"), vec!["65", "-", "67"]);
        assert_eq!(tokens("1915-1918"), vec!["1915", "-", "1918"]);
    }

    #[test]
    fn java_pins_trailing_and_leading_hyphens_split_off() {
        assert_eq!(tokens("Bytom-"), vec!["Bytom", "-"]);
        assert_eq!(tokens("-Bytom"), vec!["-", "Bytom"]);
        assert_eq!(tokens("--a"), vec!["-", "-", "a"]);
        assert_eq!(tokens("32-u"), vec!["32-u"]);
    }

    #[test]
    fn java_pins_token_offsets_survive_expansion() {
        let expanded = PolishWordTokenizer::shared().tokenize("A: polsko-indonezyjski b.");
        let joined: Vec<&str> = expanded.iter().map(|t| t.text()).collect();
        assert!(
            joined.contains(&"polsko") && joined.contains(&"-") && joined.contains(&"indonezyjski"),
            "{joined:?}"
        );
        let text = "A: polsko-indonezyjski b.";
        for token in &expanded {
            assert_eq!(
                &text[token.start()..token.end()],
                token.text(),
                "offsets must address the original text"
            );
        }
    }

    #[test]
    fn java_pins_apostrophe_and_low_quote_splitting() {
        assert_eq!(tokens("IPN'ie"), vec!["IPN", "'", "ie"]);
    }
}
