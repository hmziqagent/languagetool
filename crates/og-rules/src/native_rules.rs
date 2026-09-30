use og_core::rule::Rule;
use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement,
};

pub struct WordRepeatRule {
    id: String,
    category: Category,
}

impl WordRepeatRule {
    pub fn new() -> Self {
        Self {
            id: "WORD_REPEAT_RULE".to_string(),
            category: Category::new("MISC", "Miscellaneous"),
        }
    }
}

impl Default for WordRepeatRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for WordRepeatRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Word repetition"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Redundancy
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let tokens = sentence.non_whitespace_tokens();

        for i in 1..tokens.len() {
            let prev = tokens[i - 1];
            let curr = tokens[i];

            let prev_text = prev.token().token().to_lowercase();
            let curr_text = curr.token().token().to_lowercase();

            if prev_text == curr_text
                && !prev.is_whitespace()
                && !curr.is_whitespace()
                && prev_text.chars().any(|c| c.is_alphabetic())
            {
                let start = curr.token().start();
                let end = curr.token().end();
                let context_start = start.saturating_sub(20);
                let context_end = std::cmp::min(end + 20, sentence.text().len());
                let context_text = sentence.text()[context_start..context_end].to_string();

                let rule = RuleMatchRule::new(&self.id, self.description())
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Redundancy.as_str());

                matches.push(
                    RuleMatch::new(
                        format!("Possible word repetition: '{}'", curr.token().token()),
                        start,
                        end - start,
                        rule,
                        RuleMatchContext::new(
                            context_text,
                            context_start,
                            context_end - context_start,
                        ),
                    )
                    .with_sentence(sentence.text().to_string())
                    .with_replacements(vec![SuggestedReplacement::new(curr.token().token())]),
                );
            }
        }

        matches
    }
}

pub struct DoublePunctuationRule {
    id: String,
    description: String,
    category: Category,
    dot_message: String,
    dot_short_message: String,
    comma_message: String,
    comma_short_message: String,
    comma_character: char,
}

impl DoublePunctuationRule {
    pub fn new() -> Self {
        Self {
            id: "DOUBLE_PUNCTUATION".to_string(),
            description: "Double punctuation".to_string(),
            category: Category::new("PUNCTUATION", "Punctuation"),
            dot_message: "Two consecutive dots".to_string(),
            dot_short_message: "Two consecutive dots".to_string(),
            comma_message: "Two consecutive commas".to_string(),
            comma_short_message: "Two consecutive commas".to_string(),
            comma_character: ',',
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_messages(
        mut self,
        dot: impl Into<String>,
        dot_short: impl Into<String>,
        comma: impl Into<String>,
        comma_short: impl Into<String>,
    ) -> Self {
        self.dot_message = dot.into();
        self.dot_short_message = dot_short.into();
        self.comma_message = comma.into();
        self.comma_short_message = comma_short.into();
        self
    }
}

impl Default for DoublePunctuationRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for DoublePunctuationRule {
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
        IssueType::Typographical
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let tokens = sentence.non_whitespace_tokens();
        let comma = self.comma_character.to_string();

        let mut start_pos = 0usize;
        let mut dot_count = 0u32;
        let mut comma_count = 0u32;
        for i in 1..tokens.len() {
            let token = tokens[i].token().token();
            let next_token = tokens.get(i + 1).map(|t| t.token().token());
            let prev_prev_token = if i > 1 {
                tokens.get(i - 2).map(|t| t.token().token())
            } else {
                None
            };
            if token == "." {
                dot_count += 1;
                comma_count = 0;
                start_pos = tokens[i].token().start();
            } else if token == comma {
                comma_count += 1;
                dot_count = 0;
                start_pos = tokens[i].token().start();
            }

            if dot_count == 2
                && next_token != Some(".")
                && next_token != Some("\u{2026}")
                && token != "/"
                && next_token != Some("/")
                && token != "\\"
                && next_token != Some("\\")
                && prev_prev_token != Some("?")
                && prev_prev_token != Some("!")
                && prev_prev_token != Some("\u{2026}")
                && prev_prev_token != Some(".")
            {
                let from_pos = start_pos.saturating_sub(1);
                let to_pos = start_pos + 1;
                let context_start = from_pos.saturating_sub(20);
                let context_end = std::cmp::min(to_pos + 20, sentence.text().len());
                let context_text = sentence
                    .text()
                    .get(context_start..context_end)
                    .unwrap_or("")
                    .to_string();
                let rule = RuleMatchRule::new(&self.id, self.description())
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Typographical.as_str());
                matches.push(
                    RuleMatch::new(
                        self.dot_message.clone(),
                        from_pos,
                        to_pos - from_pos,
                        rule,
                        RuleMatchContext::new(
                            context_text,
                            context_start,
                            context_end - context_start,
                        ),
                    )
                    .with_short_message(self.dot_short_message.clone())
                    .with_sentence(sentence.text().to_string())
                    .with_replacements(vec![
                        SuggestedReplacement::new("."),
                        SuggestedReplacement::new("\u{2026}"),
                    ]),
                );
                dot_count = 0;
            } else if comma_count == 2 && next_token != Some(comma.as_str()) {
                let from_pos = start_pos.saturating_sub(1);
                let to_pos = start_pos + 1;
                let context_start = from_pos.saturating_sub(20);
                let context_end = std::cmp::min(to_pos + 20, sentence.text().len());
                let context_text = sentence
                    .text()
                    .get(context_start..context_end)
                    .unwrap_or("")
                    .to_string();
                let rule = RuleMatchRule::new(&self.id, self.description())
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Typographical.as_str());
                matches.push(
                    RuleMatch::new(
                        self.comma_message.clone(),
                        from_pos,
                        to_pos - from_pos,
                        rule,
                        RuleMatchContext::new(
                            context_text,
                            context_start,
                            context_end - context_start,
                        ),
                    )
                    .with_short_message(self.comma_short_message.clone())
                    .with_sentence(sentence.text().to_string())
                    .with_replacements(vec![SuggestedReplacement::new(comma.as_str())]),
                );
                comma_count = 0;
            }

            if token != "." && token != comma {
                dot_count = 0;
                comma_count = 0;
            }
        }

        matches
    }
}

pub struct UppercaseSentenceStartRule {
    id: String,
    category: Category,
}

impl UppercaseSentenceStartRule {
    pub fn new() -> Self {
        Self {
            id: "UPPERCASE_SENTENCE_START".to_string(),
            category: Category::new("CASING", "Capitalization"),
        }
    }
}

impl Default for UppercaseSentenceStartRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for UppercaseSentenceStartRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Sentence should start with an uppercase letter"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Capitalization
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        if tokens.is_empty() {
            return Vec::new();
        }

        let first = tokens[0];
        let text = first.token().token();

        if text
            .chars()
            .next()
            .map(|c| c.is_lowercase())
            .unwrap_or(false)
        {
            let start = first.token().start();
            let end = first.token().end();
            let context_start = start.saturating_sub(20);
            let context_end = std::cmp::min(end + 20, sentence.text().len());
            let context_text = sentence.text()[context_start..context_end].to_string();

            let mut capitalized = text.to_string();
            if let Some(first_char) = capitalized.chars().next() {
                capitalized =
                    first_char.to_uppercase().to_string() + &capitalized[first_char.len_utf8()..];
            }

            let rule = RuleMatchRule::new(&self.id, self.description())
                .with_category(self.category.clone())
                .with_issue_type(IssueType::Capitalization.as_str());

            return vec![
                RuleMatch::new(
                    "This sentence does not start with an uppercase letter.",
                    start,
                    end - start,
                    rule,
                    RuleMatchContext::new(context_text, context_start, context_end - context_start),
                )
                .with_sentence(sentence.text().to_string())
                .with_replacements(vec![SuggestedReplacement::new(capitalized)]),
            ];
        }

        Vec::new()
    }
}

pub struct CommaWhitespaceRule {
    id: String,
    category: Category,
    domain_after_dot_exception: bool,
}

impl CommaWhitespaceRule {
    pub fn new() -> Self {
        Self {
            id: "COMMA_PARENTHESIS_WHITESPACE".to_string(),
            category: Category::new("TYPOGRAPHY", "Typography"),
            domain_after_dot_exception: false,
        }
    }

    pub fn with_domain_after_dot_exception(mut self) -> Self {
        self.domain_after_dot_exception = true;
        self
    }

    fn is_exception(&self, tokens: &[og_core::AnalyzedTokenReadings], i: usize) -> bool {
        if !self.domain_after_dot_exception {
            return false;
        }
        static DOMAINS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new("^[a-z]{2,10}-Domains?$").expect("valid DOMAINS pattern")
        });
        tokens[i].token().token() == "."
            && tokens
                .get(i + 1)
                .is_some_and(|t| DOMAINS.is_match(t.token().token()))
    }
}

impl Default for CommaWhitespaceRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for CommaWhitespaceRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        "Whitespace around comma/parenthesis"
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Whitespace
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let mut matches = Vec::new();
        let tokens = sentence.tokens();

        let mut prev_token = "";
        let mut prev_prev_token = "";
        let mut prev_was_whitespace = false;

        for i in 0..tokens.len() {
            let token_reading = &tokens[i];
            let token = token_reading.token().token();
            let is_whitespace = token_reading.is_whitespace();

            let mut msg: Option<String> = None;
            let mut suggestion: Option<String> = None;

            if is_whitespace && is_left_bracket(prev_token) {
                msg = Some("Don't put a space after an opening bracket.".to_string());
                suggestion = Some(prev_token.to_string());
            } else if !is_whitespace
                && !prev_was_whitespace
                && prev_token == ","
                && !is_quote_char(token)
                && !is_hyphen_or_comma(token)
                && !contains_digit(prev_prev_token)
                && !contains_digit(token)
                && prev_prev_token != ","
            {
                msg = Some("Put a space after a comma.".to_string());
                suggestion = Some(format!(", {}", token));
            } else if prev_was_whitespace {
                if is_right_bracket(token) {
                    msg = Some("Don't put a space before a closing bracket.".to_string());
                    suggestion = Some(token.to_string());
                } else if token == "," {
                    if i + 1 < tokens.len() && tokens[i + 1].token().token() == "," {
                    } else {
                        msg = Some("Don't put a space before a comma.".to_string());
                        let next_space = if i + 1 < tokens.len() && !tokens[i + 1].is_whitespace() {
                            ", "
                        } else {
                            ","
                        };
                        suggestion = Some(next_space.to_string());
                    }
                } else if token == "." {
                    let next_is_digit_or_dot =
                        i + 1 < tokens.len() && is_digit_or_dot(tokens[i + 1].token().token());
                    let next_is_slash_alpha = i + 2 < tokens.len()
                        && tokens[i + 1].token().token() == "/"
                        && tokens[i + 2]
                            .token()
                            .token()
                            .chars()
                            .any(|c| c.is_alphabetic());
                    if !next_is_digit_or_dot && !next_is_slash_alpha {
                        msg = Some("Don't put a space before a period.".to_string());
                        suggestion = Some(".".to_string());
                    }
                }
            }

            if let Some(m) = msg
                && !self.is_exception(tokens, i)
                && !token_reading.is_immunized()
            {
                let from_pos = if i >= 1 {
                    tokens[i - 1].token().start()
                } else {
                    0
                };
                let to_pos = token_reading.token().end();

                if let Some(ref s) = suggestion {
                    let marked =
                        if to_pos <= sentence.text().len() && from_pos <= sentence.text().len() {
                            &sentence.text()[from_pos..to_pos]
                        } else {
                            ""
                        };
                    if marked == s.as_str() {
                        prev_prev_token = prev_token;
                        prev_token = token;
                        prev_was_whitespace = is_whitespace;
                        continue;
                    }
                }

                let context_start = from_pos.saturating_sub(30);
                let context_end = std::cmp::min(to_pos + 30, sentence.text().len());
                let context_text = sentence.text()[context_start..context_end].to_string();

                let rule = RuleMatchRule::new(&self.id, self.description())
                    .with_category(self.category.clone())
                    .with_issue_type(IssueType::Whitespace.as_str());

                let mut m = RuleMatch::new(
                    m,
                    from_pos,
                    to_pos - from_pos,
                    rule,
                    RuleMatchContext::new(context_text, context_start, context_end - context_start),
                )
                .with_sentence(sentence.text().to_string());

                if let Some(s) = suggestion {
                    m = m.with_replacements(vec![SuggestedReplacement::new(s)]);
                }

                matches.push(m);
            }

            prev_prev_token = prev_token;
            prev_token = token;
            prev_was_whitespace = is_whitespace;
        }

        matches
    }
}

fn is_left_bracket(s: &str) -> bool {
    s == "(" || s == "[" || s == "{"
}

fn is_right_bracket(s: &str) -> bool {
    s == ")" || s == "]" || s == "}"
}

fn is_quote_char(s: &str) -> bool {
    matches!(
        s,
        "'" | "\"" | "\u{2019}" | "\u{201D}" | "\u{201C}" | "\u{AB}" | "\u{BB}"
    )
}

fn is_hyphen_or_comma(s: &str) -> bool {
    s == "-" || s == ","
}

fn is_digit_or_dot(s: &str) -> bool {
    s.starts_with('.') || s.starts_with(|c: char| c.is_ascii_digit())
}

fn contains_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::Rule;

    fn make_sentence(text: &str) -> AnalyzedSentence {
        use og_core::{AnalyzedToken, AnalyzedTokenReadings};
        let mut sentence = AnalyzedSentence::new(text, 0, text.len());
        let mut tokens = Vec::new();
        let mut pos = 0;
        for word in text.split_whitespace() {
            let start = text[pos..].find(word).unwrap() + pos;
            let end = start + word.len();
            let at = AnalyzedToken::new(word, start, end);
            tokens.push(AnalyzedTokenReadings::new(at));
            pos = end;
        }
        sentence.set_tokens(tokens);
        sentence
    }

    fn make_punct_sentence(text: &str) -> AnalyzedSentence {
        use og_core::{AnalyzedToken, AnalyzedTokenReadings};
        let mut sentence = AnalyzedSentence::new(text, 0, text.len());
        let mut tokens = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let punct_chars = ",.()[]{}!?;:";
        while i < chars.len() {
            if chars[i].is_whitespace() {
                let start = i;
                let mut end = i + 1;
                while end < chars.len() && chars[end].is_whitespace() {
                    end += 1;
                }
                let ws: String = chars[start..end].iter().collect();
                tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                    &ws, start, end,
                )));
                i = end;
            } else if punct_chars.contains(chars[i]) {
                let ch = chars[i].to_string();
                tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                    &ch,
                    i,
                    i + 1,
                )));
                i += 1;
            } else {
                let start = i;
                let mut end = i + 1;
                while end < chars.len()
                    && !chars[end].is_whitespace()
                    && !punct_chars.contains(chars[end])
                {
                    end += 1;
                }
                let word: String = chars[start..end].iter().collect();
                tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                    &word, start, end,
                )));
                i = end;
            }
        }
        sentence.set_tokens(tokens);
        sentence
    }

    #[test]
    fn test_word_repeat_rule() {
        let rule = WordRepeatRule::new();
        let sentence = make_sentence("the the quick brown fox");
        let matches = rule.match_sentence(&sentence);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].message(), "Possible word repetition: 'the'");
    }

    #[test]
    fn test_word_repeat_no_false_positive() {
        let rule = WordRepeatRule::new();
        let sentence = make_sentence("the quick brown fox");
        let matches = rule.match_sentence(&sentence);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_word_repeat_case_insensitive() {
        let rule = WordRepeatRule::new();
        let sentence = make_sentence("The the quick brown fox");
        let matches = rule.match_sentence(&sentence);
        assert_eq!(matches.len(), 1, "Case-insensitive repeat");
    }

    #[test]
    fn test_word_repeat_three_times() {
        let rule = WordRepeatRule::new();
        let sentence = make_sentence("the the the quick");
        let matches = rule.match_sentence(&sentence);
        assert_eq!(matches.len(), 2, "Three repeats = two matches");
    }

    #[test]
    fn test_word_repeat_no_pure_numbers() {
        let rule = WordRepeatRule::new();
        let sentence = make_sentence("1 1 2 2");
        let matches = rule.match_sentence(&sentence);
        assert!(matches.is_empty(), "Number repetition should not trigger");
    }

    #[test]
    fn test_double_period() {
        let rule = DoublePunctuationRule::new();
        let sentence = make_punct_sentence("Hello..");
        let matches = rule.match_sentence(&sentence);
        assert_eq!(matches.len(), 1, "Double period should trigger");
    }

    #[test]
    fn test_double_comma() {
        let rule = DoublePunctuationRule::new();
        let sentence = make_punct_sentence("Hello,,");
        let matches = rule.match_sentence(&sentence);
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_double_exclamation() {
        let rule = DoublePunctuationRule::new();
        let sentence = make_punct_sentence("Hello!!");
        let matches = rule.match_sentence(&sentence);
        assert!(
            matches.is_empty(),
            "Java flags only dots and commas, not '!!'"
        );
    }

    #[test]
    fn test_triple_period_is_ellipsis() {
        let rule = DoublePunctuationRule::new();
        let sentence = make_punct_sentence("Hello...");
        let matches = rule.match_sentence(&sentence);
        assert!(matches.is_empty(), "Java flags '..' but never '...'");
    }

    #[test]
    fn test_single_punctuation_ok() {
        let rule = DoublePunctuationRule::new();
        let sentence = make_punct_sentence("Hello.");
        let matches = rule.match_sentence(&sentence);
        assert!(matches.is_empty(), "Single punctuation should not trigger");
    }

    #[test]
    fn test_uppercase_sentence_start() {
        let rule = UppercaseSentenceStartRule::new();
        let sentence = make_sentence("hello world");
        let matches = rule.match_sentence(&sentence);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].replacements()[0].value(), "Hello");
    }

    #[test]
    fn test_uppercase_sentence_start_already_capital() {
        let rule = UppercaseSentenceStartRule::new();
        let sentence = make_sentence("Hello world");
        let matches = rule.match_sentence(&sentence);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_uppercase_sentence_start_number_ok() {
        let rule = UppercaseSentenceStartRule::new();
        let sentence = make_sentence("123 people");
        let matches = rule.match_sentence(&sentence);
        assert!(
            matches.is_empty(),
            "Starting with number should not trigger"
        );
    }

    #[test]
    fn test_space_before_comma() {
        let rule = CommaWhitespaceRule::new();
        let sentence = make_punct_sentence("Hello , world");
        let matches = rule.match_sentence(&sentence);
        assert!(!matches.is_empty(), "Space before comma should trigger");
    }

    #[test]
    fn test_no_space_after_comma() {
        let rule = CommaWhitespaceRule::new();
        let sentence = make_punct_sentence("Hello,world");
        let matches = rule.match_sentence(&sentence);
        assert!(
            !matches.is_empty(),
            "Missing space after comma should trigger"
        );
    }

    #[test]
    fn test_correct_comma_spacing() {
        let rule = CommaWhitespaceRule::new();
        let sentence = make_punct_sentence("Hello, world");
        let matches = rule.match_sentence(&sentence);
        assert!(
            matches.is_empty(),
            "Correct comma spacing should not trigger"
        );
    }

    #[test]
    fn test_space_after_opening_bracket() {
        let rule = CommaWhitespaceRule::new();
        let sentence = make_punct_sentence("Hello ( world)");
        let matches = rule.match_sentence(&sentence);
        assert!(
            !matches.is_empty(),
            "Space after opening bracket should trigger"
        );
    }

    #[test]
    fn test_space_before_closing_bracket() {
        let rule = CommaWhitespaceRule::new();
        let sentence = make_punct_sentence("Hello (world )");
        let matches = rule.match_sentence(&sentence);
        assert!(
            !matches.is_empty(),
            "Space before closing bracket should trigger"
        );
    }

    #[test]
    fn test_correct_bracket_spacing() {
        let rule = CommaWhitespaceRule::new();
        let sentence = make_punct_sentence("Hello (world)");
        let matches = rule.match_sentence(&sentence);
        assert!(
            matches.is_empty(),
            "Correct bracket spacing should not trigger"
        );
    }

    #[test]
    fn test_space_before_period() {
        let rule = CommaWhitespaceRule::new();
        let sentence = make_punct_sentence("Hello .");
        let matches = rule.match_sentence(&sentence);
        assert!(!matches.is_empty(), "Space before period should trigger");
    }

    #[test]
    fn test_multiple_comma_issues() {
        let rule = CommaWhitespaceRule::new();
        let sentence = make_punct_sentence("Hello ,world");
        let matches = rule.match_sentence(&sentence);
        assert!(!matches.is_empty(), "Space before comma and no space after");
    }
}
