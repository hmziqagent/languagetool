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
    fsa: Option<std::sync::Arc<og_tagger::morfologik::FsaWordSet>>,
}

impl Dictionary {
    pub fn new() -> Self {
        Self {
            words: HashSet::new(),
            lowercased: HashSet::new(),
            shared: None,
            fsa: None,
        }
    }

    pub fn from_shared_words(
        shared: std::sync::Arc<crate::morfologik_view::MorfologikView>,
    ) -> Self {
        Self {
            words: HashSet::new(),
            lowercased: HashSet::new(),
            shared: Some(shared),
            fsa: None,
        }
    }

    pub fn from_fsa_word_set(fsa: std::sync::Arc<og_tagger::morfologik::FsaWordSet>) -> Self {
        Self {
            words: HashSet::new(),
            lowercased: HashSet::new(),
            shared: None,
            fsa: Some(fsa),
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
        if let Some(view) = self.shared.as_ref()
            && (view.contains(word) || view.contains(&lowered))
        {
            return true;
        }
        if let Some(fsa) = self.fsa.as_ref() {
            return fsa.contains(word) || fsa.contains(&lowered);
        }
        false
    }

    pub fn contains_exact(&self, word: &str) -> bool {
        self.words.contains(word)
            || self.shared.as_ref().is_some_and(|view| view.contains(word))
            || self.fsa.as_ref().is_some_and(|fsa| fsa.contains(word))
    }

    pub fn len(&self) -> usize {
        self.shared.as_ref().map_or(0, |view| view.len())
            + self.fsa.as_ref().map_or(0, |fsa| fsa.word_count() as usize)
            + self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
            && self.shared.as_ref().is_none_or(|view| view.is_empty())
            && self.fsa.as_ref().is_none_or(|fsa| fsa.is_empty())
    }

    pub fn word_list(&self) -> Vec<String> {
        let mut words: Vec<String> = self.words.iter().cloned().collect();
        if let Some(view) = self.shared.as_ref() {
            words.extend(view.word_list());
        }
        if let Some(fsa) = self.fsa.as_ref() {
            words.extend(fsa.word_list());
        }
        words
    }
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}
