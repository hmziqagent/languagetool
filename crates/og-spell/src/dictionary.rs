use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum DictionaryError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid dictionary format: {0}")]
    InvalidFormat(String),
}

#[derive(Clone)]
pub struct Dictionary {
    words: HashSet<String>,
    lowercased: HashSet<String>,
    shared: Option<std::sync::Arc<crate::morfologik_view::MorfologikView>>,
}

impl Dictionary {
    pub fn new() -> Self {
        Self {
            words: HashSet::new(),
            lowercased: HashSet::new(),
            shared: None,
        }
    }

    pub fn from_shared_words(
        shared: std::sync::Arc<crate::morfologik_view::MorfologikView>,
    ) -> Self {
        Self {
            words: HashSet::new(),
            lowercased: HashSet::new(),
            shared: Some(shared),
        }
    }

    pub fn from_file(path: &Path) -> Result<Self, DictionaryError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut dict = Self::new();

        for line in reader.lines() {
            let line = line?;
            let word = line.trim();
            if !word.is_empty() && !word.starts_with('#') {
                dict.words.insert(word.to_string());
                dict.lowercased.insert(word.to_lowercase());
            }
        }

        Ok(dict)
    }

    pub fn from_morfologik(path: &Path) -> Result<Self, DictionaryError> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut dict = Self::new();

        for line in reader.lines() {
            let line = line?;
            let word = line.split('\t').next().unwrap_or("").trim();
            if !word.is_empty() && !word.starts_with('#') {
                dict.words.insert(word.to_string());
                dict.lowercased.insert(word.to_lowercase());
            }
        }

        Ok(dict)
    }

    pub fn from_words(words: &[&str]) -> Self {
        let mut dict = Self::new();
        for word in words {
            dict.add_word(word);
        }
        dict
    }

    pub fn add_word(&mut self, word: &str) {
        self.words.insert(word.to_string());
        let lowered = word.to_lowercase();
        if lowered != word {
            self.lowercased.insert(lowered);
        }
    }

    pub fn contains(&self, word: &str) -> bool {
        let lowered = word.to_lowercase();
        if self.words.contains(word) || self.words.contains(&lowered) {
            return true;
        }
        if self.lowercased.contains(&lowered) {
            return true;
        }
        self.shared
            .as_ref()
            .is_some_and(|view| view.contains(word) || view.contains(&lowered))
    }

    pub fn contains_exact(&self, word: &str) -> bool {
        self.words.contains(word) || self.shared.as_ref().is_some_and(|view| view.contains(word))
    }

    pub fn len(&self) -> usize {
        self.shared.as_ref().map_or(0, |view| view.len()) + self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.shared.as_ref().is_none_or(|view| view.len() == 0)
    }

    pub fn word_list(&self) -> Vec<String> {
        self.words.iter().cloned().collect()
    }
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}
