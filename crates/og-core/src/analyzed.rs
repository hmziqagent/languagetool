use crate::Token;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedToken {
    token: String,
    lemma: Option<String>,
    pos_tags: Vec<String>,
    start: usize,
    end: usize,
}

impl AnalyzedToken {
    pub fn new(token: impl Into<String>, start: usize, end: usize) -> Self {
        Self {
            token: token.into(),
            lemma: None,
            pos_tags: Vec::new(),
            start,
            end,
        }
    }

    pub fn from_token(token: &Token) -> Self {
        Self {
            token: token.text().to_string(),
            lemma: None,
            pos_tags: Vec::new(),
            start: token.start(),
            end: token.end(),
        }
    }

    pub fn with_lemma(mut self, lemma: impl Into<String>) -> Self {
        self.lemma = Some(lemma.into());
        self
    }

    pub fn with_pos_tags(mut self, tags: Vec<String>) -> Self {
        self.pos_tags = tags;
        self
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn lemma(&self) -> Option<&str> {
        self.lemma.as_deref()
    }

    pub fn pos_tags(&self) -> &[String] {
        &self.pos_tags
    }

    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn has_pos_tag(&self, tag: &str) -> bool {
        self.pos_tags.iter().any(|t| t == tag)
    }

    pub fn has_pos_tag_matching(&self, pattern: &str) -> bool {
        let anchored = if pattern.starts_with('^') && pattern.ends_with('$') {
            pattern.to_string()
        } else {
            format!("^(?:{})$", pattern)
        };
        let re = regex::Regex::new(&anchored).ok();
        match re {
            Some(re) => self.pos_tags.iter().any(|t| re.is_match(t)),
            None => self.pos_tags.iter().any(|t| t == pattern),
        }
    }

    pub fn set_pos_tags(&mut self, tags: Vec<String>) {
        self.pos_tags = tags;
    }

    pub fn set_lemma(&mut self, lemma: Option<String>) {
        self.lemma = lemma;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedTokenReadings {
    token: AnalyzedToken,
    readings: Vec<AnalyzedToken>,
    is_whitespace: bool,
    #[serde(default)]
    chunks: Vec<String>,
    #[serde(default)]
    is_ignored_by_speller: bool,
    #[serde(default)]
    is_immunized: bool,
}

impl AnalyzedTokenReadings {
    pub fn new(token: AnalyzedToken) -> Self {
        let is_whitespace = token.token.chars().all(|c| c.is_whitespace());
        Self {
            token: token.clone(),
            readings: vec![token],
            is_whitespace,
            chunks: Vec::new(),
            is_ignored_by_speller: false,
            is_immunized: false,
        }
    }

    pub fn with_readings(mut self, readings: Vec<AnalyzedToken>) -> Self {
        self.readings = readings;
        self
    }

    pub fn token(&self) -> &AnalyzedToken {
        &self.token
    }

    pub fn into_readings(self) -> Vec<AnalyzedToken> {
        self.readings
    }

    pub fn readings(&self) -> &[AnalyzedToken] {
        &self.readings
    }

    pub fn readings_mut(&mut self) -> &mut Vec<AnalyzedToken> {
        &mut self.readings
    }

    pub fn token_mut(&mut self) -> &mut AnalyzedToken {
        &mut self.token
    }

    pub fn is_whitespace(&self) -> bool {
        self.is_whitespace
    }

    pub fn ignore_spelling(&mut self) {
        self.is_ignored_by_speller = true;
    }

    pub fn is_ignored_by_speller(&self) -> bool {
        self.is_ignored_by_speller
    }

    pub fn immunize(&mut self) {
        self.is_immunized = true;
    }

    pub fn is_immunized(&self) -> bool {
        self.is_immunized
    }

    pub fn first_reading(&self) -> Option<&AnalyzedToken> {
        self.readings.first()
    }

    pub fn has_lemma(&self, lemma: &str) -> bool {
        self.readings
            .iter()
            .any(|r| r.lemma.as_deref() == Some(lemma))
    }

    pub fn has_pos_tag(&self, tag: &str) -> bool {
        self.readings
            .iter()
            .any(|r| r.pos_tags.iter().any(|t| t == tag))
    }

    pub fn has_pos_tag_matching(&self, pattern: &str) -> bool {
        use std::collections::HashMap;
        use std::sync::LazyLock;
        use std::sync::Mutex;

        static REGEX_CACHE: LazyLock<Mutex<HashMap<String, regex::Regex>>> =
            LazyLock::new(|| Mutex::new(HashMap::new()));

        let anchored = if pattern.starts_with('^') && pattern.ends_with('$') {
            pattern.to_string()
        } else {
            format!("^(?:{})$", pattern)
        };

        {
            let cache = REGEX_CACHE.lock().unwrap();
            if let Some(re) = cache.get(&anchored) {
                return self
                    .readings
                    .iter()
                    .any(|r| r.pos_tags.iter().any(|t| re.is_match(t)));
            }
        }

        match regex::Regex::new(&anchored) {
            Ok(re) => {
                let result = self
                    .readings
                    .iter()
                    .any(|r| r.pos_tags.iter().any(|t| re.is_match(t)));
                let mut cache = REGEX_CACHE.lock().unwrap();
                cache.entry(anchored).or_insert(re);
                result
            }
            Err(_) => self
                .readings
                .iter()
                .any(|r| r.pos_tags.iter().any(|t| t == pattern)),
        }
    }

    pub fn has_pos_tag_matching_regex(&self, re: &regex::Regex) -> bool {
        self.readings
            .iter()
            .any(|r| r.pos_tags.iter().any(|t| re.is_match(t)))
    }

    pub fn chunk(&self) -> Option<&str> {
        self.chunks.first().map(String::as_str)
    }

    pub fn chunks(&self) -> &[String] {
        &self.chunks
    }

    pub fn has_chunk(&self, chunk: &str) -> bool {
        self.chunks.iter().any(|c| c == chunk)
    }

    pub fn has_chunk_matching(&self, re: &regex::Regex) -> bool {
        self.chunks.iter().any(|c| re.is_match(c))
    }

    pub fn set_chunk(&mut self, chunk: Option<String>) {
        self.chunks = chunk.into_iter().collect();
    }

    pub fn set_chunks(&mut self, chunks: Vec<String>) {
        self.chunks = chunks;
    }

    pub fn add_sent_end(&mut self) {
        let lemma = self
            .token
            .lemma
            .clone()
            .unwrap_or_else(|| self.token.token.to_string());
        let sent_end_token =
            AnalyzedToken::new(self.token.token.as_str(), self.token.start, self.token.end)
                .with_pos_tags(vec!["SENT_END".to_string()])
                .with_lemma(lemma);
        self.readings.push(sent_end_token);
        if !self.token.pos_tags.contains(&"SENT_END".to_string()) {
            self.token.pos_tags.push("SENT_END".to_string());
        }
    }

    pub fn is_sentence_start(&self) -> bool {
        self.token.pos_tags.iter().any(|t| t == "SENT_START")
            || self
                .readings
                .iter()
                .any(|r| r.pos_tags.iter().any(|t| t == "SENT_START"))
    }

    pub fn is_sentence_end(&self) -> bool {
        self.token.pos_tags.iter().any(|t| t == "SENT_END")
            || self
                .readings
                .iter()
                .any(|r| r.pos_tags.iter().any(|t| t == "SENT_END"))
    }

    pub fn all_pos_tags(&self) -> Vec<&str> {
        let mut tags: Vec<&str> = Vec::new();
        for r in &self.readings {
            for t in &r.pos_tags {
                if !tags.contains(&t.as_str()) {
                    tags.push(t);
                }
            }
        }
        tags
    }

    pub fn all_lemmas(&self) -> Vec<&str> {
        self.readings
            .iter()
            .filter_map(|r| r.lemma.as_deref())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedSentence {
    text: String,
    start: usize,
    end: usize,
    tokens: Vec<AnalyzedTokenReadings>,
}

impl AnalyzedSentence {
    pub fn new(text: impl Into<String>, start: usize, end: usize) -> Self {
        Self {
            text: text.into(),
            start,
            end,
            tokens: Vec::new(),
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

    pub fn tokens(&self) -> &[AnalyzedTokenReadings] {
        &self.tokens
    }

    pub fn set_tokens(&mut self, tokens: Vec<AnalyzedTokenReadings>) {
        self.tokens = tokens;
    }

    pub fn tokens_mut(&mut self) -> &mut Vec<AnalyzedTokenReadings> {
        &mut self.tokens
    }

    pub fn non_whitespace_tokens(&self) -> Vec<&AnalyzedTokenReadings> {
        self.tokens.iter().filter(|t| !t.is_whitespace()).collect()
    }
}
