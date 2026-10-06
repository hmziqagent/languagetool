use super::utils::{is_all_uppercase, starts_with_uppercase};
use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchRule, SuggestedReplacement,
    rule::Rule,
};
use std::collections::HashMap;
use std::sync::LazyLock;

const SPECIFIC_CASE_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/resource/ga/specific_case.txt"
);

struct PhraseBook {
    proper_spelling: HashMap<String, String>,
    max_len: usize,
}

static PHRASES: LazyLock<PhraseBook> = LazyLock::new(|| {
    let mut proper_spelling = HashMap::new();
    let mut max_len = 0usize;
    for line in SPECIFIC_CASE_TXT.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let phrase = line.trim();
        max_len = max_len.max(phrase.split(' ').count());
        proper_spelling.insert(phrase.to_lowercase(), phrase.to_string());
    }
    PhraseBook {
        proper_spelling,
        max_len,
    }
});

fn all_words_uppercase(s: &str) -> bool {
    s.split(' ').all(starts_with_uppercase)
}

pub struct IrishSpecificCaseRule {
    description: String,
    category: Category,
}

impl IrishSpecificCaseRule {
    pub fn new() -> Self {
        Self {
            description: "Checks upper/lower case spelling of some proper nouns".to_string(),
            category: Category::new("CASING", "Capitalization"),
        }
    }
}

impl Default for IrishSpecificCaseRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for IrishSpecificCaseRule {
    fn id(&self) -> &str {
        "GA_SPECIFIC_CASE"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Misspelling
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        let mut matches = Vec::new();
        for i in 0..tokens.len() {
            let mut l: Vec<&str> = Vec::new();
            let mut j = 0usize;
            while l.len() < PHRASES.max_len && i + j < tokens.len() {
                l.push(tokens[i + j].token().token());
                j += 1;
                let phrase = l.join(" ");
                let Some(proper) = PHRASES.proper_spelling.get(&phrase.to_lowercase()) else {
                    continue;
                };
                if is_all_uppercase(&phrase) || phrase == *proper {
                    continue;
                }
                if i > 0 && tokens[i - 1].is_sentence_start() && !starts_with_uppercase(proper) {
                    continue;
                }
                let message = if all_words_uppercase(proper) {
                    "Más ainmfhocal dílis é, scríobh é i gceannlitreacha."
                } else {
                    "If the term is a proper noun, use the suggested capitalization."
                };
                let start = tokens[i].token().start();
                let end = start + phrase.len();
                let rm_rule = RuleMatchRule::new(self.id(), &self.description)
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Misspelling.as_str());
                matches.push(
                    RuleMatch::new(
                        message,
                        start,
                        end - start,
                        rm_rule,
                        super::context_window(sentence, start, end, 40),
                    )
                    .with_replacements(vec![SuggestedReplacement::new(proper)])
                    .with_sentence(sentence.text().to_string()),
                );
            }
        }
        matches
    }
}
