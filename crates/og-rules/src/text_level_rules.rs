use og_core::rule::TextLevelRule;
use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchContext,
    RuleMatchRule, SuggestedReplacement,
};

pub struct MultipleWhitespaceRule {
    id: String,
    category: Category,
}

impl MultipleWhitespaceRule {
    pub fn new() -> Self {
        Self {
            id: "WHITESPACE_RULE".to_string(),
            category: Category::new("TYPOGRAPHY", "Typography"),
        }
    }
}

impl Default for MultipleWhitespaceRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for MultipleWhitespaceRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Whitespace repetition"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Whitespace
    }

    fn match_text(&self, text: &str, _sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let bytes = text.as_bytes();
        let mut i = 0;

        while i < text.len() {
            if bytes[i] == b' ' {
                let start = i;
                while i < text.len() && bytes[i] == b' ' {
                    i += 1;
                }
                let end = i;
                if end - start > 1 {
                    let context_start = start.saturating_sub(30);
                    let context_end = std::cmp::min(end + 30, text.len());
                    let context_text = text[context_start..context_end].to_string();

                    let rule = RuleMatchRule::new(&self.id, self.description())
                        .with_category(self.category.clone())
                        .with_issue_type(IssueType::Whitespace.as_str());

                    matches.push(
                        RuleMatch::new(
                            "There is duplicated whitespace.".to_string(),
                            start,
                            end - start,
                            rule,
                            RuleMatchContext::new(
                                context_text,
                                context_start,
                                context_end - context_start,
                            ),
                        )
                        .with_replacements(vec![SuggestedReplacement::new(" ")]),
                    );
                }
            } else {
                i += 1;
            }
        }

        matches
    }
}

pub struct SentenceWhitespaceRule {
    id: String,
    category: Category,
}

impl SentenceWhitespaceRule {
    pub fn new() -> Self {
        Self {
            id: "SENTENCE_WHITESPACE".to_string(),
            category: Category::new("TYPOGRAPHY", "Typography"),
        }
    }
}

impl Default for SentenceWhitespaceRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for SentenceWhitespaceRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Missing space between sentences"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Whitespace
    }

    fn match_text(&self, text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        if sentences.len() < 2 {
            return matches;
        }

        let mut pos = 0usize;
        for i in 1..sentences.len() {
            let prev_end = pos + sentences[i - 1].text().len();
            let curr_text = sentences[i].text();

            let prev_sentence_text = sentences[i - 1].text();
            let ends_with_whitespace = prev_sentence_text
                .chars()
                .last()
                .map(|c| c.is_whitespace())
                .unwrap_or(false);

            if !ends_with_whitespace && !curr_text.starts_with(|c: char| c.is_whitespace()) {
                let start = prev_end;
                let first_char_len = curr_text.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
                let end = prev_end + first_char_len;
                let context_start = start.saturating_sub(30);
                let context_end = std::cmp::min(end + 30, text.len());
                let context_text = text[context_start..context_end].to_string();

                let first_word: String = curr_text
                    .chars()
                    .take_while(|c| !c.is_whitespace())
                    .collect();
                let suggestion = format!(" {}", first_word);

                let rule = RuleMatchRule::new(&self.id, self.description())
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Whitespace.as_str());

                matches.push(
                    RuleMatch::new(
                        "Add a space between sentences.".to_string(),
                        start,
                        end - start,
                        rule,
                        RuleMatchContext::new(
                            context_text,
                            context_start,
                            context_end - context_start,
                        ),
                    )
                    .with_sentence(curr_text.to_string())
                    .with_replacements(vec![SuggestedReplacement::new(suggestion)]),
                );
            }

            pos += sentences[i - 1].text().len();
        }

        matches
    }
}

pub struct GenericUnpairedBracketsRule {
    id: String,
    category: Category,
    start_symbols: Vec<String>,
    end_symbols: Vec<String>,
}

impl GenericUnpairedBracketsRule {
    pub fn new() -> Self {
        Self {
            id: "UNPAIRED_BRACKETS".to_string(),
            category: Category::new("PUNCTUATION", "Punctuation"),
            start_symbols: vec!["[".to_string(), "(".to_string(), "{".to_string()],
            end_symbols: vec!["]".to_string(), ")".to_string(), "}".to_string()],
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_symbols(mut self, start: Vec<&str>, end: Vec<&str>) -> Self {
        assert_eq!(
            start.len(),
            end.len(),
            "Start and end symbol lists must be equal length"
        );
        self.start_symbols = start.iter().map(|s| s.to_string()).collect();
        self.end_symbols = end.iter().map(|s| s.to_string()).collect();
        self
    }
}

impl Default for GenericUnpairedBracketsRule {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct SymbolEntry {
    symbol: String,
    is_opening: bool,
    start_pos: usize,
    #[allow(dead_code)]
    sentence_idx: usize,
}

impl TextLevelRule for GenericUnpairedBracketsRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Unpaired brackets"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Other("other".to_string())
    }

    fn match_text(&self, text: &str, _sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let mut stack: Vec<SymbolEntry> = Vec::new();

        let _all_symbols: Vec<&str> = self
            .start_symbols
            .iter()
            .chain(self.end_symbols.iter())
            .map(|s| s.as_str())
            .collect();

        let chars = text.char_indices().peekable();
        for (byte_idx, ch) in chars {
            let ch_str = ch.to_string();

            if ch == ')' || ch == '(' {
                if byte_idx >= 1 {
                    let prev = &text[byte_idx - 1..byte_idx];
                    if prev == ":" || prev == ";" {
                        continue;
                    }
                }
                if byte_idx >= 2 {
                    let prev2 = &text[byte_idx - 2..byte_idx - 1];
                    let prev1 = &text[byte_idx - 1..byte_idx];
                    if prev1 == "-" && (prev2 == ":" || prev2 == ";") {
                        continue;
                    }
                }
            }

            for j in 0..self.start_symbols.len() {
                if ch_str == self.start_symbols[j] {
                    stack.push(SymbolEntry {
                        symbol: self.start_symbols[j].clone(),
                        is_opening: true,
                        start_pos: byte_idx,
                        sentence_idx: 0,
                    });
                    break;
                }
                if ch_str == self.end_symbols[j] {
                    if let Some(idx) = stack
                        .iter()
                        .rposition(|s| s.is_opening && s.symbol == self.start_symbols[j])
                    {
                        stack.remove(idx);
                    } else {
                        stack.push(SymbolEntry {
                            symbol: self.end_symbols[j].clone(),
                            is_opening: false,
                            start_pos: byte_idx,
                            sentence_idx: 0,
                        });
                    }
                    break;
                }
            }
        }

        for entry in &stack {
            let start = entry.start_pos;
            let end = start + entry.symbol.len();
            let context_start = start.saturating_sub(30);
            let context_end = std::cmp::min(end + 30, text.len());
            let context_text = text[context_start..context_end].to_string();

            let other = if entry.is_opening {
                let idx = self
                    .start_symbols
                    .iter()
                    .position(|s| s == &entry.symbol)
                    .unwrap();
                self.end_symbols[idx].clone()
            } else {
                let idx = self
                    .end_symbols
                    .iter()
                    .position(|s| s == &entry.symbol)
                    .unwrap();
                self.start_symbols[idx].clone()
            };

            let rule = RuleMatchRule::new(&self.id, self.description())
                .with_category(self.category.clone())
                .with_issue_type(IssueType::Other("other".to_string()).as_str());

            matches.push(
                RuleMatch::new(
                    format!("Unpaired '{}' (missing '{}')", entry.symbol, other),
                    start,
                    end - start,
                    rule,
                    RuleMatchContext::new(context_text, context_start, context_end - context_start),
                )
                .with_sentence("".to_string())
                .with_replacements(vec![SuggestedReplacement::new(entry.symbol.clone())]),
            );
        }

        matches
    }
}

pub struct GenericUnpairedQuotesRule {
    id: String,
    category: Category,
    quote_symbols: Vec<(String, String)>,
}

impl GenericUnpairedQuotesRule {
    pub fn new() -> Self {
        Self {
            id: "UNPAIRED_QUOTES".to_string(),
            category: Category::new("PUNCTUATION", "Punctuation"),
            quote_symbols: vec![
                ("\u{201C}".to_string(), "\u{201D}".to_string()),
                ("\u{2018}".to_string(), "\u{2019}".to_string()),
                ("\"".to_string(), "\"".to_string()),
            ],
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_quote_symbols(mut self, symbols: Vec<(String, String)>) -> Self {
        self.quote_symbols = symbols;
        self
    }

    pub fn english() -> Self {
        Self::new()
            .with_id("EN_UNPAIRED_QUOTES")
            .with_quote_symbols(vec![
                ("\u{201C}".to_string(), "\u{201D}".to_string()),
                ("\"".to_string(), "\"".to_string()),
                ("'".to_string(), "'".to_string()),
                ("\u{2018}".to_string(), "\u{2019}".to_string()),
            ])
    }
}

impl Default for GenericUnpairedQuotesRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for GenericUnpairedQuotesRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Unpaired quotes"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Other("other".to_string())
    }

    fn match_text(&self, text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let mut open_counts: Vec<usize> = vec![0; self.quote_symbols.len()];
        let mut open_positions: Vec<Vec<usize>> = vec![Vec::new(); self.quote_symbols.len()];
        let mut pos = 0usize;

        for sentence in sentences {
            let tokens = sentence.non_whitespace_tokens();

            for token_reading in &tokens {
                let token_text = token_reading.token().token();
                let token_start = pos + token_reading.token().start();

                for (qi, (open_q, close_q)) in self.quote_symbols.iter().enumerate() {
                    let apostrophe_guarded = open_q == "'"
                        && (token_reading.has_pos_tag("POS")
                            || token_reading.has_pos_tag("NNP")
                            || token_reading.has_pos_tag("_apostrophe_contraction_"));
                    if apostrophe_guarded {
                        continue;
                    }
                    if open_q == close_q {
                        if token_text == open_q.as_str() {
                            if open_counts[qi].is_multiple_of(2) {
                                open_positions[qi].push(token_start);
                            } else {
                                open_positions[qi].pop();
                            }
                            open_counts[qi] += 1;
                        }
                    } else {
                        if token_text == open_q.as_str() {
                            open_positions[qi].push(token_start);
                            open_counts[qi] += 1;
                        }
                        if token_text == close_q.as_str() {
                            if open_counts[qi] > 0 {
                                open_positions[qi].pop();
                                open_counts[qi] -= 1;
                            } else {
                                let start = token_start;
                                let end = start + close_q.len();
                                let context_start = start.saturating_sub(30);
                                let context_end = std::cmp::min(end + 30, text.len());
                                let context_text = text[context_start..context_end].to_string();

                                let rule = RuleMatchRule::new(&self.id, self.description())
                                    .with_category(self.category.clone())
                                    .with_issue_type(
                                        IssueType::Other("other".to_string()).as_str(),
                                    );

                                matches.push(
                                    RuleMatch::new(
                                        format!("Unpaired closing quote '{}'", close_q),
                                        start,
                                        end - start,
                                        rule,
                                        RuleMatchContext::new(
                                            context_text,
                                            context_start,
                                            context_end - context_start,
                                        ),
                                    )
                                    .with_sentence(sentence.text().to_string()),
                                );
                            }
                        }
                    }
                }
            }
            pos += sentence.text().len();
        }

        for (qi, positions) in open_positions.iter().enumerate() {
            let (open_q, _) = &self.quote_symbols[qi];
            for &start_pos in positions {
                let end = start_pos + open_q.len();
                let context_start = start_pos.saturating_sub(30);
                let context_end = std::cmp::min(end + 30, text.len());
                let context_text = text[context_start..context_end].to_string();

                let rule = RuleMatchRule::new(&self.id, self.description())
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Other("other".to_string()).as_str());

                matches.push(
                    RuleMatch::new(
                        format!("Unpaired opening quote '{}'", open_q),
                        start_pos,
                        end - start_pos,
                        rule,
                        RuleMatchContext::new(
                            context_text,
                            context_start,
                            context_end - context_start,
                        ),
                    )
                    .with_sentence("".to_string()),
                );
            }
        }

        matches
    }
}

pub struct LongSentenceRule {
    id: String,
    category: Category,
    max_words: usize,
}

impl LongSentenceRule {
    pub fn new(max_words: usize) -> Self {
        Self {
            id: "TOO_LONG_SENTENCE".to_string(),
            category: Category::new("STYLE", "Style"),
            max_words,
        }
    }
}

impl Default for LongSentenceRule {
    fn default() -> Self {
        Self::new(40)
    }
}

impl TextLevelRule for LongSentenceRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn is_picky(&self) -> bool {
        true
    }

    fn description(&self) -> &str {
        "Sentence is too long"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn match_text(&self, text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let mut pos = 0usize;

        for sentence in sentences {
            let tokens = sentence.non_whitespace_tokens();
            let word_count = tokens
                .iter()
                .filter(|t| t.token().token().chars().any(|c| c.is_alphabetic()))
                .count();

            if word_count > self.max_words {
                let first_word = tokens
                    .iter()
                    .find(|t| t.token().token().chars().any(|c| c.is_alphabetic()));
                let last_word = tokens
                    .iter()
                    .rev()
                    .find(|t| t.token().token().chars().any(|c| c.is_alphabetic()));

                if let (Some(first), Some(last)) = (first_word, last_word) {
                    let start = pos + first.token().start();
                    let end = pos + last.token().end();
                    let context_start = start.saturating_sub(30);
                    let context_end = std::cmp::min(end + 30, text.len());
                    let context_text = text[context_start..context_end].to_string();

                    let rule = RuleMatchRule::new(&self.id, self.description())
                        .with_picky(true)
                        .with_category(self.category.clone())
                        .with_issue_type(IssueType::Style.as_str());

                    matches.push(
                        RuleMatch::new(
                            format!(
                                "Sentence is too long ({} words, max: {})",
                                word_count, self.max_words
                            ),
                            start,
                            end - start,
                            rule,
                            RuleMatchContext::new(
                                context_text,
                                context_start,
                                context_end - context_start,
                            ),
                        )
                        .with_sentence(sentence.text().to_string()),
                    );
                }
            }

            pos += sentence.text().len();
        }

        matches
    }
}

pub struct WordRepeatBeginningRule {
    id: String,
    category: Category,
    min_sentences: usize,
}

impl WordRepeatBeginningRule {
    pub fn new() -> Self {
        Self {
            id: "WORD_REPEAT_BEGINNING_RULE".to_string(),
            category: Category::new("STYLE", "Style"),
            min_sentences: 3,
        }
    }
}

impl Default for WordRepeatBeginningRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for WordRepeatBeginningRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Successive sentences start with the same word"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let mut first_words: Vec<(String, usize, usize)> = Vec::new();

        for sentence in sentences {
            let tokens = sentence.non_whitespace_tokens();
            if let Some(first) = tokens.iter().find(|t| !t.is_sentence_start()) {
                let word = first.token().token().to_lowercase();
                let start = first.token().start();
                let end = first.token().end();
                first_words.push((word, start, end));
            }
        }

        let mut i = 0;
        while i < first_words.len() {
            let mut run_len = 1;
            while i + run_len < first_words.len() && first_words[i + run_len].0 == first_words[i].0
            {
                run_len += 1;
            }

            if run_len >= self.min_sentences {
                for j in i..i + run_len {
                    let (_, start, end) = &first_words[j];
                    let context_start = if *start >= 30 { start - 30 } else { 0 };
                    let context_end = std::cmp::min(end + 30, text.len());
                    let context_text = text[context_start..context_end].to_string();

                    let rule = RuleMatchRule::new(&self.id, self.description())
                        .with_category(self.category.clone())
                        .with_issue_type(IssueType::Style.as_str());

                    matches.push(
                        RuleMatch::new(
                            format!(
                                "Three or more sentences start with the word '{}'.",
                                first_words[i].0
                            ),
                            *start,
                            *end - start,
                            rule,
                            RuleMatchContext::new(
                                context_text,
                                context_start,
                                context_end - context_start,
                            ),
                        )
                        .with_sentence("".to_string()),
                    );
                }
            }

            i += run_len;
        }

        matches
    }
}

#[allow(dead_code)]
fn is_normal_whitespace(token: &AnalyzedTokenReadings) -> bool {
    token.is_whitespace() || token.token().token() == "\u{A0}"
}

#[allow(dead_code)]
fn is_removable_whitespace(token: &AnalyzedTokenReadings) -> bool {
    (token.is_whitespace() || token.token().token() == "\u{A0}")
        && !is_linebreak(token)
        && token.token().token() != "\t"
        && !contains_invisible(token)
}

#[allow(dead_code)]
fn is_linebreak(token: &AnalyzedTokenReadings) -> bool {
    let t = token.token().token();
    t.contains('\n') || t.contains('\r')
}

#[allow(dead_code)]
fn contains_invisible(token: &AnalyzedTokenReadings) -> bool {
    let t = token.token().token();
    t.contains('\u{200B}') || t.contains('\u{FEFF}') || t.contains('\u{2060}')
}

#[allow(dead_code)]
fn is_smiley_exception(
    token_text: &str,
    all_tokens: &[AnalyzedTokenReadings],
    current: &AnalyzedTokenReadings,
) -> bool {
    let current_idx = match all_tokens.iter().position(|t| std::ptr::eq(t, current)) {
        Some(idx) => idx,
        None => return false,
    };

    if (token_text == ")" || token_text == "(") && current_idx >= 1 {
        let prev = all_tokens[current_idx - 1].token().token();
        if prev == ":" || prev == ";" {
            return true;
        }
    }

    if (token_text == ")" || token_text == "(") && current_idx >= 2 {
        let prev = all_tokens[current_idx - 1].token().token();
        let prev_prev = all_tokens[current_idx - 2].token().token();
        if prev == "-" && (prev_prev == ":" || prev_prev == ";") {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::TextLevelRule;
    use og_core::{AnalyzedToken, AnalyzedTokenReadings};

    fn make_sentence_with_whitespace(text: &str, offset: usize) -> AnalyzedSentence {
        let mut sentence = AnalyzedSentence::new(text, offset, offset + text.len());
        let mut tokens = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i].is_whitespace() {
                let start = i;
                let mut end = i + 1;
                while end < chars.len() && chars[end].is_whitespace() {
                    end += 1;
                }
                let ws_text: String = chars[start..end].iter().collect();
                let at = AnalyzedToken::new(&ws_text, offset + start, offset + end);
                tokens.push(AnalyzedTokenReadings::new(at));
                i = end;
            } else {
                let start = i;
                let mut end = i + 1;
                while end < chars.len() && !chars[end].is_whitespace() {
                    end += 1;
                }
                let word: String = chars[start..end].iter().collect();
                let at = AnalyzedToken::new(&word, offset + start, offset + end);
                tokens.push(AnalyzedTokenReadings::new(at));
                i = end;
            }
        }

        sentence.set_tokens(tokens);
        sentence
    }

    fn make_simple_sentence(text: &str, offset: usize) -> AnalyzedSentence {
        let mut sentence = AnalyzedSentence::new(text, offset, offset + text.len());
        let mut tokens = Vec::new();
        let mut pos = 0;
        for word in text.split_whitespace() {
            let start = text[pos..].find(word).unwrap_or(0) + pos;
            let end = start + word.len();
            tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                word,
                offset + start,
                offset + end,
            )));
            pos = end;
        }
        sentence.set_tokens(tokens);
        sentence
    }

    #[test]
    fn test_multiple_whitespace_double_space() {
        let rule = MultipleWhitespaceRule::new();
        let s1 = make_sentence_with_whitespace("Hello  world", 0);
        let text = "Hello  world";
        let matches = rule.match_text(text, &[s1]);
        assert_eq!(matches.len(), 1, "Expected 1 match for double space");
        assert!(matches[0].message().contains("duplicated whitespace"));
    }

    #[test]
    fn test_multiple_whitespace_triple_space() {
        let rule = MultipleWhitespaceRule::new();
        let s1 = make_sentence_with_whitespace("Hello   world", 0);
        let text = "Hello   world";
        let matches = rule.match_text(text, &[s1]);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_multiple_whitespace_single_space_ok() {
        let rule = MultipleWhitespaceRule::new();
        let s1 = make_sentence_with_whitespace("Hello world", 0);
        let text = "Hello world";
        let matches = rule.match_text(text, &[s1]);
        assert!(matches.is_empty(), "Single space should not trigger");
    }

    #[test]
    fn test_multiple_whitespace_leading_space_ok() {
        let rule = MultipleWhitespaceRule::new();
        let s1 = make_sentence_with_whitespace("Hello world", 0);
        let text = "Hello world";
        let matches = rule.match_text(text, &[s1]);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_multiple_whitespace_across_sentences() {
        let rule = MultipleWhitespaceRule::new();
        let s1 = make_sentence_with_whitespace("Hello.  ", 0);
        let s2 = make_sentence_with_whitespace("World.", 7);
        let text = "Hello.  World.";
        let matches = rule.match_text(text, &[s1, s2]);
        assert_eq!(matches.len(), 1, "Double space between sentences");
    }

    #[test]
    fn test_sentence_whitespace_missing_space() {
        let rule = SentenceWhitespaceRule::new();
        let s1 = AnalyzedSentence::new("Hello.", 0, 6);
        let mut s2 = AnalyzedSentence::new("World.", 6, 12);
        s1_clone_tokens_for_test(&mut s2, "World.", 6);

        let text = "Hello.World.";
        let matches = rule.match_text(text, &[s1, s2]);
        assert_eq!(matches.len(), 1, "Missing space between sentences");
    }

    #[test]
    fn test_sentence_whitespace_has_space_ok() {
        let rule = SentenceWhitespaceRule::new();
        let mut s1 = AnalyzedSentence::new("Hello. ", 0, 7);
        let mut s2 = AnalyzedSentence::new("World.", 7, 13);

        let tokens1 = vec![
            AnalyzedTokenReadings::new(AnalyzedToken::new("Hello", 0, 5)),
            AnalyzedTokenReadings::new(AnalyzedToken::new(".", 5, 6)),
            AnalyzedTokenReadings::new(AnalyzedToken::new(" ", 6, 7)),
        ];
        s1.set_tokens(tokens1);

        s1_clone_tokens_for_test(&mut s2, "World.", 7);

        let text = "Hello. World.";
        let matches = rule.match_text(text, &[s1, s2]);
        assert!(matches.is_empty(), "Space present, no error");
    }

    fn s1_clone_tokens_for_test(sentence: &mut AnalyzedSentence, text: &str, offset: usize) {
        let mut tokens = Vec::new();
        let mut pos = 0;
        for word in text.split_whitespace() {
            let start = text[pos..].find(word).unwrap() + pos;
            let end = start + word.len();
            let at = AnalyzedToken::new(word, offset + start, offset + end);
            tokens.push(AnalyzedTokenReadings::new(at));
            pos = end;
        }
        sentence.set_tokens(tokens);
    }

    #[test]
    fn test_english_unpaired_quotes_lone_apostrophe() {
        let rule = GenericUnpairedQuotesRule::english();
        assert_eq!(rule.id(), "EN_UNPAIRED_QUOTES");
        let text = "It was ' open";
        let s = make_simple_sentence(text, 0);
        let matches = rule.match_text(text, &[s]);
        assert_eq!(matches.len(), 1, "lone apostrophe is an unpaired quote");
    }

    #[test]
    fn test_english_quotes_paired_apostrophes_ok() {
        let rule = GenericUnpairedQuotesRule::english();
        let text = "It was ' open ' closed";
        let s = make_simple_sentence(text, 0);
        let matches = rule.match_text(text, &[s]);
        assert_eq!(matches.len(), 0, "paired apostrophes must not match");
    }

    #[test]
    fn test_english_quotes_contraction_token_not_flagged() {
        let rule = GenericUnpairedQuotesRule::english();
        let text = "I do n't like it";
        let s = make_simple_sentence(text, 0);
        let matches = rule.match_text(text, &[s]);
        assert_eq!(matches.len(), 0, "contraction token is not a quote symbol");
    }

    #[test]
    fn test_quotes_attached_marks_are_ignored() {
        let rule = GenericUnpairedQuotesRule::english();
        let text = "He said word now";
        let s = make_simple_sentence(text, 0);
        let matches = rule.match_text(text, &[s]);
        assert_eq!(matches.len(), 0);
    }

    #[test]
    fn test_unpaired_opening_bracket() {
        let rule = GenericUnpairedBracketsRule::new();
        let s = make_simple_sentence("Hello world", 0);
        let text = "Hello ( world";
        let matches = rule.match_text(text, &[s]);
        assert_eq!(matches.len(), 1, "Unpaired opening bracket");
        assert!(matches[0].message().contains("Unpaired"));
    }

    #[test]
    fn test_unpaired_closing_bracket() {
        let rule = GenericUnpairedBracketsRule::new();
        let s = make_simple_sentence("Hello world", 0);
        let text = "Hello ) world";
        let matches = rule.match_text(text, &[s]);
        assert_eq!(matches.len(), 1, "Unpaired closing bracket");
    }

    #[test]
    fn test_paired_brackets_ok() {
        let rule = GenericUnpairedBracketsRule::new();
        let s = make_simple_sentence("Hello world", 0);
        let text = "Hello (world)";
        let matches = rule.match_text(text, &[s]);
        assert!(matches.is_empty(), "Paired brackets should not trigger");
    }

    #[test]
    fn test_nested_brackets_ok() {
        let rule = GenericUnpairedBracketsRule::new();
        let s = make_simple_sentence("Hello world", 0);
        let text = "Hello ((world))";
        let matches = rule.match_text(text, &[s]);
        assert!(
            matches.is_empty(),
            "Nested paired brackets should not trigger"
        );
    }

    #[test]
    fn test_smiley_exception() {
        let rule = GenericUnpairedBracketsRule::new();
        let s = make_simple_sentence("Hello world", 0);
        let text = "Hello :) world";
        let matches = rule.match_text(text, &[s]);
        assert!(
            matches.is_empty(),
            "Smiley :) should not trigger unpaired bracket"
        );
    }

    #[test]
    fn test_multiple_bracket_types() {
        let rule = GenericUnpairedBracketsRule::new();
        let s = make_simple_sentence("Hello", 0);
        let text = "[{(Hello)}]";
        let matches = rule.match_text(text, &[s]);
        assert!(matches.is_empty(), "All paired should not trigger");
    }

    #[test]
    fn test_long_sentence_below_java_threshold_not_flagged() {
        let words: Vec<String> = (0..35).map(|i| format!("w{i}")).collect();
        let text = words.join(" ");
        let rule = LongSentenceRule::new(40);
        assert!(
            rule.match_text(&text, &[make_simple_sentence(&text, 0)])
                .is_empty()
        );
        let rule30 = LongSentenceRule::new(30);
        assert_eq!(
            rule30
                .match_text(&text, &[make_simple_sentence(&text, 0)])
                .len(),
            1
        );
    }

    #[test]
    fn test_long_sentence_triggers() {
        let rule = LongSentenceRule::new(5);
        let mut sentence = AnalyzedSentence::new("one two three four five six", 0, 26);
        let mut tokens = Vec::new();
        let mut pos = 0;
        for word in "one two three four five six".split_whitespace() {
            let start = pos;
            let end = start + word.len();
            let at = AnalyzedToken::new(word, start, end);
            tokens.push(AnalyzedTokenReadings::new(at));
            pos = end + 1;
        }
        sentence.set_tokens(tokens);

        let text = "one two three four five six";
        let matches = rule.match_text(text, &[sentence]);
        assert_eq!(
            matches.len(),
            1,
            "Sentence with 6 words, max 5, should trigger"
        );
    }

    #[test]
    fn test_short_sentence_ok() {
        let rule = LongSentenceRule::new(10);
        let mut sentence = AnalyzedSentence::new("Short sentence.", 0, 15);
        let mut tokens = Vec::new();
        for word in "Short sentence.".split_whitespace() {
            let _start = 0usize;
            let at = AnalyzedToken::new(word, 0, word.len());
            tokens.push(AnalyzedTokenReadings::new(at));
        }
        sentence.set_tokens(tokens);

        let text = "Short sentence.";
        let matches = rule.match_text(text, &[sentence]);
        assert!(matches.is_empty(), "Short sentence should not trigger");
    }

    #[test]
    fn test_repeat_beginning_triggers() {
        let rule = WordRepeatBeginningRule::new();

        let mut s1 = AnalyzedSentence::new("The cat sat.", 0, 12);
        let mut s2 = AnalyzedSentence::new("The dog ran.", 13, 25);
        let mut s3 = AnalyzedSentence::new("The bird flew.", 26, 40);

        s1_clone_tokens_for_test(&mut s1, "The cat sat.", 0);
        s1_clone_tokens_for_test(&mut s2, "The dog ran.", 13);
        s1_clone_tokens_for_test(&mut s3, "The bird flew.", 26);

        let text = "The cat sat. The dog ran. The bird flew.";
        let matches = rule.match_text(text, &[s1, s2, s3]);
        assert_eq!(
            matches.len(),
            3,
            "Three sentences starting with 'The' should trigger 3 matches"
        );
    }

    #[test]
    fn test_no_repeat_beginning_ok() {
        let rule = WordRepeatBeginningRule::new();

        let mut s1 = AnalyzedSentence::new("The cat sat.", 0, 12);
        let mut s2 = AnalyzedSentence::new("A dog ran.", 13, 23);
        let mut s3 = AnalyzedSentence::new("Some bird flew.", 24, 39);

        s1_clone_tokens_for_test(&mut s1, "The cat sat.", 0);
        s1_clone_tokens_for_test(&mut s2, "A dog ran.", 13);
        s1_clone_tokens_for_test(&mut s3, "Some bird flew.", 24);

        let text = "The cat sat. A dog ran. Some bird flew.";
        let matches = rule.match_text(text, &[s1, s2, s3]);
        assert!(
            matches.is_empty(),
            "Different starting words should not trigger"
        );
    }

    #[test]
    fn test_two_repeats_not_enough() {
        let rule = WordRepeatBeginningRule::new();

        let mut s1 = AnalyzedSentence::new("The cat sat.", 0, 12);
        let mut s2 = AnalyzedSentence::new("The dog ran.", 13, 25);

        s1_clone_tokens_for_test(&mut s1, "The cat sat.", 0);
        s1_clone_tokens_for_test(&mut s2, "The dog ran.", 13);

        let text = "The cat sat. The dog ran.";
        let matches = rule.match_text(text, &[s1, s2]);
        assert!(matches.is_empty(), "Only 2 repeats, need 3");
    }

    #[test]
    fn punctuation_paragraph_end2_requires_threshold_and_missing_mark() {
        let rule = PunctuationMarkAtParagraphEnd2Rule::new();
        let words = "one two three four five six seven eight nine ten eleven";
        let mut s1 = AnalyzedSentence::new(words, 0, words.len());
        s1_clone_tokens_for_test(&mut s1, words, 0);
        let matches = rule.match_text(words, &[s1]);
        assert_eq!(matches.len(), 1, "11 tokens without final punctuation");
        assert_eq!(matches[0].replacements()[0].value(), "eleven.");
    }

    #[test]
    fn punctuation_paragraph_end2_allows_terminal_punctuation() {
        let rule = PunctuationMarkAtParagraphEnd2Rule::new();
        let words = "one two three four five six seven eight nine ten eleven .";
        let mut s1 = AnalyzedSentence::new(words, 0, words.len());
        s1_clone_tokens_for_test(&mut s1, words, 0);
        let matches = rule.match_text(words, &[s1]);
        assert!(matches.is_empty(), "paragraph ends in a period");
    }
}

fn is_linebreak_token(token: &AnalyzedTokenReadings) -> bool {
    let text = token.token().token();
    text == "\n" || text == "\r"
}

fn is_paragraph_end(sentences: &[AnalyzedSentence], index: usize) -> bool {
    if index >= sentences.len() - 1 {
        return true;
    }
    let text = sentences[index].text();
    if text.ends_with("\n\n") || text.ends_with("\n\r\n\r") || text.ends_with("\r\n\r\n") {
        return true;
    }
    let next = sentences[index + 1].text();
    next.starts_with('\n') || next.starts_with("\r\n")
}

static NON_WORD_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r#"[.?!…:;,~’'\"„“”»«‚‘›‹()\[\]\-–—*×∗·+÷/=]"#).expect("valid regex")
});

fn is_non_word_token(token: &str) -> bool {
    token.chars().count() == 1 && NON_WORD_REGEX.is_match(token)
}

pub struct LongParagraphRule {
    category: Category,
    max_words: usize,
    description: String,
    message: String,
}

impl LongParagraphRule {
    pub fn new(max_words: usize) -> Self {
        Self {
            category: Category::new("STYLE", "Style"),
            max_words,
            description: format!("Readability: paragraph over {max_words} words"),
            message: format!(
                "This paragraph is over {max_words} words long here, consider revising it."
            ),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }
}

impl Default for LongParagraphRule {
    fn default() -> Self {
        Self::new(220)
    }
}

impl TextLevelRule for LongParagraphRule {
    fn id(&self) -> &str {
        "TOO_LONG_PARAGRAPH"
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn is_picky(&self) -> bool {
        true
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

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut rule_matches = Vec::new();
        let mut start_pos = 0usize;
        let mut end_pos = 0usize;
        let mut word_count = 0usize;
        let mut para_has_linebreaks = false;
        for (n, sentence) in sentences.iter().enumerate() {
            let paragraph_end = is_paragraph_end(sentences, n);
            if !paragraph_end && sentence.text().trim_start_matches('\n').contains('\n') {
                para_has_linebreaks = true;
            }
            for token in sentence.non_whitespace_tokens() {
                if !token.is_sentence_start() && !is_non_word_token(token.token().token()) {
                    word_count += 1;
                    if word_count == self.max_words {
                        end_pos = token.token().end();
                    } else if word_count == self.max_words - 1 {
                        start_pos = token.token().start();
                    }
                }
            }
            if paragraph_end {
                if word_count > self.max_words + 5 && !para_has_linebreaks {
                    self.push_match(sentence, start_pos, end_pos, &mut rule_matches);
                }
                word_count = 0;
                para_has_linebreaks = false;
            }
        }
        if word_count > self.max_words
            && let Some(last) = sentences.last()
        {
            self.push_match(last, start_pos, end_pos, &mut rule_matches);
        }
        rule_matches
    }
}

impl LongParagraphRule {
    fn push_match(
        &self,
        sentence: &AnalyzedSentence,
        start: usize,
        end: usize,
        rule_matches: &mut Vec<RuleMatch>,
    ) {
        if end <= start {
            return;
        }
        let rel_start = start.saturating_sub(sentence.start());
        let rel_end = end.saturating_sub(sentence.start());
        let context_start = rel_start.saturating_sub(30);
        let context_end = std::cmp::min(rel_end + 30, sentence.text().len());
        let context_text = sentence.text()[context_start..context_end].to_string();
        let rm_rule = RuleMatchRule::new(self.id(), self.description())
            .with_picky(true)
            .with_category(self.category.clone())
            .with_issue_type(IssueType::Style.as_str());
        rule_matches.push(
            RuleMatch::new(
                &self.message,
                start,
                end - start,
                rm_rule,
                RuleMatchContext::new(context_text, context_start, context_end - context_start),
            )
            .with_sentence(sentence.text().to_string()),
        );
    }
}

pub struct PunctuationMarkAtParagraphEnd2Rule {
    category: Category,
    token_threshold: usize,
}

impl PunctuationMarkAtParagraphEnd2Rule {
    pub fn new() -> Self {
        Self {
            category: Category::new("PUNCTUATION", "Punctuation"),
            token_threshold: 10,
        }
    }
}

impl Default for PunctuationMarkAtParagraphEnd2Rule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for PunctuationMarkAtParagraphEnd2Rule {
    fn id(&self) -> &str {
        "PUNCTUATION_PARAGRAPH_END2"
    }

    fn description(&self) -> &str {
        "No punctuation mark at the end of paragraph"
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Grammar
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut rule_matches = Vec::new();
        let mut token_count = 0usize;
        for (sent_pos, sentence) in sentences.iter().enumerate() {
            for token in sentence.tokens() {
                if !token.is_whitespace() && !is_non_word_token(token.token().token()) {
                    token_count += 1;
                }
            }
            let last_non_space = sentence.tokens().iter().rev().find(|t| !t.is_whitespace());
            let para_end = is_paragraph_end(sentences, sent_pos);
            if para_end
                && token_count > self.token_threshold
                && let Some(last) = last_non_space
            {
                let token_text = last.token().token();
                if !matches!(token_text, ":" | "." | "?" | "!" | "…")
                    && !is_non_word_token(token_text)
                {
                    let start = last.token().start();
                    let end = last.token().end();
                    let ctx_start = start.saturating_sub(30);
                    let ctx_end = std::cmp::min(end + 30, sentence.text().len());
                    let context_text = sentence.text()[ctx_start..ctx_end].to_string();
                    let rm_rule = RuleMatchRule::new(self.id(), self.description())
                        .with_category(self.category.clone())
                        .with_issue_type(IssueType::Grammar.as_str());
                    rule_matches.push(
                        RuleMatch::new(
                            "Please add a punctuation mark at the end of paragraph.",
                            start,
                            end - start,
                            rm_rule,
                            RuleMatchContext::new(context_text, ctx_start, ctx_end - ctx_start),
                        )
                        .with_replacements(vec![SuggestedReplacement::new(format!(
                            "{token_text}."
                        ))])
                        .with_sentence(sentence.text().to_string()),
                    );
                }
            }
            if para_end {
                token_count = 0;
            }
        }
        rule_matches
    }
}

pub struct WhiteSpaceBeforeParagraphEndRule {
    category: Category,
}

impl WhiteSpaceBeforeParagraphEndRule {
    pub fn new() -> Self {
        Self {
            category: Category::new("STYLE", "Style"),
        }
    }
}

impl Default for WhiteSpaceBeforeParagraphEndRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for WhiteSpaceBeforeParagraphEndRule {
    fn id(&self) -> &str {
        "WHITESPACE_PARAGRAPH"
    }

    fn description(&self) -> &str {
        "Whitespace before paragraph end"
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        for (index, sentence) in sentences.iter().enumerate() {
            if !is_paragraph_end(sentences, index) {
                continue;
            }
            let tokens = sentence.tokens();
            let mut line_break = tokens.len().saturating_sub(1);
            while line_break > 0 && is_linebreak_token(&tokens[line_break]) {
                line_break -= 1;
            }
            let mut whitespace = line_break;
            while whitespace > 0
                && tokens[whitespace].is_whitespace()
                && tokens[whitespace].token().token() != "\u{200B}"
            {
                whitespace -= 1;
            }
            if whitespace >= line_break {
                continue;
            }
            let from = if tokens[whitespace].is_whitespace() {
                tokens[whitespace + 1].token().start()
            } else {
                tokens[whitespace].token().start()
            };
            let to = tokens[line_break].token().end();
            let replacement = if whitespace > 0 && !tokens[whitespace].is_whitespace() {
                tokens[whitespace].token().token().to_string()
            } else {
                String::new()
            };
            let rule = RuleMatchRule::new(self.id(), self.description())
                .with_category(self.category.clone())
                .with_issue_type(IssueType::Style.as_str());
            matches.push(
                RuleMatch::new(
                    "Whitespace before the end of the paragraph.".to_string(),
                    sentence.start() + from,
                    to - from,
                    rule,
                    RuleMatchContext::new(sentence.text().to_string(), from, to - from),
                )
                .with_replacements(vec![SuggestedReplacement::new(&replacement)])
                .with_sentence(sentence.text().to_string()),
            );
        }
        matches
    }
}

pub struct WhiteSpaceAtBeginOfParagraphRule {
    category: Category,
}

impl WhiteSpaceAtBeginOfParagraphRule {
    pub fn new() -> Self {
        Self {
            category: Category::new("STYLE", "Style"),
        }
    }
}

impl Default for WhiteSpaceAtBeginOfParagraphRule {
    fn default() -> Self {
        Self::new()
    }
}

fn deletable_whitespace(token: &AnalyzedTokenReadings) -> bool {
    token.is_whitespace() && token.token().token() != "\u{200B}" && !is_linebreak_token(token)
}

pub struct EmptyLineRule {
    category: Category,
}

impl EmptyLineRule {
    pub fn new() -> Self {
        Self {
            category: Category::new("STYLE", "Style"),
        }
    }
}

impl Default for EmptyLineRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for EmptyLineRule {
    fn id(&self) -> &str {
        "EMPTY_LINE"
    }

    fn description(&self) -> &str {
        "Empty line"
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        for index in 0..sentences.len().saturating_sub(1) {
            let sentence = &sentences[index];
            if !is_paragraph_end(sentences, index) {
                continue;
            }
            let text = sentence.text();
            let double_break = text.ends_with("\n\n\n\n")
                || text.ends_with("\n\r\n\r\n\r\n\r")
                || text.ends_with("\r\n\r\n\r\n\r\n");
            if !double_break {
                continue;
            }
            let tokens = sentence.non_whitespace_tokens();
            let Some(last) = tokens.last() else {
                continue;
            };
            let from = last.token().start();
            let to = last.token().end();
            let rule = RuleMatchRule::new(self.id(), self.description())
                .with_category(self.category.clone())
                .with_issue_type(IssueType::Style.as_str());
            matches.push(RuleMatch::new(
                "Empty line.".to_string(),
                sentence.start() + from,
                to - from,
                rule,
                RuleMatchContext::new(sentence.text().to_string(), from, to - from),
            ));
        }
        matches
    }
}

impl og_core::rule::Rule for WhiteSpaceAtBeginOfParagraphRule {
    fn id(&self) -> &str {
        "WHITESPACE_PARAGRAPH_BEGIN"
    }

    fn description(&self) -> &str {
        "Whitespace at the beginning of a paragraph"
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.tokens();
        let mut i = 1;
        while i < tokens.len() && deletable_whitespace(&tokens[i]) {
            i += 1;
        }
        if i <= 1 || i >= tokens.len() || is_linebreak_token(&tokens[i]) {
            return Vec::new();
        }
        let from = tokens[1].token().start();
        let to = tokens[i].token().end();
        let rule = RuleMatchRule::new(self.id(), self.description())
            .with_category(self.category.clone())
            .with_issue_type(IssueType::Style.as_str());
        vec![
            RuleMatch::new(
                "Whitespace at the beginning of a paragraph.".to_string(),
                sentence.start() + from,
                to - from,
                rule,
                RuleMatchContext::new(sentence.text().to_string(), from, to - from),
            )
            .with_replacements(vec![SuggestedReplacement::new(tokens[i].token().token())]),
        ]
    }
}

#[cfg(test)]
mod paragraph_tests {
    use super::*;
    use og_core::rule::{Rule, TextLevelRule};

    fn sentence_from_text(text: &str, offset: usize) -> AnalyzedSentence {
        let mut sentence = AnalyzedSentence::new(text, offset, offset + text.len());
        let mut tokens = vec![og_core::AnalyzedTokenReadings::new(
            og_core::AnalyzedToken::new("<S>", offset, offset)
                .with_pos_tags(vec!["SENT_START".to_string()]),
        )];
        let mut pos = offset;
        let mut last = 0;
        let mut parts: Vec<&str> = Vec::new();
        for (i, c) in text.char_indices() {
            if c == ' ' || c == '\n' || c == '\r' {
                if i > last {
                    parts.push(&text[last..i]);
                }
                parts.push(&text[i..i + c.len_utf8()]);
                last = i + c.len_utf8();
            }
        }
        if last < text.len() {
            parts.push(&text[last..]);
        }
        for part in parts {
            tokens.push(og_core::AnalyzedTokenReadings::new(
                og_core::AnalyzedToken::new(part, pos, pos + part.len()),
            ));
            pos += part.len();
        }
        sentence.set_tokens(tokens);
        sentence
    }

    #[test]
    fn whitespace_before_paragraph_end_is_flagged() {
        let rule = WhiteSpaceBeforeParagraphEndRule::new();
        let sentences = vec![
            sentence_from_text("Hello world. \n\n", 0),
            sentence_from_text("Next", 19),
        ];
        let matches = rule.match_text("", &sentences);
        assert_eq!(matches.len(), 1, "{matches:?}");
        assert!(matches[0].message().contains("Whitespace before"));
    }

    #[test]
    fn no_match_without_trailing_whitespace() {
        let rule = WhiteSpaceBeforeParagraphEndRule::new();
        let sentences = vec![
            sentence_from_text("Hello world.\n\n", 0),
            sentence_from_text("Next", 15),
        ];
        assert!(rule.match_text("", &sentences).is_empty());
    }

    #[test]
    fn whitespace_at_begin_of_paragraph_is_flagged() {
        let rule = WhiteSpaceAtBeginOfParagraphRule::new();
        let sentence = sentence_from_text("  Hello world.", 0);
        let matches = rule.match_sentence(&sentence);
        assert_eq!(matches.len(), 1, "{matches:?}");
    }

    #[test]
    fn empty_line_is_flagged_between_paragraphs() {
        let rule = EmptyLineRule::new();
        let sentences = vec![
            sentence_from_text("One.\n\n\n\n", 0),
            sentence_from_text("Two.", 9),
        ];
        let matches = rule.match_text("", &sentences);
        assert_eq!(matches.len(), 1, "{matches:?}");
    }
}
