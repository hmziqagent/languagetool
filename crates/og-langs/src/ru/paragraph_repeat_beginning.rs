use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchContext,
    RuleMatchRule, rule::TextLevelRule,
};
use std::sync::LazyLock;

static QUOTES_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^[’'\"„“”»«‚‘›‹()\\[\\]]$").unwrap());

fn is_article(token: &AnalyzedTokenReadings) -> bool {
    token
        .readings()
        .iter()
        .any(|r| r.pos_tags().iter().any(|t| t.starts_with("DT")))
}

fn num_char_equal_beginning(
    last_tokens: &[&AnalyzedTokenReadings],
    next_tokens: &[&AnalyzedTokenReadings],
) -> usize {
    if last_tokens.len() < 2 || next_tokens.len() < 2 {
        return 0;
    }
    let mut n_token = 1;
    let mut last_token = last_tokens[n_token].token().token();
    let mut next_token = next_tokens[n_token].token().token();
    if QUOTES_REGEX.is_match(last_token) && last_token == next_token {
        if last_tokens.len() <= n_token + 1 || next_tokens.len() <= n_token + 1 {
            return 0;
        }
        n_token += 1;
        last_token = last_tokens[n_token].token().token();
        next_token = next_tokens[n_token].token().token();
    }
    if !last_token.chars().next().is_some_and(char::is_alphabetic) {
        return 0;
    }
    if last_tokens.len() > n_token + 1
        && is_article(last_tokens[n_token])
        && last_token == next_token
    {
        if next_tokens.len() <= n_token + 1 {
            return 0;
        }
        n_token += 1;
        last_token = last_tokens[n_token].token().token();
        next_token = next_tokens[n_token].token().token();
    }
    if !last_token.chars().next().is_some_and(char::is_alphabetic) {
        return 0;
    }
    if last_token == next_token {
        return last_tokens[n_token].token().end();
    }
    0
}

fn is_sentence_start_token(token: &og_core::AnalyzedTokenReadings) -> bool {
    token.token().token() == "<S>" && token.has_pos_tag("SENT_START")
}

fn merge_whitespace_sentences(sentences: &[AnalyzedSentence]) -> Vec<AnalyzedSentence> {
    let mut merged: Vec<AnalyzedSentence> = Vec::with_capacity(sentences.len());
    let mut pending_ws: Option<AnalyzedSentence> = None;
    for sentence in sentences {
        if sentence.text().trim().is_empty() {
            match &mut pending_ws {
                Some(pending) => {
                    let extra: Vec<_> = sentence
                        .tokens()
                        .iter()
                        .filter(|t| !is_sentence_start_token(t))
                        .cloned()
                        .collect();
                    let mut tokens = pending.tokens().to_vec();
                    tokens.extend(extra);
                    pending.set_tokens(tokens);
                }
                None => pending_ws = Some(sentence.clone()),
            }
            continue;
        }
        match pending_ws.take() {
            Some(mut pending) => {
                let extra: Vec<_> = sentence
                    .tokens()
                    .iter()
                    .filter(|t| !is_sentence_start_token(t))
                    .cloned()
                    .collect();
                let mut tokens = pending.tokens().to_vec();
                tokens.extend(extra);
                let text = format!("{}{}", pending.text(), sentence.text());
                let start = pending.start();
                let end = sentence.end();
                let mut combined = AnalyzedSentence::new(text, start, end);
                combined.set_tokens(tokens);
                merged.push(combined);
            }
            None => merged.push(sentence.clone()),
        }
    }
    if let Some(trailing) = pending_ws {
        if let Some(last) = merged.last_mut() {
            let extra: Vec<_> = trailing
                .tokens()
                .iter()
                .filter(|t| !is_sentence_start_token(t))
                .cloned()
                .collect();
            let text = format!("{}{}", last.text(), trailing.text());
            let start = last.start();
            let end = trailing.end();
            let mut tokens = last.tokens().to_vec();
            tokens.extend(extra);
            *last = AnalyzedSentence::new(text, start, end);
            last.set_tokens(tokens);
        } else {
            merged.push(trailing);
        }
    }
    merged
}

pub struct RussianParagraphRepeatBeginningRule;

impl Default for RussianParagraphRepeatBeginningRule {
    fn default() -> Self {
        Self
    }
}

impl TextLevelRule for RussianParagraphRepeatBeginningRule {
    fn id(&self) -> &str {
        "PARAGRAPH_REPEAT_BEGINNING_RULE"
    }

    fn description(&self) -> &str {
        "Повтор начала абзаца"
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        Category::new("STYLE", "Стиль")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut rule_matches = Vec::new();
        if sentences.is_empty() {
            return rule_matches;
        }
        let sentences = &merge_whitespace_sentences(sentences)[..];
        let mut last_sentence = &sentences[0];
        let mut last_tokens = last_sentence.non_whitespace_tokens();
        for n in 0..sentences.len().saturating_sub(1) {
            if !crate::de::helpers::is_paragraph_end(sentences, n) {
                continue;
            }
            let next_sentence = &sentences[n + 1];
            let next_tokens = next_sentence.non_whitespace_tokens();
            let end_pos = num_char_equal_beginning(&last_tokens, &next_tokens);
            if end_pos > 0 {
                let msg = "Повтор начала последнего абзаца";
                if let Some(second) = last_tokens.get(1) {
                    let start_pos = second.token().start();
                    if start_pos < end_pos {
                        let rm_rule = RuleMatchRule::new(self.id(), self.description())
                            .with_category(self.category())
                            .with_issue_type(self.issue_type().as_str());
                        rule_matches.push(
                            RuleMatch::new(
                                msg,
                                start_pos,
                                end_pos - start_pos,
                                rm_rule,
                                RuleMatchContext::new(
                                    last_sentence.text().to_string(),
                                    start_pos,
                                    end_pos - start_pos,
                                ),
                            )
                            .with_sentence(last_sentence.text().to_string()),
                        );
                    }
                }
                if let (Some(last_second), Some(second)) = (last_tokens.get(1), next_tokens.get(1))
                {
                    let start_pos = second.token().start();
                    let next_end = second.token().start() + (end_pos - last_second.token().start());
                    if start_pos < next_end {
                        let rm_rule = RuleMatchRule::new(self.id(), self.description())
                            .with_category(self.category())
                            .with_issue_type(self.issue_type().as_str());
                        rule_matches.push(
                            RuleMatch::new(
                                msg,
                                start_pos,
                                next_end - start_pos,
                                rm_rule,
                                RuleMatchContext::new(
                                    next_sentence.text().to_string(),
                                    start_pos,
                                    next_end - start_pos,
                                ),
                            )
                            .with_sentence(next_sentence.text().to_string()),
                        );
                    }
                }
            }
            last_sentence = next_sentence;
            last_tokens = next_tokens;
        }
        rule_matches
    }
}

#[cfg(test)]
mod tests {
    use og_core::rule::TextLevelRule as _;

    #[test]
    fn flags_repeated_paragraph_beginning_with_java_id_and_ru_messages() {
        let rule = super::RussianParagraphRepeatBeginningRule;
        assert_eq!(rule.id(), "PARAGRAPH_REPEAT_BEGINNING_RULE");
        assert!(!rule.is_default_on());
        assert_eq!(rule.description(), "Повтор начала абзаца");

        let engine = crate::engine::LanguageEngine::shared_for_code("ru").expect("ru engine");
        let text = "Собака бежит быстро.\n\nСобака лает громко.";
        let mut request = og_core::CheckRequest::new(text, engine.language().clone());
        request.enabled_rules = vec!["PARAGRAPH_REPEAT_BEGINNING_RULE".to_string()];
        let matches = engine.check_with_request(&request);
        let hits: Vec<&og_core::RuleMatch> = matches
            .matches
            .iter()
            .filter(|m| m.rule().id() == "PARAGRAPH_REPEAT_BEGINNING_RULE")
            .collect();
        assert_eq!(hits.len(), 2, "one match per paragraph, got {hits:?}");
        assert!(
            hits.iter()
                .all(|m| m.message() == "Повтор начала последнего абзаца")
        );

        let distinct = "Собака бежит быстро.\n\nКошка спит долго.";
        let mut request = og_core::CheckRequest::new(distinct, engine.language().clone());
        request.enabled_rules = vec!["PARAGRAPH_REPEAT_BEGINNING_RULE".to_string()];
        assert!(
            engine
                .check_with_request(&request)
                .matches
                .iter()
                .all(|m| m.rule().id() != "PARAGRAPH_REPEAT_BEGINNING_RULE")
        );
    }
}
