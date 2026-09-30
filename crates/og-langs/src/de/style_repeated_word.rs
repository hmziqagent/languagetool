use super::helpers::{has_pos_tag_starting_with, is_pos_tag_unknown, matches_pos_tag_regex};
use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchContext,
    RuleMatchRule, rule::TextLevelRule,
};
use std::sync::LazyLock;

const MAX_TOKEN_TO_CHECK: usize = 5;
const MAX_DISTANCE_OF_SENTENCES: usize = 1;
const EXCLUDE_DIRECT_SPEECH: bool = true;
const TEST_COMPOUND_WORDS: bool = false;

static OPENING_QUOTES: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^[\"“„»«]$").unwrap());
static ENDING_QUOTES: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^[\"“”»«]$").unwrap());
static SINGLE_QUOTES: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^['‚‘’›‹]$").unwrap());
static LETTERS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^[A-Za-zÄÖÜäöüß]+$").unwrap());

fn is_whitespace_before(tokens: &[&AnalyzedTokenReadings], i: usize) -> bool {
    i > 0 && tokens[i].token().start() > tokens[i - 1].token().end()
}

fn is_unknown_word(token: &AnalyzedTokenReadings) -> bool {
    is_pos_tag_unknown(token)
        && token.token().token().chars().count() > 2
        && LETTERS.is_match(token.token().token())
}

fn has_break_token(tokens: &[&AnalyzedTokenReadings]) -> bool {
    tokens
        .iter()
        .take(MAX_TOKEN_TO_CHECK)
        .any(|t| matches!(t.token().token(), "-" | "—" | "–"))
}

fn is_in_quotes(tokens: &[&AnalyzedTokenReadings], i: usize) -> bool {
    i > 0
        && (OPENING_QUOTES.is_match(tokens[i - 1].token().token())
            || SINGLE_QUOTES.is_match(tokens[i - 1].token().token()))
        && i < tokens.len() - 1
        && (ENDING_QUOTES.is_match(tokens[i + 1].token().token())
            || SINGLE_QUOTES.is_match(tokens[i + 1].token().token()))
}

fn trailing_token<'a>(tokens: &[&'a AnalyzedTokenReadings]) -> &'a str {
    if ENDING_QUOTES.is_match(tokens[tokens.len() - 1].token().token()) {
        tokens[tokens.len() - 2].token().token()
    } else {
        tokens[tokens.len() - 1].token().token()
    }
}

fn is_question_response(
    n_act: usize,
    n_test: usize,
    token_list: &[Vec<&AnalyzedTokenReadings>],
) -> bool {
    let dist = n_act as isize - n_test as isize;
    if dist != 1 && dist != -1 {
        return false;
    }
    let act_tokens = &token_list[n_act];
    let test_tokens = &token_list[n_test];
    if act_tokens.len() < 2 || test_tokens.len() < 2 {
        return false;
    }
    let act_token = trailing_token(act_tokens);
    let test_token = trailing_token(test_tokens);
    (act_token == "?" && test_token != "?") || (test_token == "?" && act_token != "?")
}

fn get_starts_with_direct_speech(
    n: usize,
    sentences: &[AnalyzedSentence],
    mut is_direct_speech: bool,
) -> bool {
    if n == 0 {
        return false;
    }
    let sentence = sentences[n - 1].non_whitespace_tokens();
    for i in 0..sentence.len() {
        let token = sentence[i];
        if !is_direct_speech
            && OPENING_QUOTES.is_match(token.token().token())
            && i < sentence.len() - 1
            && !is_whitespace_before(&sentence, i + 1)
        {
            is_direct_speech = true;
        } else if is_direct_speech
            && ENDING_QUOTES.is_match(token.token().token())
            && i > 1
            && !is_whitespace_before(&sentence, i)
        {
            is_direct_speech = false;
        }
    }
    is_direct_speech
}

pub struct GermanStyleRepeatedWordRule {
    max_distance_of_sentences: usize,
}

impl GermanStyleRepeatedWordRule {
    pub fn new() -> Self {
        Self {
            max_distance_of_sentences: MAX_DISTANCE_OF_SENTENCES,
        }
    }

    fn is_token_to_check(&self, tokens: &[&AnalyzedTokenReadings], n: usize) -> bool {
        if n > 0
            && n < tokens.len() - 1
            && (has_pos_tag_starting_with(tokens[n + 1], "EIG") || is_unknown_word(tokens[n + 1]))
            && matches!(
                tokens[n].token().token(),
                "Frau" | "Fräulein" | "Herr" | "Herrn" | "Lady" | "Mister"
            )
        {
            return false;
        }
        let token = tokens[n];
        ((matches_pos_tag_regex(token, "(SUB|EIG|VER|ADJ):.*")
            && !matches_pos_tag_regex(token, "(PRO|A(RT|DV)|VER:(AUX|MOD)):.*"))
            || is_unknown_word(token))
            && !matches!(
                token.token().token(),
                "sicher" | "weit" | "Sie" | "Ich" | "Euch" | "Eure" | "Der" | "all"
            )
    }

    fn is_token_pair(&self, tokens: &[&AnalyzedTokenReadings], n: usize, before: bool) -> bool {
        let t = |i: usize| tokens[i].token().token();
        if before {
            (n > 2
                && n < tokens.len()
                && has_pos_tag_starting_with(tokens[n - 2], "SUB")
                && has_pos_tag_starting_with(tokens[n - 1], "PRP")
                && has_pos_tag_starting_with(tokens[n], "SUB"))
                || (n >= 2 && t(n - 2) == "hart" && t(n - 1) == "auf" && t(n) == "hart")
                || (n >= 2 && t(n - 2) == "dicht" && t(n - 1) == "an" && t(n) == "dicht")
                || (n >= 2 && t(n - 2) == "fressen" && t(n - 1) == "und" && t(n) == "gefressen")
        } else {
            (n > 0
                && n + 2 < tokens.len()
                && has_pos_tag_starting_with(tokens[n], "SUB")
                && has_pos_tag_starting_with(tokens[n + 1], "PRP")
                && has_pos_tag_starting_with(tokens[n + 2], "SUB"))
                || (n + 2 < tokens.len()
                    && t(n) == "hart"
                    && t(n + 1) == "auf"
                    && t(n + 2) == "hart")
                || (n + 2 < tokens.len()
                    && t(n) == "dicht"
                    && t(n + 1) == "an"
                    && t(n + 2) == "dicht")
                || (n + 2 < tokens.len()
                    && t(n) == "fressen"
                    && t(n + 1) == "und"
                    && t(n + 2) == "gefressen")
        }
    }

    fn is_exception_pair(
        &self,
        token1: &AnalyzedTokenReadings,
        token2: &AnalyzedTokenReadings,
    ) -> bool {
        (token1.has_lemma("nah") && token1.has_lemma("nächst") && !token2.has_lemma("nächst"))
            || (token2.has_lemma("nah")
                && token2.has_lemma("nächst")
                && !token1.has_lemma("nächst"))
            || (token1.has_lemma("gut")
                && ((token1.token().token().starts_with("gut")
                    && !token2.token().token().starts_with("gut"))
                    || (token2.token().token().starts_with("gut")
                        && !token1.token().token().starts_with("gut"))))
    }

    fn is_part_of_word(&self, _test_token_text: &str, _token_text: &str) -> bool {
        TEST_COMPOUND_WORDS
    }

    fn is_token_in_sentence(
        &self,
        test_token: &AnalyzedTokenReadings,
        tokens: &[&AnalyzedTokenReadings],
        not_check: isize,
        is_direct_speech: bool,
    ) -> bool {
        let lemmas: Vec<&str> = test_token
            .readings()
            .iter()
            .filter_map(|r| r.lemma())
            .collect();
        let mut is_direct_speech = is_direct_speech;
        for i in 0..tokens.len() {
            let token_text = tokens[i].token().token();
            if EXCLUDE_DIRECT_SPEECH
                && !is_direct_speech
                && OPENING_QUOTES.is_match(token_text)
                && i < tokens.len() - 1
                && !is_whitespace_before(tokens, i + 1)
            {
                is_direct_speech = true;
            } else if EXCLUDE_DIRECT_SPEECH
                && is_direct_speech
                && ENDING_QUOTES.is_match(token_text)
                && i > 1
                && !is_whitespace_before(tokens, i)
            {
                is_direct_speech = false;
            } else if i as isize != not_check
                && !is_direct_speech
                && !is_in_quotes(tokens, i)
                && self.is_token_to_check(tokens, i)
            {
                let shares_lemma = !lemmas.is_empty()
                    && tokens[i]
                        .readings()
                        .iter()
                        .any(|r| r.lemma().is_some_and(|l| lemmas.contains(&l)))
                    && !self.is_exception_pair(test_token, tokens[i]);
                if shares_lemma || self.is_part_of_word(test_token.token().token(), token_text) {
                    if not_check >= 0 {
                        if not_check == i as isize - 2 {
                            return !self.is_token_pair(tokens, i, true);
                        } else if not_check == i as isize + 2 {
                            return !self.is_token_pair(tokens, i, false);
                        } else if (not_check == i as isize + 1 || not_check == i as isize - 1)
                            && test_token.token().token() == token_text
                        {
                            return false;
                        }
                    }
                    return true;
                }
            }
        }
        false
    }

    fn synonyms_url(token: &AnalyzedTokenReadings) -> String {
        let lemmas: Vec<&str> = token.readings().iter().filter_map(|r| r.lemma()).collect();
        if lemmas.len() == 1 {
            format!("https://www.openthesaurus.de/synonyme/{}", lemmas[0])
        } else {
            format!(
                "https://www.openthesaurus.de/synonyme/{}",
                token.token().token()
            )
        }
    }
}

impl Default for GermanStyleRepeatedWordRule {
    fn default() -> Self {
        Self::new()
    }
}

impl TextLevelRule for GermanStyleRepeatedWordRule {
    fn id(&self) -> &str {
        "STYLE_REPEATED_WORD_RULE_DE"
    }

    fn description(&self) -> &str {
        "Wiederholte Worte in aufeinanderfolgenden Sätzen"
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        Category::new("STYLE", "Stil")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut rule_matches = Vec::new();
        let max_dist = self.max_distance_of_sentences;
        let mut token_list: Vec<Vec<&AnalyzedTokenReadings>> = Vec::new();
        let mut is_ds_list: Vec<bool> = Vec::new();
        let mut starts_with_direct_speech = false;
        for n in 0..max_dist.min(sentences.len()) {
            token_list.push(sentences[n].non_whitespace_tokens());
            starts_with_direct_speech =
                get_starts_with_direct_speech(n, sentences, starts_with_direct_speech);
            is_ds_list.push(starts_with_direct_speech);
        }
        let mut is_direct_speech = false;
        for n in 0..sentences.len() {
            if n + max_dist < sentences.len() {
                token_list.push(sentences[n + max_dist].non_whitespace_tokens());
                starts_with_direct_speech = get_starts_with_direct_speech(
                    n + max_dist,
                    sentences,
                    starts_with_direct_speech,
                );
                is_ds_list.push(starts_with_direct_speech);
            }
            if token_list.len() > 2 * max_dist + 1 {
                token_list.remove(0);
                is_ds_list.remove(0);
            }
            let mut n_tok = max_dist;
            if n < max_dist {
                n_tok = n;
            } else if n + max_dist >= sentences.len() {
                n_tok = token_list.len() - (sentences.len() - n);
            }
            if !has_break_token(&token_list[n_tok]) {
                let tokens = token_list[n_tok].clone();
                for i in 0..tokens.len() {
                    let token = tokens[i];
                    let token_text = token.token().token();
                    if EXCLUDE_DIRECT_SPEECH
                        && !is_direct_speech
                        && OPENING_QUOTES.is_match(token_text)
                        && i < tokens.len() - 1
                        && !is_whitespace_before(&tokens, i + 1)
                    {
                        is_direct_speech = true;
                    } else if EXCLUDE_DIRECT_SPEECH
                        && is_direct_speech
                        && ENDING_QUOTES.is_match(token_text)
                        && i > 1
                        && !is_whitespace_before(&tokens, i)
                    {
                        is_direct_speech = false;
                    } else if !is_direct_speech
                        && !is_in_quotes(&tokens, i)
                        && self.is_token_to_check(&tokens, i)
                    {
                        let mut is_repeated = 0u8;
                        if self.is_token_in_sentence(token, &tokens, i as isize, is_ds_list[n_tok])
                        {
                            is_repeated = 1;
                        }
                        let mut j = n_tok as isize - 1;
                        while is_repeated == 0 && j >= 0 && j >= n_tok as isize - max_dist as isize
                        {
                            if !is_question_response(n_tok, j as usize, &token_list)
                                && self.is_token_in_sentence(
                                    token,
                                    &token_list[j as usize],
                                    -1,
                                    is_ds_list[j as usize],
                                )
                            {
                                is_repeated = 2;
                            }
                            j -= 1;
                        }
                        let mut j = n_tok + 1;
                        while is_repeated == 0 && j < token_list.len() && j <= n_tok + max_dist {
                            if !is_question_response(n_tok, j, &token_list)
                                && self.is_token_in_sentence(
                                    token,
                                    &token_list[j],
                                    -1,
                                    is_ds_list[j],
                                )
                            {
                                is_repeated = 3;
                            }
                            j += 1;
                        }
                        if is_repeated != 0 {
                            let msg = match is_repeated {
                                1 => {
                                    "Mögliches Stilproblem: Das Wort wird noch einmal im selben Satz verwendet."
                                }
                                2 => {
                                    "Mögliches Stilproblem: Das Wort wird bereits in einem vorhergehenden Satz verwendet."
                                }
                                _ => {
                                    "Mögliches Stilproblem: Das Wort wird auch in einem nachfolgenden Satz verwendet."
                                }
                            };
                            let start = token.token().start();
                            let end = token.token().end();
                            let rm_rule = RuleMatchRule::new(self.id(), self.description())
                                .with_category(self.category())
                                .with_issue_type(self.issue_type().as_str())
                                .with_urls(vec![Self::synonyms_url(token)]);
                            rule_matches.push(RuleMatch::new(
                                msg,
                                start,
                                end - start,
                                rm_rule,
                                RuleMatchContext::new(
                                    sentences[n].text().to_string(),
                                    start,
                                    end - start,
                                ),
                            ));
                        }
                    }
                }
            }
        }
        rule_matches
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::LanguageEngine;

    #[test]
    fn flags_repeated_verb_when_enabled() {
        let engine = crate::engine::LanguageEngine::shared_for_code("de").expect("german engine");
        let text = "Ich gehe zum Supermarkt, danach gehe ich nach Hause. Der Hund läuft schnell.";
        let mut request = og_core::CheckRequest::new(text, engine.language().clone());
        request.enabled_rules = vec!["STYLE_REPEATED_WORD_RULE_DE".to_string()];
        request.picky = true;
        let matches = engine.check_with_request(&request);
        assert!(
            matches
                .matches
                .iter()
                .any(|m| m.rule().id() == "STYLE_REPEATED_WORD_RULE_DE"),
            "expected STYLE_REPEATED_WORD_RULE_DE match on repeated 'gehe'"
        );
    }

    #[test]
    fn default_off_in_plain_check() {
        let engine = crate::engine::LanguageEngine::shared_for_code("de").expect("german engine");
        let text = "Ich gehe zum Supermarkt, danach gehe ich nach Hause. Der Hund läuft schnell.";
        let matches = engine.check(text);
        assert!(
            !matches
                .matches
                .iter()
                .any(|m| m.rule().id() == "STYLE_REPEATED_WORD_RULE_DE")
        );
    }
}
