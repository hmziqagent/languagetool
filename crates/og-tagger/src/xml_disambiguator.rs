use og_core::analyzed::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};
use og_core::checker::Disambiguator;
use og_xml::compiler::{
    CompiledPattern, CompiledPatternElement, CompiledPatternToken, XmlCompiler,
};
use og_xml::disambig_parser::DisambigXmlParser;
use og_xml::disambig_types::{CompiledDisambigRule, DisambigAction, DisambigRule};

enum FirstTokenGate {
    None,
    Text { text: String, case_sensitive: bool },
    LiteralSet(std::collections::HashSet<String>),
    PostagPlain { tag: String, negate_pos: bool },
}

pub struct XmlDisambiguator {
    rules: Vec<CompiledDisambigRule>,
    unifications: Vec<(String, Vec<(String, regex::Regex)>)>,
    action_regexes: Vec<DisambigActionRegex>,
    gates: Vec<FirstTokenGate>,
}

enum DisambigActionRegex {
    Remove(Vec<RemoveWordMatcher>),
    RemovePostag(Option<regex::Regex>),
    Filter(Option<regex::Regex>),
    None,
}

struct RemoveWordMatcher {
    regex: Option<regex::Regex>,
    literal: Option<String>,
    lemma: Option<String>,
}

impl XmlDisambiguator {
    pub fn from_xml(xml: &str) -> Result<Self, String> {
        let parser = DisambigXmlParser::new();
        let rule_set = parser.parse(xml).map_err(|e| e.to_string())?;

        let compiler = XmlCompiler::new();
        let compiled_rules: Vec<CompiledDisambigRule> = rule_set
            .rules
            .into_iter()
            .map(|r| compile_disambig_rule(&compiler, r))
            .collect();

        let mut unifications: Vec<(String, Vec<(String, regex::Regex)>)> = Vec::new();
        for uni in &rule_set.unifications {
            let mut compiled = Vec::new();
            for eq in &uni.equivalences {
                if let Ok(re) = regex::Regex::new(&format!("^(?:{})$", eq.postag)) {
                    compiled.push((eq.eq_type.clone(), re));
                }
            }
            unifications.push((uni.feature.clone(), compiled));
        }

        let is_regex_pos = |pos: &str| {
            pos.contains('.')
                || pos.contains('*')
                || pos.contains('+')
                || pos.contains('[')
                || pos.contains('(')
                || pos.contains('|')
        };
        let action_regexes = compiled_rules
            .iter()
            .map(|rule| match &rule.disambig {
                DisambigAction::Remove(wds) => DisambigActionRegex::Remove(
                    wds.iter()
                        .map(|w| {
                            let literal = w.pos.clone().filter(|pos| !is_regex_pos(pos));
                            let regex = match (&w.pos, &literal) {
                                (Some(pos), None) => regex::Regex::new(pos).ok(),
                                _ => None,
                            };
                            RemoveWordMatcher {
                                regex,
                                literal,
                                lemma: w.lemma.clone(),
                            }
                        })
                        .collect(),
                ),
                DisambigAction::RemovePostag(postag) => DisambigActionRegex::RemovePostag(
                    regex::Regex::new(&format!("^(?:{})$", postag)).ok(),
                ),
                DisambigAction::Filter { postag } => {
                    DisambigActionRegex::Filter(regex::Regex::new(postag).ok())
                }
                _ => DisambigActionRegex::None,
            })
            .collect();

        let gates = compiled_rules.iter().map(build_first_token_gate).collect();

        Ok(Self {
            rules: compiled_rules,
            unifications,
            action_regexes,
            gates,
        })
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn rule_ids(&self) -> Vec<&str> {
        self.rules.iter().filter_map(|r| r.id.as_deref()).collect()
    }
}

fn parse_literal_alternation(pattern: &str) -> Option<Vec<String>> {
    let trimmed = pattern
        .strip_prefix("^(?:")
        .and_then(|p| p.strip_suffix(")$"))
        .unwrap_or(pattern);
    let mut literals = Vec::new();
    for part in trimmed.split('|') {
        if part.is_empty() {
            return None;
        }
        for ch in part.chars() {
            if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' && ch != '\'' && ch != '/' {
                return None;
            }
        }
        literals.push(part.to_string());
    }
    Some(literals)
}

fn build_first_token_gate(rule: &CompiledDisambigRule) -> FirstTokenGate {
    let Some(first) = rule.pattern.tokens.first() else {
        return FirstTokenGate::None;
    };
    if first.min == Some(0) || first.negate {
        return FirstTokenGate::None;
    }
    if let Some(re) = first.compiled_regexp.as_ref()
        && first.text.is_none()
        && !first.inflected
    {
        if let Some(literals) = parse_literal_alternation(re.as_str()) {
            let set = if first.case_sensitive {
                literals.into_iter().collect()
            } else {
                literals
                    .iter()
                    .map(|l| l.to_lowercase())
                    .collect::<std::collections::HashSet<_>>()
            };
            return FirstTokenGate::LiteralSet(set);
        }
        return FirstTokenGate::None;
    }
    if let Some(text) = first.text.as_ref()
        && first.compiled_regexp.is_none()
    {
        return FirstTokenGate::Text {
            text: if first.case_sensitive {
                text.clone()
            } else {
                text.to_lowercase()
            },
            case_sensitive: first.case_sensitive,
        };
    }
    if let Some(postag) = first.postag.as_ref()
        && first.compiled_postag_regexp.is_none()
        && first.text.is_none()
        && first.compiled_regexp.is_none()
    {
        return FirstTokenGate::PostagPlain {
            tag: postag.clone(),
            negate_pos: first.negate_pos,
        };
    }
    FirstTokenGate::None
}

impl Disambiguator for XmlDisambiguator {
    fn disambiguate(&self, sentence: &mut AnalyzedSentence) {
        let mut pending_actions: Vec<(usize, &DisambigAction, &DisambigActionRegex)> = Vec::new();
        let mut pending_unify: Vec<(usize, AnalyzedToken, Vec<AnalyzedToken>)> = Vec::new();

        let tokens: Vec<&AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
        let raw_texts: std::collections::HashSet<&str> =
            tokens.iter().map(|t| t.token().token()).collect();
        let lower_texts: std::collections::HashSet<String> = tokens
            .iter()
            .map(|t| t.token().token().to_lowercase())
            .collect();
        let mut postag_cache: std::collections::HashMap<&str, Vec<bool>> =
            std::collections::HashMap::new();
        let mut postag_evaluated: std::collections::HashMap<&str, Vec<bool>> =
            std::collections::HashMap::new();

        let pos_tags: Vec<std::collections::HashSet<&str>> = tokens
            .iter()
            .map(|t| {
                t.readings()
                    .iter()
                    .flat_map(|r| r.pos_tags().iter().map(|t| t.as_str()))
                    .collect::<std::collections::HashSet<&str>>()
            })
            .collect();

        for (rule_idx, rule) in self.rules.iter().enumerate() {
            if rule.pattern.tokens.is_empty() {
                continue;
            }

            let first = &rule.pattern.tokens[0];
            match &self.gates[rule_idx] {
                FirstTokenGate::Text {
                    text,
                    case_sensitive,
                } => {
                    let can_match = if *case_sensitive {
                        raw_texts.contains(text.as_str())
                    } else {
                        lower_texts.contains(text.as_str()) || raw_texts.contains(text.as_str())
                    };
                    if !can_match {
                        continue;
                    }
                }
                FirstTokenGate::LiteralSet(set) => {
                    let any = if first.case_sensitive {
                        tokens.iter().any(|t| set.contains(t.token().token()))
                    } else {
                        lower_texts.iter().any(|l| set.contains(l))
                    };
                    if !any {
                        continue;
                    }
                }
                FirstTokenGate::PostagPlain { tag, negate_pos } => {
                    let any = pos_tags.iter().any(|set| set.contains(tag.as_str()));
                    if any == *negate_pos {
                        continue;
                    }
                }
                FirstTokenGate::None => {}
            }

            for start_idx in 0..tokens.len() {
                if let FirstTokenGate::PostagPlain { tag, negate_pos } = &self.gates[rule_idx]
                    && pos_tags[start_idx].contains(tag.as_str()) == *negate_pos
                {
                    continue;
                }
                if let Some(re) = first.compiled_postag_regexp.as_ref()
                    && first.text.is_none()
                    && first.compiled_regexp.is_none()
                {
                    let key = re.as_str();
                    let evaluated = postag_evaluated.entry(key).or_default();
                    if evaluated.len() < tokens.len() {
                        evaluated.resize(tokens.len(), false);
                    }
                    if !evaluated[start_idx] {
                        let results = postag_cache.entry(key).or_default();
                        if results.len() < tokens.len() {
                            results.resize(tokens.len(), false);
                        }
                        results[start_idx] = tokens[start_idx]
                            .readings()
                            .iter()
                            .any(|r| r.pos_tags().iter().any(|t| re.is_match(t)));
                        evaluated[start_idx] = true;
                    }
                    if !postag_cache[key][start_idx] && !first.negate_pos {
                        continue;
                    }
                    if postag_cache[key][start_idx] && first.negate_pos {
                        continue;
                    }
                }
                if let Some(match_result) = match_pattern(&rule.pattern, &tokens, start_idx) {
                    let mut blocked = false;
                    for ap in &rule.antipatterns {
                        if antipattern_matches(ap, &tokens, start_idx) {
                            blocked = true;
                            break;
                        }
                    }
                    if blocked {
                        continue;
                    }

                    let mut unify_keep: Option<Vec<regex::Regex>> = None;
                    if let Some(ref spec) = rule.unify {
                        let Some((_, equivalences)) =
                            self.unifications.iter().find(|(f, _)| *f == spec.feature)
                        else {
                            continue;
                        };
                        let has_structured = rule.pattern.has_structured_elements;
                        let (start, end) = if has_structured {
                            (spec.elem_start, spec.elem_end)
                        } else {
                            (spec.token_start, spec.token_end)
                        };
                        let unify_positions: Vec<usize> = match_result
                            .matched_positions
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i >= start && *i < end)
                            .map(|(_, &p)| p)
                            .collect();
                        if unify_positions.len() != end - start {
                            continue;
                        }
                        let mut common: Option<Vec<String>> = None;
                        for &pos in &unify_positions {
                            let values: Vec<String> = equivalences
                                .iter()
                                .filter(|(_, re)| {
                                    tokens[pos]
                                        .readings()
                                        .iter()
                                        .any(|r| r.pos_tags().iter().any(|t| re.is_match(t)))
                                })
                                .map(|(ty, _)| ty.clone())
                                .collect();
                            if values.is_empty() {
                                common = None;
                                break;
                            }
                            common = Some(match common {
                                None => values,
                                Some(prev) => {
                                    prev.into_iter().filter(|v| values.contains(v)).collect()
                                }
                            });
                            if common.as_ref().is_some_and(|c| c.is_empty()) {
                                break;
                            }
                        }
                        let keep = common.unwrap_or_default();
                        if keep.is_empty() {
                            continue;
                        }
                        unify_keep = Some(
                            equivalences
                                .iter()
                                .filter(|(ty, _)| keep.contains(ty))
                                .map(|(_, re)| re.clone())
                                .collect::<Vec<_>>(),
                        );
                    }

                    let target_indices = if let (Some(ms), Some(me)) =
                        (rule.pattern.marker_start, rule.pattern.marker_end)
                    {
                        match_result
                            .matched_positions
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| *i >= ms && *i < me)
                            .map(|(_, &pos)| pos)
                            .collect()
                    } else {
                        match_result
                            .matched_positions
                            .first()
                            .map(|&p| vec![p])
                            .unwrap_or_default()
                    };

                    if matches!(rule.disambig, DisambigAction::RemovePostag(_)) {
                        if let Some(&first) = target_indices.first() {
                            pending_actions.push((
                                first,
                                &rule.disambig,
                                &self.action_regexes[rule_idx],
                            ));
                        }
                    } else if matches!(rule.disambig, DisambigAction::Unify)
                        && let Some(ref regexes) = unify_keep
                    {
                        if let Some(ref spec) = rule.unify {
                            let has_structured = rule.pattern.has_structured_elements;
                            let (start, end) = if has_structured {
                                (spec.elem_start, spec.elem_end)
                            } else {
                                (spec.token_start, spec.token_end)
                            };
                            let unify_positions: Vec<usize> = match_result
                                .matched_positions
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| *i >= start && *i < end)
                                .map(|(_, &p)| p)
                                .collect();
                            for pos in unify_positions {
                                let base = tokens[pos].token().clone();
                                let kept: Vec<AnalyzedToken> = tokens[pos]
                                    .readings()
                                    .to_vec()
                                    .into_iter()
                                    .filter(|r| {
                                        r.pos_tags()
                                            .iter()
                                            .any(|t| regexes.iter().any(|re| re.is_match(t)))
                                    })
                                    .collect();
                                if !kept.is_empty() {
                                    pending_unify.push((pos, base, kept));
                                }
                            }
                        }
                    } else {
                        for token_idx in target_indices {
                            pending_actions.push((
                                token_idx,
                                &rule.disambig,
                                &self.action_regexes[rule_idx],
                            ));
                        }
                    }
                }
            }
        }

        for (non_ws_idx, action, matcher) in pending_actions {
            apply_disambiguation(sentence.tokens_mut(), non_ws_idx, action, matcher);
        }

        let tokens = sentence.tokens_mut();
        let nw: Vec<usize> = (0..tokens.len())
            .filter(|&i| !tokens[i].is_whitespace())
            .collect();
        for (non_ws_idx, base, kept) in pending_unify {
            if let Some(&real_idx) = nw.get(non_ws_idx) {
                let chunks = tokens[real_idx].chunks().to_vec();
                tokens[real_idx] = AnalyzedTokenReadings::new(base).with_readings(kept);
                tokens[real_idx].set_chunks(chunks);
            }
        }
    }
}

struct MatchResult {
    matched_positions: Vec<usize>,
}

fn match_pattern(
    pattern: &CompiledPattern,
    tokens: &[&AnalyzedTokenReadings],
    start_idx: usize,
) -> Option<MatchResult> {
    let elements = &pattern.elements;
    let has_structured = pattern.has_structured_elements;

    if pattern.case_sensitive {
        let prepared = clone_with_case_sensitivity(pattern, true);
        if has_structured {
            return match_elements(&prepared.elements, tokens, start_idx);
        }
        return match_tokens(&prepared.tokens, tokens, start_idx);
    }

    if has_structured {
        match_elements(elements, tokens, start_idx)
    } else {
        match_tokens(&pattern.tokens, tokens, start_idx)
    }
}

fn clone_with_case_sensitivity(pattern: &CompiledPattern, case_sensitive: bool) -> CompiledPattern {
    let map_tokens = |tokens: &[og_xml::compiler::CompiledPatternToken]| -> Vec<og_xml::compiler::CompiledPatternToken> {
        tokens
            .iter()
            .map(|t| {
                let mut t = t.clone();
                t.case_sensitive = case_sensitive;
                t
            })
            .collect()
    };
    let mut prepared = pattern.clone();
    prepared.tokens = map_tokens(&pattern.tokens);
    prepared.elements = pattern
        .elements
        .iter()
        .map(|e| match e {
            og_xml::compiler::CompiledPatternElement::Token(t) => {
                let mut t = t.clone();
                t.case_sensitive = case_sensitive;
                og_xml::compiler::CompiledPatternElement::Token(t)
            }
            og_xml::compiler::CompiledPatternElement::OrGroup(alts) => {
                og_xml::compiler::CompiledPatternElement::OrGroup(map_tokens(alts))
            }
            og_xml::compiler::CompiledPatternElement::AndGroup(cs) => {
                og_xml::compiler::CompiledPatternElement::AndGroup(map_tokens(cs))
            }
        })
        .collect();
    prepared
}

fn match_tokens(
    pattern_tokens: &[CompiledPatternToken],
    tokens: &[&AnalyzedTokenReadings],
    start_idx: usize,
) -> Option<MatchResult> {
    match_tokens_recursive(pattern_tokens, tokens, start_idx, 0)
}

fn match_tokens_recursive(
    pattern_tokens: &[CompiledPatternToken],
    tokens: &[&AnalyzedTokenReadings],
    token_idx: usize,
    pattern_idx: usize,
) -> Option<MatchResult> {
    if pattern_idx >= pattern_tokens.len() {
        return Some(MatchResult {
            matched_positions: Vec::new(),
        });
    }
    if token_idx >= tokens.len() {
        let all_optional = pattern_tokens[pattern_idx..]
            .iter()
            .all(|pt| pt.min.unwrap_or(1) == 0);
        if all_optional {
            return Some(MatchResult {
                matched_positions: Vec::new(),
            });
        }
        return None;
    }

    let pt = &pattern_tokens[pattern_idx];
    let is_optional = pt.min.unwrap_or(1) == 0;

    if token_matches(pt, tokens, token_idx, token_idx > 0) {
        let skip = if pt.skip > 0 { pt.skip as usize } else { 0 };
        let next_token = token_idx + 1 + skip;

        if let Some(mut result) =
            match_tokens_recursive(pattern_tokens, tokens, next_token, pattern_idx + 1)
        {
            result.matched_positions.insert(0, token_idx);
            return Some(result);
        }
    }

    if is_optional {
        return match_tokens_recursive(pattern_tokens, tokens, token_idx, pattern_idx + 1);
    }

    None
}

fn match_elements(
    elements: &[CompiledPatternElement],
    tokens: &[&AnalyzedTokenReadings],
    start_idx: usize,
) -> Option<MatchResult> {
    let mut positions = Vec::new();
    let mut token_idx = start_idx;

    for elem in elements {
        if token_idx >= tokens.len() {
            return None;
        }

        match elem {
            CompiledPatternElement::Token(pt) => {
                if token_matches(pt, tokens, token_idx, token_idx > start_idx) {
                    positions.push(token_idx);
                    token_idx += 1;
                } else if pt.min.unwrap_or(1) == 0 {
                    positions.push(token_idx);
                } else {
                    return None;
                }
            }
            CompiledPatternElement::OrGroup(alternatives) => {
                let mut matched = false;
                for alt in alternatives {
                    if token_matches(alt, tokens, token_idx, token_idx > start_idx) {
                        positions.push(token_idx);
                        token_idx += 1;
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    return None;
                }
            }
            CompiledPatternElement::AndGroup(constraints) => {
                let mut all_match = true;
                for c in constraints {
                    if !token_matches(c, tokens, token_idx, token_idx > start_idx) {
                        all_match = false;
                        break;
                    }
                }
                if all_match {
                    positions.push(token_idx);
                    token_idx += 1;
                } else {
                    return None;
                }
            }
        }
    }

    Some(MatchResult {
        matched_positions: positions,
    })
}

fn token_matches(
    pt: &CompiledPatternToken,
    tokens: &[&AnalyzedTokenReadings],
    token_idx: usize,
    has_previous: bool,
) -> bool {
    if token_idx >= tokens.len() {
        return false;
    }
    let token = tokens[token_idx];
    let text = token.token().token();

    if pt.space_before.as_deref() == Some("no") && has_previous && token_idx > 0 {
        let prev = tokens[token_idx - 1];
        if token.token().start() != prev.token().end() {
            return false;
        }
    }

    let text_matches = if let Some(ref pattern_text) = pt.text {
        let surface_matches = if pt.case_sensitive {
            text == pattern_text
        } else {
            text.eq_ignore_ascii_case(pattern_text)
        };

        if pt.inflected {
            let lemma_matches = surface_matches
                || token.readings().iter().any(|r| {
                    if let Some(lemma) = r.lemma() {
                        if pt.case_sensitive {
                            lemma == pattern_text
                        } else {
                            lemma.eq_ignore_ascii_case(pattern_text)
                        }
                    } else {
                        false
                    }
                });
            if pt.negate {
                !lemma_matches
            } else {
                lemma_matches
            }
        } else {
            if pt.negate {
                !surface_matches
            } else {
                surface_matches
            }
        }
    } else if let Some(ref re) = pt.compiled_regexp {
        let re_to_use = if !pt.case_sensitive {
            pt.compiled_regexp_ci.as_ref().unwrap_or(re)
        } else {
            re
        };
        let surface_match = re_to_use.is_match(text);
        let matched = if pt.inflected {
            surface_match
                || token
                    .readings()
                    .iter()
                    .any(|r| r.lemma().is_some_and(|lemma| re_to_use.is_match(lemma)))
        } else {
            surface_match
        };
        if pt.negate { !matched } else { matched }
    } else if pt.is_regexp {
        false
    } else {
        !pt.negate
    };

    if !text_matches {
        return false;
    }

    let pos_matches = if let Some(ref postag) = pt.postag {
        if let Some(ref re) = pt.compiled_postag_regexp {
            let has_matching = token
                .readings()
                .iter()
                .any(|r| r.pos_tags().iter().any(|t| re.is_match(t)));
            if pt.negate_pos {
                !has_matching
            } else {
                has_matching
            }
        } else {
            let has_matching = token
                .readings()
                .iter()
                .any(|r| r.pos_tags().iter().any(|t| t == postag));
            if pt.negate_pos {
                !has_matching
            } else {
                has_matching
            }
        }
    } else {
        true
    };

    if !pos_matches {
        return false;
    }

    if let Some(ref chunk) = pt.chunk
        && !token.has_chunk(chunk)
    {
        return false;
    }
    if let Some(ref chunk_re) = pt.chunk_re
        && !token.has_chunk_matching(chunk_re)
    {
        return false;
    }

    for exc in &pt.exceptions {
        let text_exc = if let Some(ref exc_text) = exc.text {
            if let Some(ref re) = exc.compiled_regexp {
                re.is_match(text)
            } else {
                text.eq_ignore_ascii_case(exc_text)
            }
        } else {
            false
        };
        let pos_exc = if let Some(ref postag) = exc.postag {
            if let Some(ref re) = exc.compiled_postag_regexp {
                token
                    .readings()
                    .iter()
                    .any(|r| r.pos_tags().iter().any(|t| re.is_match(t)))
            } else {
                token
                    .readings()
                    .iter()
                    .any(|r| r.pos_tags().iter().any(|t| t == postag))
            }
        } else {
            false
        };

        let has_text_constraint = exc.text.is_some();
        let has_pos_constraint = exc.postag.is_some();
        let pos_ok = if exc.negate_pos { !pos_exc } else { pos_exc };
        let exc_matches = if has_text_constraint && has_pos_constraint {
            text_exc && pos_ok
        } else if has_text_constraint {
            text_exc
        } else if has_pos_constraint {
            pos_ok
        } else {
            false
        };

        let exc_active = if exc.negate {
            !exc_matches
        } else {
            exc_matches
        };
        if exc_active {
            return false;
        }
    }

    true
}

fn antipattern_matches(
    antipattern: &CompiledPattern,
    tokens: &[&AnalyzedTokenReadings],
    _rule_start: usize,
) -> bool {
    for start in 0..tokens.len() {
        if match_pattern(antipattern, tokens, start).is_some() {
            return true;
        }
    }
    false
}

fn apply_disambiguation(
    tokens: &mut [AnalyzedTokenReadings],
    token_idx: usize,
    action: &DisambigAction,
    matcher: &DisambigActionRegex,
) {
    let mut real_idx: Option<usize> = None;
    let mut non_ws_count = 0;
    for (i, t) in tokens.iter().enumerate() {
        if !t.is_whitespace() {
            if non_ws_count == token_idx {
                real_idx = Some(i);
                break;
            }
            non_ws_count += 1;
        }
    }
    let real_idx = match real_idx {
        Some(idx) => idx,
        None => return,
    };
    let prior_chunks = tokens[real_idx].chunks().to_vec();

    match action {
        DisambigAction::SetPos(pos) => {
            let base = tokens[real_idx].token().clone();
            let mut new_reading = AnalyzedToken::new(base.token(), base.start(), base.end());
            new_reading.set_pos_tags(vec![pos.clone()]);
            new_reading.set_lemma(base.lemma().map(|l| l.to_string()));
            let mut updated = AnalyzedTokenReadings::new(base).with_readings(vec![new_reading]);
            updated.set_chunks(prior_chunks.clone());
            tokens[real_idx] = updated;
        }
        DisambigAction::Replace(wds) => {
            let base = tokens[real_idx].token().clone();
            let new_readings: Vec<AnalyzedToken> = wds
                .iter()
                .map(|w| {
                    let mut reading = AnalyzedToken::new(base.token(), base.start(), base.end());
                    if let Some(pos) = &w.pos {
                        reading.set_pos_tags(vec![pos.clone()]);
                    }
                    if let Some(lemma) = &w.lemma {
                        reading.set_lemma(Some(lemma.clone()));
                    } else {
                        reading.set_lemma(base.lemma().map(|l| l.to_string()));
                    }
                    reading
                })
                .collect();
            if !new_readings.is_empty() {
                let mut updated = AnalyzedTokenReadings::new(base).with_readings(new_readings);
                updated.set_chunks(prior_chunks.clone());
                tokens[real_idx] = updated;
            }
        }
        DisambigAction::Remove(_) => {
            let DisambigActionRegex::Remove(matchers) = matcher else {
                return;
            };
            let base = tokens[real_idx].token().clone();
            let current_readings = tokens[real_idx].readings().to_vec();
            let filtered: Vec<AnalyzedToken> = current_readings
                .into_iter()
                .filter(|r| {
                    !matchers.iter().any(|w| {
                        let pos_match = match (&w.regex, &w.literal) {
                            (Some(re), _) => r.pos_tags().iter().any(|t| re.is_match(t)),
                            (None, Some(pos)) => r.pos_tags().iter().any(|t| t == pos),
                            (None, None) => false,
                        };
                        let lemma_match = w
                            .lemma
                            .as_ref()
                            .is_none_or(|lemma| r.lemma() == Some(lemma.as_str()));
                        pos_match && lemma_match
                    })
                })
                .collect();
            if !filtered.is_empty() {
                let mut updated = AnalyzedTokenReadings::new(base).with_readings(filtered);
                updated.set_chunks(prior_chunks.clone());
                tokens[real_idx] = updated;
            }
        }
        DisambigAction::RemovePostag(_) => {
            let DisambigActionRegex::RemovePostag(re) = matcher else {
                return;
            };
            let Some(re) = re else {
                return;
            };
            let base = tokens[real_idx].token().clone();
            let current_readings = tokens[real_idx].readings().to_vec();
            let filtered: Vec<AnalyzedToken> = current_readings
                .into_iter()
                .filter(|r| !r.pos_tags().iter().any(|t| re.is_match(t)))
                .collect();
            if !filtered.is_empty() {
                let mut updated = AnalyzedTokenReadings::new(base).with_readings(filtered);
                updated.set_chunks(prior_chunks.clone());
                tokens[real_idx] = updated;
            }
        }
        DisambigAction::Add(wds) => {
            let current_readings = tokens[real_idx].readings().to_vec();
            let base = tokens[real_idx].token().clone();
            let mut new_readings = current_readings;
            for w in wds {
                let mut reading = AnalyzedToken::new(base.token(), base.start(), base.end());
                if let Some(pos) = &w.pos {
                    reading.set_pos_tags(vec![pos.clone()]);
                }
                if let Some(lemma) = &w.lemma {
                    reading.set_lemma(Some(lemma.clone()));
                } else {
                    reading.set_lemma(base.lemma().map(|l| l.to_string()));
                }
                new_readings.push(reading);
            }
            let mut updated = AnalyzedTokenReadings::new(base).with_readings(new_readings);
            updated.set_chunks(prior_chunks.clone());
            tokens[real_idx] = updated;
        }
        DisambigAction::Filter { .. } => {
            let DisambigActionRegex::Filter(re) = matcher else {
                return;
            };
            let Some(re) = re else {
                return;
            };
            let base = tokens[real_idx].token().clone();
            let current_readings = tokens[real_idx].readings().to_vec();
            let filtered: Vec<AnalyzedToken> = current_readings
                .into_iter()
                .filter(|r| r.pos_tags().iter().any(|t| re.is_match(t)))
                .collect();
            if !filtered.is_empty() {
                let mut updated = AnalyzedTokenReadings::new(base).with_readings(filtered);
                updated.set_chunks(prior_chunks.clone());
                tokens[real_idx] = updated;
            }
        }
        DisambigAction::FilterAll => {
            let base = tokens[real_idx].token().clone();
            let empty_reading = AnalyzedToken::new(base.token(), base.start(), base.end());
            let mut updated = AnalyzedTokenReadings::new(base).with_readings(vec![empty_reading]);
            updated.set_chunks(prior_chunks.clone());
            tokens[real_idx] = updated;
        }
        DisambigAction::IgnoreSpelling => {}
        DisambigAction::Unify => {}
    }
}

fn compile_disambig_rule(compiler: &XmlCompiler, rule: DisambigRule) -> CompiledDisambigRule {
    CompiledDisambigRule {
        id: rule.id,
        name: rule.name,
        pattern: CompiledPattern {
            marker_elem_start: None,
            marker_elem_end: None,
            unify_groups: Vec::new(),
            tokens: rule
                .pattern
                .tokens
                .iter()
                .map(|t| compiler.compile_token(t))
                .collect(),
            elements: rule
                .pattern
                .elements
                .iter()
                .map(|e| compiler.compile_element(e))
                .collect(),
            case_sensitive: rule.pattern.case_sensitive,
            marker_start: rule.pattern.marker_start,
            marker_end: rule.pattern.marker_end,
            has_structured_elements: rule.pattern.elements.iter().any(|e| {
                matches!(
                    e,
                    og_xml::types::XmlPatternElement::OrGroup(_)
                        | og_xml::types::XmlPatternElement::AndGroup(_)
                )
            }),
        },
        antipatterns: rule
            .antipatterns
            .iter()
            .map(|ap| CompiledPattern {
                marker_elem_start: ap.marker_elem_start,
                marker_elem_end: ap.marker_elem_end,
                unify_groups: Vec::new(),
                tokens: ap
                    .tokens
                    .iter()
                    .map(|t| compiler.compile_token(t))
                    .collect(),
                elements: ap
                    .elements
                    .iter()
                    .map(|e| compiler.compile_element(e))
                    .collect(),
                case_sensitive: ap.case_sensitive,
                marker_start: None,
                marker_end: None,
                has_structured_elements: ap.elements.iter().any(|e| {
                    matches!(
                        e,
                        og_xml::types::XmlPatternElement::OrGroup(_)
                            | og_xml::types::XmlPatternElement::AndGroup(_)
                    )
                }),
            })
            .collect(),
        disambig: rule.disambig,
        unify: rule.unify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::analyzed::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};

    fn make_sentence(tokens: Vec<(&str, Vec<&str>)>) -> AnalyzedSentence {
        let mut offset = 0;
        let mut analyzed = Vec::new();
        for (text, pos_tags) in tokens {
            let start = offset;
            let end = offset + text.len();
            let base = AnalyzedToken::new(text, start, end);
            let readings: Vec<AnalyzedToken> = pos_tags
                .into_iter()
                .map(|pos| {
                    AnalyzedToken::new(text, start, end).with_pos_tags(vec![pos.to_string()])
                })
                .collect();
            analyzed.push(AnalyzedTokenReadings::new(base).with_readings(readings));
            offset = end + 1;
        }
        let total_len = offset.saturating_sub(1);
        let mut sentence = AnalyzedSentence::new("test", 0, total_len);
        sentence.set_tokens(analyzed);
        sentence
    }

    #[test]
    fn test_set_pos_disambiguation() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="NUM" name="Tag numbers">
        <pattern>
            <token regexp="yes">\d+</token>
        </pattern>
        <disambig postag="CD"/>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        let mut sentence = make_sentence(vec![("10", vec!["CD", "NN"])]);
        disambig.disambiguate(&mut sentence);

        let token = &sentence.tokens()[0];
        assert!(token.has_pos_tag("CD"));
        assert!(!token.has_pos_tag("NN"));
    }

    #[test]
    fn test_remove_disambiguation() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="OR_JJ" name="Remove JJ from or">
        <pattern case_sensitive="yes">
            <token>or</token>
        </pattern>
        <disambig action="remove"><wd pos="JJ"/></disambig>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        let mut sentence = make_sentence(vec![("or", vec!["CC", "JJ", "NN:U"])]);
        disambig.disambiguate(&mut sentence);

        let token = &sentence.tokens()[0];
        assert!(token.has_pos_tag("CC"));
        assert!(!token.has_pos_tag("JJ"));
        assert!(token.has_pos_tag("NN:U"));
    }

    #[test]
    fn test_replace_disambiguation() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="CA" name="Replace ca">
        <pattern>
            <marker>
                <token>ca</token>
            </marker>
            <token spacebefore="no">n't</token>
        </pattern>
        <disambig action="replace"><wd lemma="can" pos="MD"/></disambig>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();

        let mut sentence = AnalyzedSentence::new("ca n't", 0, 5);
        let mut tokens = Vec::new();
        let t1 = AnalyzedToken::new("ca", 0, 2).with_pos_tags(vec!["NN".to_string()]);
        tokens.push(AnalyzedTokenReadings::new(t1));
        let t2 = AnalyzedToken::new("n't", 2, 5).with_pos_tags(vec!["RB".to_string()]);
        tokens.push(AnalyzedTokenReadings::new(t2));
        sentence.set_tokens(tokens);

        disambig.disambiguate(&mut sentence);

        let first = &sentence.tokens()[0];
        assert!(first.has_pos_tag("MD"));
        assert!(!first.has_pos_tag("NN"));
    }

    #[test]
    fn test_add_disambiguation() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="PUNCT" name="Add PCT">
        <pattern>
            <token regexp="yes">[\.,;:…!\?]</token>
        </pattern>
        <disambig action="add"><wd pos="PCT"/></disambig>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        let mut sentence = make_sentence(vec![(".", vec!["."])]);
        disambig.disambiguate(&mut sentence);

        let token = &sentence.tokens()[0];
        assert!(token.has_pos_tag("."));
        assert!(token.has_pos_tag("PCT"));
    }

    #[test]
    fn test_filter_disambiguation() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="VFILT" name="Filter verb">
        <pattern>
            <marker>
                <token postag="V.*" postag_regexp="yes"/>
            </marker>
            <token spacebefore="no">n't</token>
        </pattern>
        <disambig action="filter" postag="V.*"/>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();

        let mut sentence = AnalyzedSentence::new("don't", 0, 5);
        let mut tokens = Vec::new();
        let base1 = AnalyzedToken::new("do", 0, 2);
        let readings1 = vec![
            AnalyzedToken::new("do", 0, 2).with_pos_tags(vec!["VBP".to_string()]),
            AnalyzedToken::new("do", 0, 2).with_pos_tags(vec!["NN".to_string()]),
        ];
        tokens.push(AnalyzedTokenReadings::new(base1).with_readings(readings1));
        let base2 = AnalyzedToken::new("n't", 2, 5);
        let readings2 = vec![AnalyzedToken::new("n't", 2, 5).with_pos_tags(vec!["RB".to_string()])];
        tokens.push(AnalyzedTokenReadings::new(base2).with_readings(readings2));
        sentence.set_tokens(tokens);

        disambig.disambiguate(&mut sentence);

        let first = &sentence.tokens()[0];
        assert!(first.has_pos_tag("VBP"));
        assert!(!first.has_pos_tag("NN"));
    }

    #[test]
    fn test_no_match_no_change() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="NUM" name="Tag numbers">
        <pattern>
            <token regexp="yes">\d+</token>
        </pattern>
        <disambig postag="CD"/>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        let mut sentence = make_sentence(vec![("hello", vec!["UH", "NN"])]);
        disambig.disambiguate(&mut sentence);

        let token = &sentence.tokens()[0];
        assert!(token.has_pos_tag("UH"));
        assert!(token.has_pos_tag("NN"));
    }

    #[test]
    fn test_parse_real_disambiguation_header() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE rules [
    <!ENTITY months "January|February|March|April|May|Ju(ne|ly)|August|September|October|November|December">
]>
<rules lang="en">
    <rule id="MONTHS" name="Months">
        <pattern>
            <token regexp="yes">&months;</token>
        </pattern>
        <disambig postag="NNP"/>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        assert_eq!(disambig.rule_count(), 1);

        let mut sentence = make_sentence(vec![("January", vec!["NNP", "NN"])]);
        disambig.disambiguate(&mut sentence);
        let token = &sentence.tokens()[0];
        assert!(token.has_pos_tag("NNP"));
        assert!(!token.has_pos_tag("NN"));
    }

    #[test]
    fn test_rule_count() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="R1" name="Rule 1">
        <pattern><token>a</token></pattern>
        <disambig postag="DT"/>
    </rule>
    <rule id="R2" name="Rule 2">
        <pattern><token>b</token></pattern>
        <disambig postag="NN"/>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        assert_eq!(disambig.rule_count(), 2);
    }

    #[test]
    fn test_and_group_disambiguation() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="AND" name="And group">
        <pattern>
            <and>
                <token inflected="yes">install</token>
                <token inflected="yes">instal</token>
            </and>
        </pattern>
        <disambig action="remove"><wd lemma="instal"/></disambig>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        assert_eq!(disambig.rule_count(), 1);
    }

    #[test]
    fn test_antipattern_blocks_disambiguation() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="ANTI" name="With antipattern">
        <antipattern>
            <token>nope</token>
        </antipattern>
        <pattern>
            <token>test</token>
        </pattern>
        <disambig postag="NN"/>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();

        let mut sentence1 = make_sentence(vec![("test", vec!["NN", "VB"])]);
        disambig.disambiguate(&mut sentence1);
        assert!(sentence1.tokens()[0].has_pos_tag("NN"));
        assert!(!sentence1.tokens()[0].has_pos_tag("VB"));

        let mut sentence2 = make_sentence(vec![("nope", vec!["RB"]), ("test", vec!["NN", "VB"])]);
        disambig.disambiguate(&mut sentence2);
        assert!(sentence2.tokens()[1].has_pos_tag("NN"));
        assert!(sentence2.tokens()[1].has_pos_tag("VB"));
    }

    #[test]
    fn test_load_real_english_disambiguation() {
        let path = "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/resource/en/disambiguation.xml";
        if !std::path::Path::new(path).exists() {
            eprintln!("Skipping: disambiguation.xml not found");
            return;
        }
        let xml = std::fs::read_to_string(path).unwrap();
        match XmlDisambiguator::from_xml(&xml) {
            Ok(d) => {
                println!("Loaded {} disambiguation rules", d.rule_count());
                assert!(
                    d.rule_count() > 100,
                    "Expected at least 100 disambiguation rules"
                );
            }
            Err(e) => panic!("Failed to load disambiguation.xml: {}", e),
        }
    }
    #[test]
    fn test_remove_postag_strips_matching_readings() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="HAVE_ADJ" name="have + adjective keeps only non-verb readings">
        <pattern>
            <token regexp="yes" postag="VB[ZPG]?" postag_regexp="yes" inflected="yes">be|have</token>
            <marker>
                <token postag="JJ.*" postag_regexp="yes"/>
            </marker>
        </pattern>
        <disambig action="remove" postag="VB.*"/>
    </rule>
</rules>"#;

        let disambiguator = XmlDisambiguator::from_xml(xml).unwrap();
        let mut sentence = make_sentence(vec![
            ("it", vec!["PRP"]),
            ("has", vec!["VBZ"]),
            ("open", vec!["JJ", "NN:U", "VB", "VBP"]),
            ("positions", vec!["NNS"]),
        ]);
        let has = sentence.tokens_mut()[1].token().clone();
        sentence.tokens_mut()[1] = AnalyzedTokenReadings::new(has).with_readings(vec![
            AnalyzedToken::new("has", 3, 6)
                .with_pos_tags(vec!["VBZ".to_string()])
                .with_lemma("have".to_string()),
        ]);
        disambiguator.disambiguate(&mut sentence);
        let token = &sentence.tokens()[2];
        assert!(token.has_pos_tag("JJ"), "JJ must remain");
        assert!(token.has_pos_tag("NN:U"), "NN:U must remain");
        assert!(!token.has_pos_tag("VB"), "VB must be removed");
        assert!(!token.has_pos_tag("VBP"), "VBP must be removed");
    }
    #[test]
    fn test_unify_action_filters_readings_by_number_feature() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <unification feature="number">
        <equivalence type="sg">
            <token postag="NNP?(:UN?)?|VB[ZD]|SENT_END|PCT" postag_regexp="yes"/>
        </equivalence>
        <equivalence type="pl">
            <token postag="NNP?S|VB[PD]|SENT_END|PCT" postag_regexp="yes"/>
        </equivalence>
    </unification>
    <rule id="ENGLISH_NN" name="adjective noun">
        <pattern>
            <token postag_regexp="yes" postag="DT|JJ|PRP\$"/>
            <token postag_regexp="yes" postag="JJ.*|VBG"><exception postag="RB"/><exception regexp="yes">being|one</exception></token>
            <marker>
                <unify>
                    <feature id="number"/>
                    <and>
                        <token postag="NN.*" postag_regexp="yes"/>
                        <token postag="JJ.?" postag_regexp="yes"/>
                    </and>
                    <token postag_regexp="yes" postag="SENT_END|VB."><exception postag_regexp="yes" postag="NN.*|VBN"/></token>
                </unify>
            </marker>
        </pattern>
        <disambig action="unify"/>
    </rule>
</rules>"#;

        let disambiguator = XmlDisambiguator::from_xml(xml).unwrap();
        let mut sentence = make_sentence(vec![
            ("my", vec!["PRP$"]),
            ("bad", vec!["JJ"]),
            ("English", vec!["JJ", "NNP"]),
            (".", vec!["SENT_END", "PCT"]),
        ]);
        disambiguator.disambiguate(&mut sentence);
        let token = &sentence.tokens()[2];
        assert!(!token.has_pos_tag("JJ"), "JJ must be dropped by unify");
        assert!(token.has_pos_tag("NNP"), "NNP must be kept");

        let mut plural = make_sentence(vec![
            ("These", vec!["DT"]),
            ("real", vec!["JJ"]),
            ("English", vec!["JJ", "NNP"]),
            ("are", vec!["VBP"]),
            ("nice", vec!["JJ"]),
            (".", vec!["PCT"]),
        ]);
        disambiguator.disambiguate(&mut plural);
        let token = &plural.tokens()[2];
        assert!(token.has_pos_tag("JJ"), "untouched example keeps JJ");
        assert!(token.has_pos_tag("NNP"), "untouched example keeps NNP");
    }
}
