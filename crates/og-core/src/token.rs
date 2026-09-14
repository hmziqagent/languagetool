use serde::{Deserialize, Serialize};

/// Coarse token classification, mirroring Java LT's `StringTagger` token
/// kinds. Derived lazily from the token text; see [`Token::token_type`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TokenType {
    /// Contains at least one alphabetic character (e.g. `word`, `n't`, `abc123`).
    Word,
    /// All characters are whitespace (e.g. ` `, `\t`, ` \n `).
    Whitespace,
    /// All characters are ASCII punctuation and none is alphabetic (e.g. `.`, `!?`).
    Punctuation,
    /// Anything else (symbols, emoji, e.g. `+`, `👍`, `$`).
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    text: String,
    start: usize,
    end: usize,
    /// Whitespace text that immediately preceded this token in the source
    /// (Java LT's `wsAmount`/`whitespaceBefore` tracking). `None` when the
    /// token starts a sentence or follows another token directly. Not part
    /// of the token's byte span — `start`/`end` still cover only `text`.
    #[serde(default)]
    whitespace_before: Option<String>,
}

impl Token {
    pub fn new(text: impl Into<String>, start: usize, end: usize) -> Self {
        Self {
            text: text.into(),
            start,
            end,
            whitespace_before: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn is_whitespace(&self) -> bool {
        self.text.chars().all(|c| c.is_whitespace())
    }

    pub fn is_punctuation(&self) -> bool {
        self.text.chars().all(|c| c.is_ascii_punctuation())
    }

    pub fn is_word(&self) -> bool {
        self.text.chars().any(|c| c.is_alphabetic())
    }

    /// Coarse classification of this token. Empty tokens are [`TokenType::Whitespace`] (vacuous
    /// `all()` in `is_whitespace()`, matching `token_type_empty_is_whitespace`).
    pub fn token_type(&self) -> TokenType {
        if self.is_whitespace() {
            TokenType::Whitespace
        } else if self.is_word() {
            TokenType::Word
        } else if self.is_punctuation() {
            TokenType::Punctuation
        } else {
            TokenType::Other
        }
    }

    pub fn whitespace_before(&self) -> Option<&str> {
        self.whitespace_before.as_deref()
    }

    pub fn set_whitespace_before(&mut self, ws: Option<String>) {
        self.whitespace_before = ws;
    }

    pub fn with_whitespace_before(mut self, ws: impl Into<String>) -> Self {
        self.whitespace_before = Some(ws.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sentence {
    text: String,
    start: usize,
    end: usize,
    tokens: Vec<Token>,
    /// Whitespace that separated this sentence from the previous one
    /// (Java LT tracks this so rules can detect paragraph breaks).
    /// `None` for the first sentence. Not part of the `start..end` span.
    #[serde(default)]
    whitespace_before: Option<String>,
}

impl Sentence {
    pub fn new(text: impl Into<String>, start: usize, end: usize) -> Self {
        Self {
            text: text.into(),
            start,
            end,
            tokens: Vec::new(),
            whitespace_before: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub fn set_tokens(&mut self, tokens: Vec<Token>) {
        self.tokens = tokens;
    }

    pub fn whitespace_before(&self) -> Option<&str> {
        self.whitespace_before.as_deref()
    }

    pub fn set_whitespace_before(&mut self, ws: Option<String>) {
        self.whitespace_before = ws;
    }

    pub fn with_whitespace_before(mut self, ws: impl Into<String>) -> Self {
        self.whitespace_before = Some(ws.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_type_classifies_word() {
        let token = Token::new("hello", 0, 5);
        assert_eq!(token.token_type(), TokenType::Word);
    }

    #[test]
    fn token_type_classifies_whitespace() {
        let token = Token::new(" \t ", 0, 3);
        assert_eq!(token.token_type(), TokenType::Whitespace);
    }

    #[test]
    fn token_type_classifies_punctuation() {
        assert_eq!(Token::new(".,", 0, 2).token_type(), TokenType::Punctuation);
    }

    #[test]
    fn token_type_classifies_ascii_symbols_as_punctuation() {
        // is_punctuation() accepts any all-ASCII-punctuation text, so
        // math/currency symbols land in Punctuation, like "," or ".".
        assert_eq!(Token::new("$", 0, 1).token_type(), TokenType::Punctuation);
        assert_eq!(Token::new("+", 0, 1).token_type(), TokenType::Punctuation);
    }

    #[test]
    fn token_type_classifies_non_ascii_symbols_as_other() {
        assert_eq!(Token::new("👍", 0, 4).token_type(), TokenType::Other);
        assert_eq!(Token::new("€", 0, 3).token_type(), TokenType::Other);
    }

    #[test]
    fn token_type_empty_is_whitespace() {
        // Vacuous `all()` — empty text classifies as Whitespace,
        // matching the existing is_whitespace() behavior.
        assert_eq!(Token::new("", 0, 0).token_type(), TokenType::Whitespace);
    }

    #[test]
    fn token_type_mixed_alnum_is_word() {
        assert_eq!(Token::new("abc123", 0, 6).token_type(), TokenType::Word);
    }

    #[test]
    fn whitespace_before_defaults_to_none() {
        let token = Token::new("x", 0, 1);
        assert_eq!(token.whitespace_before(), None);
    }

    #[test]
    fn whitespace_before_is_preserved() {
        let token = Token::new("x", 1, 2).with_whitespace_before(" ");
        assert_eq!(token.whitespace_before(), Some(" "));
    }

    #[test]
    fn sentence_whitespace_before_defaults_to_none() {
        let sentence = Sentence::new("Hi.", 0, 3);
        assert_eq!(sentence.whitespace_before(), None);
    }

    #[test]
    fn sentence_whitespace_before_is_preserved() {
        let sentence = Sentence::new("Hi.", 4, 7).with_whitespace_before(". ");
        assert_eq!(sentence.whitespace_before(), Some(". "));
    }

    #[test]
    fn token_deserializes_without_whitespace_before_field() {
        // Backward compatibility with JSON serialized before the field existed.
        let json = r#"{"text":"a","start":0,"end":1}"#;
        let token: Token = serde_json::from_str(json).expect("deserialize legacy Token");
        assert_eq!(token.text(), "a");
        assert_eq!(token.whitespace_before(), None);
    }
}
