use og_core::rule::{Rule, TextLevelRule};
use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement,
};
use std::sync::Arc;

pub mod word_tokenizer;

pub use word_tokenizer::GreekWordTokenizer;

#[cfg(test)]
mod example_suite;

const NUMERAL_SUFFIX_PAIRS: [(&str, &str); 12] = [
    ("ος", "ός"),
    ("ου", "ού"),
    ("ο", "ό"),
    ("ον", "όν"),
    ("οι", "οί"),
    ("ων", "ών"),
    ("ους", "ούς"),
    ("η", "ή"),
    ("ης", "ής"),
    ("ην", "ήν"),
    ("ες", "ές"),
    ("α", "ά"),
];

pub struct NumeralStressRule {
    id: String,
    description: String,
    category: Category,
}

impl NumeralStressRule {
    pub fn new() -> Self {
        Self {
            id: "GREEK_ORTHOGRAPHY_NUMERAL_STRESS".to_string(),
            description: "Έλεγχος τονισμού αριθμητικών".to_string(),
            category: Category::new("ORTHOGRAPHY", "Orthography"),
        }
    }

    fn split_numeral(token: &str) -> Option<(&str, &str)> {
        let bytes = token.as_bytes();
        if bytes.is_empty() || !bytes[0].is_ascii_digit() || bytes[0] == b'0' {
            return None;
        }
        let mut i = 1usize;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        let suffix = &token[i..];
        if suffix.is_empty() {
            return None;
        }
        let known = NUMERAL_SUFFIX_PAIRS
            .iter()
            .any(|(unstressed, stressed)| suffix == *unstressed || suffix == *stressed);
        if known {
            Some((&token[..i], suffix))
        } else {
            None
        }
    }

    fn number_needs_stress(number: &str) -> bool {
        let b = number.as_bytes();
        b.len() >= 2
            && b[b.len() - 1] == b'0'
            && (b[b.len() - 2] == b'0' || (b'2'..=b'9').contains(&b[b.len() - 2]))
    }

    fn suffix_has_stress(suffix: &str) -> bool {
        NUMERAL_SUFFIX_PAIRS
            .iter()
            .any(|(_, stressed)| suffix == *stressed)
    }

    fn mapped_suffix(suffix: &str) -> &str {
        for (unstressed, stressed) in NUMERAL_SUFFIX_PAIRS {
            if suffix == stressed {
                return unstressed;
            }
            if suffix == unstressed {
                return stressed;
            }
        }
        suffix
    }
}

impl Default for NumeralStressRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for NumeralStressRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Misspelling
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        for token in sentence.non_whitespace_tokens() {
            let Some((number, suffix)) = Self::split_numeral(token.token().token()) else {
                continue;
            };
            let needs_stress = Self::number_needs_stress(number);
            let has_stress = Self::suffix_has_stress(suffix);
            if needs_stress == has_stress {
                continue;
            }
            let suggestion = format!("{}{}", number, Self::mapped_suffix(suffix));
            let start = token.token().start();
            let end = token.token().end();
            let (context_text, ctx_start, ctx_end) = local_context(sentence, start, end, 20);
            let rule = RuleMatchRule::new(&self.id, &self.description)
                .with_category(self.category.clone())
                .with_issue_type(IssueType::Misspelling.as_str());
            matches.push(
                RuleMatch::new(
                    format!("<suggestion>{}</suggestion>", suggestion),
                    start,
                    end - start,
                    rule,
                    RuleMatchContext::new(context_text, ctx_start, ctx_end - ctx_start),
                )
                .with_short_message("Πρόβλημα ορθογραφίας".to_string())
                .with_replacements(vec![SuggestedReplacement::new(&suggestion)])
                .with_sentence(sentence.text().to_string()),
            );
        }
        matches
    }
}

fn local_context(
    sentence: &AnalyzedSentence,
    start: usize,
    end: usize,
    pad: usize,
) -> (String, usize, usize) {
    let text = sentence.text();
    let text_len = text.len();
    let mut ctx_start = start.saturating_sub(sentence.start()).saturating_sub(pad);
    while ctx_start > 0 && !text.is_char_boundary(ctx_start) {
        ctx_start -= 1;
    }
    let mut ctx_end = ((end.saturating_sub(sentence.start())) + pad).min(text_len);
    while ctx_end < text_len && !text.is_char_boundary(ctx_end) {
        ctx_end += 1;
    }
    (
        text[ctx_start..ctx_end].to_string(),
        sentence.start() + ctx_start,
        sentence.start() + ctx_end,
    )
}

fn eq_ignore_case(a: &str, b: &str) -> bool {
    let mut ai = a.chars().flat_map(char::to_uppercase);
    let mut bi = b.chars().flat_map(char::to_uppercase);
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x == y => {}
            _ => return false,
        }
    }
}

fn is_emoji_token(token: &str) -> bool {
    token.chars().any(|c| (c as u32) > 0xFFFF)
        && !token.chars().all(|c| {
            c.is_alphabetic()
                || c.is_numeric()
                || c.is_whitespace()
                || c.is_ascii_punctuation()
                || matches!(c, '\u{00A1}'..='\u{00BF}' | '\u{2010}'..='\u{2027}')
        })
}

fn is_repeat_word(token: &str) -> bool {
    if is_emoji_token(token) {
        return false;
    }
    if !token.is_empty() && token.chars().all(|c| c.is_numeric() || c == ' ') {
        return false;
    }
    if token.chars().count() == 1 {
        return token.chars().next().is_some_and(char::is_alphabetic);
    }
    true
}

const IGNORED_NAME_REPEATS: [&str; 9] = [
    "Phi", "Li", "Xiao", "Duran", "Wagga", "Abdullah", "Nwe", "Pago", "Cao",
];

pub struct GreekWordRepeatRule {
    id: String,
    description: String,
    category: Category,
}

impl GreekWordRepeatRule {
    pub fn new() -> Self {
        Self {
            id: "WORD_REPEAT_RULE".to_string(),
            description: "Επανάληψη λέξης (π.χ. 'και και')".to_string(),
            category: Category::new("MISC", "Διάφορα"),
        }
    }

    fn is_ignored_name_repeat(prev: &str, curr: &str) -> bool {
        IGNORED_NAME_REPEATS
            .iter()
            .any(|word| prev == *word && curr == *word)
    }
}

impl Default for GreekWordRepeatRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for GreekWordRepeatRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Redundancy
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        let mut matches = Vec::new();
        let mut prev_token = "";
        let mut prev_start = 0usize;
        for token in &tokens {
            let text = token.token().token();
            if token.is_immunized() {
                prev_token = "";
                prev_start = token.token().start();
                continue;
            }
            if is_repeat_word(text)
                && !prev_token.is_empty()
                && eq_ignore_case(prev_token, text)
                && !Self::is_ignored_name_repeat(prev_token, text)
            {
                let start = prev_start;
                let end = token.token().start() + prev_token.len();
                let (context_text, ctx_start, ctx_end) = local_context(sentence, start, end, 20);
                let rule = RuleMatchRule::new(&self.id, &self.description)
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Redundancy.as_str());
                matches.push(
                    RuleMatch::new(
                        "Πιθανό λάθος: επαναλαμβάνετε μία λέξη".to_string(),
                        start,
                        end - start,
                        rule,
                        RuleMatchContext::new(context_text, ctx_start, ctx_end - ctx_start),
                    )
                    .with_short_message("Επανάληψη λέξης".to_string())
                    .with_replacements(vec![SuggestedReplacement::new(prev_token)])
                    .with_sentence(sentence.text().to_string()),
                );
            }
            prev_token = text;
            prev_start = token.token().start();
        }
        matches
    }
}

pub fn replace_homonyms_rule() -> crate::pt::simple_replace2::SimpleReplace2Rule {
    let mut rule = crate::pt::simple_replace2::SimpleReplace2Rule::new(
        "GREEK_HOMONYMS_REPLACE",
        "Έλεγχος για λανθασμένη χρήση ομόηχων λέξεων σε μια πρόταση",
        "Λανθασμένη χρήση της λέξης",
        "Μήπως εννοούσατε $suggestions",
    )
    .with_category(og_core::Category::new("MISC", "Διάφορα"));
    rule.load_data(include_str!(
        "../../../../languagetool/languagetool-language-modules/el/src/main/resources/org/languagetool/rules/el/replace.txt"
    ));
    rule
}

pub struct GreekSpecificCaseRule {
    inner: crate::specific_case::SpecificCaseRule,
}

pub fn greek_specific_case_rule() -> GreekSpecificCaseRule {
    GreekSpecificCaseRule {
        inner: crate::specific_case::SpecificCaseRule::from_data(
            "EL_SPECIFIC_CASE",
            "Ελέγχει αν κάποιες λέξεις χρειάζονται κεφαλαιο το πρώτο τους γράμμα",
            "Χρήση κεφαλαίων",
            "Οι λέξεις της συγκεκριμένης έκφρασης χρείαζεται να ξεκινούν με κεφαλαία γράμματα.",
            "Η συγκεκριμένη έκφραση γράφεται σύμφωνα με την προτεινόμενη κεφαλαιοποίηση.",
            include_str!(
                "../../../../languagetool/languagetool-language-modules/el/src/main/resources/org/languagetool/resource/el/specific_case.txt"
            ),
        ),
    }
}

impl Rule for GreekSpecificCaseRule {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn is_default_on(&self) -> bool {
        self.inner.is_default_on()
    }

    fn category(&self) -> Category {
        self.inner.category()
    }

    fn issue_type(&self) -> IssueType {
        self.inner.issue_type()
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        self.inner
            .match_sentence(sentence)
            .into_iter()
            .map(|m| m.with_short_message("Ειδική κεφαλαιοποίηση".to_string()))
            .collect()
    }
}

pub fn greek_redundancy_rule() -> crate::pt::simple_replace2::SimpleReplace2Rule {
    let mut rule = crate::pt::simple_replace2::SimpleReplace2Rule::new(
        "EL_REDUNDANCY_REPLACE",
        "Έλεγχος για χρήση πλεονασμού σε μια πρόταση.",
        "Πλεονασμός",
        "'$match' είναι πλεονασμός. Γενικά, είναι προτιμότερο το: $suggestions",
    )
    .with_category(og_core::Category::new("REDUNDANCY", "Πλεονασμός"))
    .with_issue_type(og_core::IssueType::Style);
    rule.load_data(include_str!(
        "../../../../languagetool/languagetool-language-modules/el/src/main/resources/org/languagetool/rules/el/redundancies.txt"
    ));
    rule
}

const ADD_ADVERBS: [&str; 5] = [
    "Επίσης",
    "Επιπρόσθετα",
    "Ακόμη",
    "Επιπλέον",
    "Συμπληρωματικά",
];
const CONTRAST_ADVERBS: [&str; 4] = ["Αντίθετα", "Ωστόσο", "Εντούτοις", "Εξάλλου"];
const EXPLAIN_ADVERBS: [&str; 4] = ["Δηλαδή", "Ειδικότερα", "Ειδικά", "Συγκεκριμένα"];

const REPEAT_BEGINNING_EXCEPTIONS: [&str; 8] = ["Ο", "Η", "Το", "Οι", "Τα", "Ένας", "Μία", "Ένα"];

const BASE_BEGINNING_EXCEPTIONS: [&str; 8] = [":", "–", "-", "✔️", "➡️", "—", "⭐️", "⚠️"];

pub struct GreekWordRepeatBeginningRule {
    id: String,
    description: String,
    category: Category,
}

impl GreekWordRepeatBeginningRule {
    pub fn new() -> Self {
        Self {
            id: "GREEK_WORD_REPEAT_BEGINNING_RULE".to_string(),
            description: "Συνεχόμενες προτάσεις που ξεκινάνε με την ίδια λέξη".to_string(),
            category: Category::new("REPETITIONS_STYLE", "Repetitions (Style)"),
        }
    }

    fn is_exception(token: &str) -> bool {
        REPEAT_BEGINNING_EXCEPTIONS.contains(&token) || BASE_BEGINNING_EXCEPTIONS.contains(&token)
    }

    fn is_adverb(token: &str) -> bool {
        ADD_ADVERBS.contains(&token)
            || CONTRAST_ADVERBS.contains(&token)
            || EXPLAIN_ADVERBS.contains(&token)
    }

    fn suggestions_for(token: &str) -> Vec<String> {
        let set: &[&str] = if ADD_ADVERBS.contains(&token) {
            &ADD_ADVERBS
        } else if CONTRAST_ADVERBS.contains(&token) {
            &CONTRAST_ADVERBS
        } else if EXPLAIN_ADVERBS.contains(&token) {
            &EXPLAIN_ADVERBS
        } else {
            &[]
        };
        set.iter()
            .filter(|adv| **adv != token)
            .map(|adv| adv.to_string())
            .collect()
    }
}

impl Default for GreekWordRepeatBeginningRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for GreekWordRepeatBeginningRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let mut last_token = String::new();
        let mut before_last_token = String::new();
        let mut prev_sentence_ends_with_punct = false;
        for sentence in sentences {
            let tokens = sentence.non_whitespace_tokens();
            let mut token = "";
            if tokens.len() > 1 {
                let analyzed = &tokens[1];
                token = analyzed.token().token();
                if tokens.len() > 3 {
                    let is_word = token.chars().count() != 1
                        || token.chars().next().is_some_and(char::is_alphabetic);
                    if is_word
                        && last_token == token
                        && !Self::is_exception(token)
                        && !Self::is_exception(tokens[2].token().token())
                        && !Self::is_exception(tokens[3].token().token())
                        && prev_sentence_ends_with_punct
                    {
                        let short_msg = if Self::is_adverb(token) {
                            "Δύο συνεχόμενες προτάσεις ξεκινάνε με το ίδιο επίρρημα."
                        } else if before_last_token == token {
                            "Τρεις συνεχόμενες προτάσεις ξεκινάνε με την ίδια λέξη."
                        } else {
                            ""
                        };
                        if !short_msg.is_empty() {
                            let start = analyzed.token().start();
                            let end = analyzed.token().end();
                            let local_start = start.saturating_sub(sentence.start());
                            let local_end = end.saturating_sub(sentence.start()).min(text.len());
                            let context_text = sentence.text()[local_start..local_end].to_string();
                            let rule = RuleMatchRule::new(&self.id, &self.description)
                                .with_category(self.category.clone())
                                .with_issue_type(IssueType::Style.as_str());
                            let mut rule_match = RuleMatch::new(
                                format!("{short_msg} Αλλάξτε την σειρά των λέξεων στην πρόταση ή συμβουλευτείτε έναν θησαυρό για να βρείτε μία συνώνυμη λέξη."),
                                start,
                                end - start,
                                rule,
                                RuleMatchContext::new(context_text, start, end - start),
                            )
                            .with_short_message(short_msg.to_string());
                            let suggestions = Self::suggestions_for(token);
                            if !suggestions.is_empty() {
                                rule_match = rule_match.with_replacements(
                                    suggestions.iter().map(SuggestedReplacement::new).collect(),
                                );
                            }
                            matches.push(rule_match);
                        }
                    }
                }
            }
            before_last_token = last_token.clone();
            last_token = token.to_string();
            let trimmed = sentence.text().trim();
            prev_sentence_ends_with_punct =
                trimmed.len() >= 2 && trimmed.ends_with(['.', '?', '!']);
        }
        matches
    }
}

pub struct GreekNativeRules {
    pub rules: Vec<Arc<dyn Rule>>,
    pub text_level_rules: Vec<Arc<dyn TextLevelRule>>,
}

pub fn greek_native_rules() -> GreekNativeRules {
    GreekNativeRules {
        rules: vec![
            Arc::new(GreekWordRepeatRule::new()),
            Arc::new(replace_homonyms_rule()),
            Arc::new(greek_specific_case_rule()),
            Arc::new(NumeralStressRule::new()),
            Arc::new(greek_redundancy_rule()),
        ],
        text_level_rules: vec![Arc::new(GreekWordRepeatBeginningRule::new())],
    }
}
