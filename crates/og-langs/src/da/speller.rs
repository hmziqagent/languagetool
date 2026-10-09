use og_core::rule::Rule;
use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchKind,
    RuleMatchRule, SuggestedReplacement,
};
use og_spell::Dictionary;
use og_spell::hunspell::HunspellChecker;
use og_spell::suggestion::SuggestionEngine;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

pub const HUNSPELL_RULE_DA: &str = "HUNSPELL_RULE";

pub struct DanishSpellerRule {
    hunspell: Option<Arc<HunspellChecker>>,
    dictionary: Arc<Dictionary>,
    suggestion_engine: SuggestionEngine,
    ignore_words: HashSet<String>,
    category: Category,
}

pub fn da_resource_dir() -> Option<std::path::PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("da/src/main/resources/org/languagetool/resource/da"))
}

fn shared_hunspell() -> Option<Arc<HunspellChecker>> {
    static CACHE: og_core::releasable::Releasable<HunspellChecker> =
        og_core::releasable::Releasable::new();
    og_core::releasable::shared_get_or_build(&CACHE, || {
        let dir = da_resource_dir()?;
        HunspellChecker::load(
            &dir.join("hunspell/da_DK.aff"),
            &dir.join("hunspell/da_DK.dic"),
        )
        .map(Arc::new)
    })
}

fn speller_dictionary(resource_dir: &Path) -> Dictionary {
    let mut dict = match og_tagger::generic_tagger::shared_dictionary(resource_dir, "danish") {
        Some(shared) => Dictionary::from_shared_words(Arc::new(
            og_spell::morfologik_view::MorfologikView::new(shared),
        )),
        None => Dictionary::new(),
    };
    if let Ok(extra) = std::fs::read_to_string(resource_dir.join("hunspell/spelling.txt")) {
        for line in extra.lines() {
            let word = line.trim();
            if !word.is_empty() && !word.starts_with('#') {
                dict.add_word(word);
            }
        }
    }
    dict
}

fn da_ignore_words(resource_dir: &Path) -> HashSet<String> {
    let mut ignore = HashSet::new();
    if let Ok(words) = std::fs::read_to_string(resource_dir.join("hunspell/ignore.txt")) {
        for line in words.lines() {
            let word = line.trim();
            if !word.is_empty() && !word.starts_with('#') {
                ignore.insert(word.to_lowercase());
            }
        }
    }
    ignore
}

impl DanishSpellerRule {
    pub fn new() -> Self {
        let resource_dir = da_resource_dir().unwrap_or_default();
        let dictionary = Arc::new(speller_dictionary(&resource_dir));
        Self {
            hunspell: shared_hunspell(),
            suggestion_engine: SuggestionEngine::new(&dictionary),
            ignore_words: da_ignore_words(&resource_dir),
            dictionary,
            category: Category::new("TYPOS", "Possible Typo")
                .with_description("Possible spelling mistake"),
        }
    }

    pub fn dictionary_size(&self) -> usize {
        self.dictionary.len()
    }

    fn is_known_word(&self, word: &str) -> bool {
        if !word.chars().any(|c| c.is_alphabetic()) {
            return true;
        }
        if self.ignore_words.contains(word) || self.ignore_words.contains(&word.to_lowercase()) {
            return true;
        }
        if word.chars().count() == 1 {
            return true;
        }
        if word.contains('/')
            || word.contains('@')
            || word.starts_with("http")
            || word.starts_with("www.")
        {
            return true;
        }
        if word.chars().any(|c| c.is_ascii_digit()) {
            return true;
        }
        let lowercase = word.to_lowercase();
        if self.dictionary.contains(word) || self.dictionary.contains(&lowercase) {
            return true;
        }
        match &self.hunspell {
            Some(hunspell) => hunspell.spell(word) || hunspell.spell(&lowercase),
            None => false,
        }
    }
}

impl Default for DanishSpellerRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for DanishSpellerRule {
    fn id(&self) -> &str {
        HUNSPELL_RULE_DA
    }

    fn description(&self) -> &str {
        "Possible spelling mistake"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Misspelling
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        for token_readings in sentence.tokens() {
            if token_readings.is_whitespace() {
                continue;
            }
            let text = token_readings.token().token();
            if text == "<S>" || token_readings.is_sentence_start() {
                continue;
            }
            if token_readings.is_ignored_by_speller() || token_readings.is_immunized() {
                continue;
            }
            if !text.chars().any(|c| c.is_alphabetic()) {
                continue;
            }
            if text.contains('.') && text.chars().filter(|c| *c == '.').count() > 1 {
                continue;
            }
            if self.is_known_word(text) {
                continue;
            }
            let start = token_readings.token().start();
            let end = token_readings.token().end();
            let suggestions = self.suggestion_engine.suggest(text, 5);
            let replacements: Vec<SuggestedReplacement> = suggestions
                .into_iter()
                .map(SuggestedReplacement::new)
                .collect();
            let rule = RuleMatchRule::new(self.id(), self.description())
                .with_category(self.category.clone())
                .with_issue_type(IssueType::Misspelling.as_str());
            matches.push(
                RuleMatch::new(
                    format!("Possible spelling mistake found: '{text}'"),
                    start,
                    end - start,
                    rule,
                    RuleMatchContext::new(sentence.text().to_string(), 0, sentence.text().len()),
                )
                .with_sentence(sentence.text().to_string())
                .with_replacements(replacements)
                .with_match_type(RuleMatchKind::UnknownWord),
            );
        }
        matches
    }
}
