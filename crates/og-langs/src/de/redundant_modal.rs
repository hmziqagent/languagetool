use super::helpers::{eq_ignore_case, has_pos_tag_starting_with};
use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchContext,
    RuleMatchRule, SuggestedReplacement, rule::Rule,
};
use std::sync::LazyLock;

const VERB_TEXT: &str = " scheint redundant zu sein. Prüfen Sie, ob es gelöscht oder der Satz umformuliert werden kann.";
const SUB_TEXT: &str = "Der Satzteil scheint redundant zu sein. Prüfen Sie, ob es gelöscht oder der Satz umformuliert werden kann.";

static MARKS_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r#"^[,;.:?!\-–—’'\"„“”»«‚‘›‹()\[\]]$"#).unwrap());

fn is_mark(token: &str) -> bool {
    MARKS_REGEX.is_match(token)
}

fn is_break_token(token: &str) -> bool {
    is_mark(token) || token == "und" || token == "oder" || token == "sowie"
}

pub struct RedundantModalOrAuxiliaryVerb;

impl RedundantModalOrAuxiliaryVerb {
    fn has_participle_at(
        &self,
        tokens: &[&AnalyzedTokenReadings],
        n_conjunction: usize,
        n_start: usize,
    ) -> Option<usize> {
        let prev = tokens.get(n_conjunction.checked_sub(1)?)?;
        if !has_pos_tag_starting_with(prev, "PA2") {
            return None;
        }
        let participle = prev.token().token();
        for i in n_start..tokens.len() {
            let s_token = tokens[i].token().token();
            if is_break_token(s_token) {
                return None;
            }
            if s_token == participle {
                if i == tokens.len() - 1 || is_break_token(tokens[i + 1].token().token()) {
                    return Some(i);
                }
                return None;
            }
        }
        None
    }
}

impl Default for RedundantModalOrAuxiliaryVerb {
    fn default() -> Self {
        Self
    }
}

impl Rule for RedundantModalOrAuxiliaryVerb {
    fn id(&self) -> &str {
        "REDUNDANT_MODAL_VERB"
    }

    fn description(&self) -> &str {
        "Redundantes Modal- oder Hilfsverb"
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

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        let len = tokens.len();
        let mut rule_matches = Vec::new();
        let tok = |i: usize| tokens[i].token().token();
        let end_of = |i: usize| tokens[i].token().end();
        let mut nt = 2usize;
        while nt < len {
            let is_mod_verb = has_pos_tag_starting_with(tokens[nt], "VER:MOD");
            if (is_mod_verb || has_pos_tag_starting_with(tokens[nt], "VER:AUX"))
                && nt + 1 < len
                && tok(nt - 1) != tok(nt + 1)
            {
                let s_verb = tok(nt).to_string();
                let n_verb = nt;
                let mut do_break = false;
                nt += 1;
                while nt < len {
                    let s_token = tok(nt);
                    if is_mark(s_token) {
                        break;
                    }
                    if s_token == "und" || s_token == "oder" || s_token == "sowie" {
                        let n_conjunction = nt;
                        do_break = true;
                        nt += 1;
                        while nt < len {
                            let s_token = tok(nt);
                            if is_break_token(s_token) {
                                break;
                            }
                            if s_token == s_verb {
                                let verb_kind = if is_mod_verb {
                                    "Modalverb"
                                } else {
                                    "Hilfsverb"
                                };
                                let rule_match = 'outer: {
                                    if nt - 1 == n_conjunction {
                                        if n_verb + 1 == n_conjunction {
                                            break 'outer None;
                                        }
                                        let mut n = 1usize;
                                        while nt + n < len
                                            && eq_ignore_case(tok(nt + n), tok(n_verb + n))
                                        {
                                            n += 1;
                                        }
                                        if n > 1 {
                                            if n_verb + n == n_conjunction {
                                                break 'outer None;
                                            }
                                            Some((
                                                SUB_TEXT.to_string(),
                                                end_of(nt - 1),
                                                end_of(nt + n - 1),
                                                None,
                                            ))
                                        } else {
                                            Some((
                                                format!("Das {verb_kind}{VERB_TEXT}"),
                                                end_of(nt - 1),
                                                end_of(nt),
                                                None,
                                            ))
                                        }
                                    } else if eq_ignore_case(tok(nt - 1), tok(n_verb - 1))
                                        && has_pos_tag_starting_with(tokens[nt - 1], "PRO:PER")
                                        && !has_pos_tag_starting_with(tokens[nt - 1], "ART")
                                    {
                                        if n_verb + 1 == n_conjunction {
                                            Some((
                                                SUB_TEXT.to_string(),
                                                end_of(n_verb - 2),
                                                end_of(n_verb),
                                                None,
                                            ))
                                        } else {
                                            Some((
                                                SUB_TEXT.to_string(),
                                                end_of(nt - 2),
                                                end_of(nt),
                                                None,
                                            ))
                                        }
                                    } else if nt + 1 < len
                                        && eq_ignore_case(tok(nt + 1), tok(n_verb + 1))
                                        && (has_pos_tag_starting_with(tokens[nt + 1], "PRO:IND")
                                            || (has_pos_tag_starting_with(
                                                tokens[nt + 1],
                                                "PRO:PER",
                                            ) && tok(nt + 1) != "Sie"
                                                && !has_pos_tag_starting_with(
                                                    tokens[nt + 1],
                                                    "ART",
                                                )))
                                    {
                                        if has_pos_tag_starting_with(tokens[nt + 1], "PRO:PER:AKK")
                                            && tokens[nt]
                                                .has_pos_tag_matching("VER:(AUX|MOD):.*KJ1")
                                        {
                                            Some((
                                                format!("Das {verb_kind}{VERB_TEXT}"),
                                                end_of(nt - 1),
                                                end_of(nt),
                                                None,
                                            ))
                                        } else {
                                            Some((
                                                SUB_TEXT.to_string(),
                                                end_of(nt - 1),
                                                end_of(nt + 1),
                                                None,
                                            ))
                                        }
                                    } else {
                                        if has_pos_tag_starting_with(tokens[nt - 1], "PRO:PER")
                                            || tok(nt - 1) == "da"
                                            || tok(nt - 1) == "zu"
                                            || tok(n_verb + 1) == tok(nt - 1)
                                            || (nt + 1 < len
                                                && (has_pos_tag_starting_with(
                                                    tokens[nt + 1],
                                                    "PRO:PER",
                                                ) || tok(nt - 1) == tok(nt + 1)
                                                    || tok(n_verb - 1) == tok(nt + 1)
                                                    || (has_pos_tag_starting_with(
                                                        tokens[n_verb + 1],
                                                        "VER:MOD",
                                                    ) && has_pos_tag_starting_with(
                                                        tokens[nt + 1],
                                                        "VER:MOD",
                                                    ))
                                                    || (n_verb + 1 == n_conjunction
                                                        && !is_break_token(tok(nt + 1)))))
                                            || (n_verb + 1 < n_conjunction
                                                && (nt + 1 == len || is_break_token(tok(nt + 1))))
                                        {
                                            break 'outer None;
                                        }
                                        if n_verb + 1 == n_conjunction {
                                            let mut n = 1usize;
                                            while n_verb > n
                                                && nt > n_conjunction + n
                                                && tok(n_verb - n) == tok(nt - n)
                                            {
                                                n += 1;
                                            }
                                            if n > 1 {
                                                Some((
                                                    SUB_TEXT.to_string(),
                                                    end_of(n_verb - n),
                                                    end_of(n_verb),
                                                    None,
                                                ))
                                            } else {
                                                Some((
                                                    format!("Das {verb_kind}{VERB_TEXT}"),
                                                    end_of(n_verb - 1),
                                                    end_of(n_verb),
                                                    None,
                                                ))
                                            }
                                        } else if let Some(pa_at) =
                                            self.has_participle_at(&tokens, n_conjunction, nt + 1)
                                        {
                                            let mut suggestion = String::new();
                                            for token in tokens.iter().take(pa_at).skip(nt + 1) {
                                                suggestion.push(' ');
                                                suggestion.push_str(token.token().token());
                                            }
                                            Some((
                                                SUB_TEXT.to_string(),
                                                end_of(nt - 1),
                                                end_of(pa_at),
                                                Some(suggestion),
                                            ))
                                        } else {
                                            let mut n = 1usize;
                                            while n + n_verb < n_conjunction
                                                && n + nt < len
                                                && tok(n_verb + n) == tok(nt + n)
                                            {
                                                n += 1;
                                            }
                                            if n + n_verb == n_conjunction {
                                                Some((
                                                    SUB_TEXT.to_string(),
                                                    end_of(nt - 1),
                                                    end_of(nt + n - 1),
                                                    None,
                                                ))
                                            } else {
                                                Some((
                                                    format!("Das {verb_kind}{VERB_TEXT}"),
                                                    end_of(nt - 1),
                                                    end_of(nt),
                                                    None,
                                                ))
                                            }
                                        }
                                    }
                                };
                                if let Some((msg, from, to, suggestion)) = rule_match {
                                    let rm_rule = RuleMatchRule::new(self.id(), self.description())
                                        .with_category(self.category())
                                        .with_issue_type(self.issue_type().as_str());
                                    rule_matches.push(
                                        RuleMatch::new(
                                            msg,
                                            from,
                                            to - from,
                                            rm_rule,
                                            RuleMatchContext::new(
                                                sentence.text().to_string(),
                                                from,
                                                to - from,
                                            ),
                                        )
                                        .with_replacements(vec![SuggestedReplacement::new(
                                            suggestion.unwrap_or_default(),
                                        )])
                                        .with_sentence(sentence.text().to_string()),
                                    );
                                }
                                break;
                            }
                            nt += 1;
                        }
                    }
                    if do_break {
                        break;
                    }
                    nt += 1;
                }
            }
            nt += 1;
        }
        rule_matches
    }
}
