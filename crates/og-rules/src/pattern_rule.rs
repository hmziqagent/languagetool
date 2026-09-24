use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, RuleMatch, RuleMatchContext, RuleMatchRule,
    SuggestedReplacement,
};
use og_xml::compiler::CompiledRule;
use std::collections::HashMap;

/// Java LT normalizes these, so "it\u{2019}s" should match pattern token "'s".
fn normalize_apostrophes(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains('\u{2019}') || text.contains('\u{2018}') || text.contains('\u{201B}') {
        std::borrow::Cow::Owned(text.replace(['\u{2019}', '\u{2018}', '\u{201B}'], "'"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

fn apply_case_conversion(text: &str, conversion: &str) -> String {
    match conversion {
        "startlower" => {
            let mut chars = text.chars();
            match chars.next() {
                Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        "startupper" => {
            let mut chars = text.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
        "alllower" => text.to_lowercase(),
        "allupper" => text.to_uppercase(),
        _ => text.to_string(),
    }
}

pub struct PatternRuleEngine {
    word_index: HashMap<String, Vec<usize>>,
    catch_all_rule_indices: Vec<usize>,
    spelling: Option<std::sync::Arc<crate::spelling_support::SpellingSupport>>,
}

impl PatternRuleEngine {
    pub fn new() -> Self {
        Self {
            word_index: HashMap::new(),
            catch_all_rule_indices: Vec::new(),
            spelling: None,
        }
    }

    pub fn with_spelling_support(
        mut self,
        spelling: std::sync::Arc<crate::spelling_support::SpellingSupport>,
    ) -> Self {
        self.spelling = Some(spelling);
        self
    }

    pub fn build_index(&mut self, rules: &[&CompiledRule]) {
        self.word_index.clear();
        self.catch_all_rule_indices.clear();

        for (rule_idx, rule) in rules.iter().enumerate() {
            if rule.pattern.tokens.is_empty() {
                continue;
            }
            let first_token = &rule.pattern.tokens[0];
            if let Some(ref text) = first_token.text
                && !first_token.negate
                && first_token.compiled_regexp.is_none()
            {
                let key = text.to_lowercase();
                self.word_index.entry(key).or_default().push(rule_idx);
                continue;
            }
            self.catch_all_rule_indices.push(rule_idx);
        }
    }

    pub fn match_indexed_rules(
        &self,
        rules: &[&CompiledRule],
        sentence: &AnalyzedSentence,
    ) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        let mut all_matches = Vec::new();

        for token_idx in 0..tokens.len() {
            let token_lower = tokens[token_idx].token().token().to_lowercase();

            if let Some(rule_indices) = self.word_index.get(&token_lower) {
                for &rule_idx in rule_indices {
                    if let Some(rm) =
                        self.try_match_at(rules[rule_idx], &tokens, token_idx, sentence)
                    {
                        all_matches.push(rm);
                    }
                }
            }

            for &rule_idx in &self.catch_all_rule_indices {
                if let Some(rm) = self.try_match_at(rules[rule_idx], &tokens, token_idx, sentence) {
                    all_matches.push(rm);
                }
            }
        }

        all_matches
    }

    pub fn match_indexed_rules_parallel(
        &self,
        rules: &[&CompiledRule],
        sentences: &[AnalyzedSentence],
    ) -> Vec<RuleMatch> {
        use rayon::prelude::*;

        sentences
            .par_iter()
            .flat_map(|sentence| self.match_indexed_rules(rules, sentence))
            .collect()
    }
}

impl PatternRuleEngine {
    pub fn match_rule(&self, rule: &CompiledRule, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        let mut matches = Vec::new();

        if rule.pattern.tokens.is_empty() && rule.pattern.elements.is_empty() {
            return matches;
        }

        let has_structured = rule.pattern.has_structured_elements;

        let first_element_is_group = has_structured
            && matches!(
                rule.pattern.elements.first(),
                Some(og_xml::compiler::CompiledPatternElement::OrGroup(_))
                    | Some(og_xml::compiler::CompiledPatternElement::AndGroup(_))
            );

        if first_element_is_group {
            for start_idx in 0..tokens.len() {
                if let Some(rule_match) = self.try_match_at(rule, &tokens, start_idx, sentence) {
                    matches.push(rule_match);
                }
            }
            return matches;
        }

        let first_token = &rule.pattern.tokens[0];

        let start_positions: Vec<usize> = if let Some(ref text) = first_token.text {
            if !first_token.negate && first_token.compiled_regexp.is_none() {
                tokens
                    .iter()
                    .enumerate()
                    .filter(|(_, t)| {
                        let surface_matches = if first_token.case_sensitive {
                            t.token().token() == text
                        } else {
                            t.token().token().eq_ignore_ascii_case(text)
                        };
                        if surface_matches {
                            true
                        } else if first_token.inflected {
                            t.readings().iter().any(|r| {
                                if let Some(lemma) = r.lemma() {
                                    if first_token.case_sensitive {
                                        lemma == text
                                    } else {
                                        lemma.eq_ignore_ascii_case(text)
                                    }
                                } else {
                                    false
                                }
                            })
                        } else {
                            false
                        }
                    })
                    .map(|(i, _)| i)
                    .collect()
            } else {
                (0..tokens.len()).collect()
            }
        } else {
            (0..tokens.len()).collect()
        };

        for start_idx in start_positions {
            if let Some(rule_match) = self.try_match_at(rule, &tokens, start_idx, sentence) {
                matches.push(rule_match);
            }
        }

        matches
    }

    pub fn match_rules(
        &self,
        rules: &[&CompiledRule],
        sentence: &AnalyzedSentence,
    ) -> Vec<RuleMatch> {
        let mut engine = Self::new();
        engine.build_index(rules);
        engine.match_indexed_rules(rules, sentence)
    }

    fn try_match_at(
        &self,
        rule: &CompiledRule,
        tokens: &[&AnalyzedTokenReadings],
        start_idx: usize,
        sentence: &AnalyzedSentence,
    ) -> Option<RuleMatch> {
        let pattern_tokens = &rule.pattern.tokens;
        let elements = &rule.pattern.elements;

        let has_structured = rule.pattern.has_structured_elements;

        let match_result = if has_structured {
            self.match_pattern_elements(elements, tokens, start_idx)?
        } else {
            self.match_pattern_tokens(pattern_tokens, tokens, start_idx)?
        };

        for antipattern in rule.antipatterns.iter() {
            if self.antipattern_matches_with_unification(
                antipattern,
                tokens,
                &match_result.matched_positions,
                &rule.unifications,
            ) {
                return None;
            }
        }

        if !rule.pattern.unify_groups.is_empty()
            && !unify_groups_satisfied(rule, &match_result, tokens)
        {
            return None;
        }

        let text = sentence.text();
        let marker_indices = if rule.pattern.has_structured_elements {
            (rule.pattern.marker_elem_start, rule.pattern.marker_elem_end)
        } else {
            (rule.pattern.marker_start, rule.pattern.marker_end)
        };
        let (match_start, match_end) = if let (Some(ms), Some(me)) = marker_indices {
            let ms_tok_idx = *match_result.matched_positions.get(ms)?;
            let me_idx = if me > 0 { me - 1 } else { 0 };
            let me_tok_idx = *match_result.matched_positions.get(me_idx)?;
            let start_tok = tokens.get(ms_tok_idx)?;
            let end_tok = tokens.get(me_tok_idx)?;
            let mut start = start_tok.token().start();
            while start > 0
                && text
                    .as_bytes()
                    .get(start - 1)
                    .map(|&b| b.is_ascii_whitespace())
                    .unwrap_or(false)
            {
                start -= 1;
            }
            let end = end_tok.token().end();
            (start, end)
        } else {
            let mut start = match_result.first_start?;
            let end = match_result.last_end?;
            while start > 0
                && text
                    .as_bytes()
                    .get(start - 1)
                    .map(|&b| b.is_ascii_whitespace())
                    .unwrap_or(false)
            {
                start -= 1;
            }
            (start, end)
        };

        if match_end < match_start {
            return None;
        }

        let match_length = match_end - match_start;

        let context_start = {
            let mut pos = match_start.saturating_sub(40);
            while !text.is_char_boundary(pos) && pos < text.len() {
                pos += 1;
            }
            pos
        };
        let context_end = {
            let mut pos = std::cmp::min(match_end + 40, text.len());
            while !text.is_char_boundary(pos) && pos > 0 {
                pos -= 1;
            }
            pos
        };
        let context_text = text[context_start..context_end].to_string();

        let replacements: Vec<SuggestedReplacement> = rule
            .suggestions
            .iter()
            .map(|s| {
                if !s.parts.is_empty() {
                    let mut resolved = String::new();
                    for part in &s.parts {
                        match part {
                            og_xml::types::SuggestionPart::Text(t) => {
                                resolved.push_str(
                                    &self.resolve_backreferences(t, &match_result.matched_texts),
                                );
                            }
                            og_xml::types::SuggestionPart::Match(m) => {
                                let token_idx = (m.no as usize).saturating_sub(1);
                                if let Some(&pos) = match_result.matched_positions.get(token_idx)
                                    && let Some(token) = tokens.get(pos)
                                {
                                    let mut text = token.token().token().to_string();
                                    if let Some(ref cc) = m.case_conversion {
                                        text = apply_case_conversion(&text, cc);
                                    }
                                    if let Some(ref rm) = m.regexp_match
                                        && let Ok(re) = regex::Regex::new(rm)
                                    {
                                        let replace = m.regexp_replace.as_deref().unwrap_or("");
                                        text = re.replace_all(&text, replace).to_string();
                                    }
                                    resolved.push_str(&text);
                                }
                            }
                        }
                    }
                    SuggestedReplacement::new(&resolved)
                } else {
                    let resolved =
                        self.resolve_backreferences(&s.text, &match_result.matched_texts);
                    SuggestedReplacement::new(&resolved)
                }
            })
            .collect();

        let rm_rule = RuleMatchRule::new(&rule.id, &rule.name)
            .with_category(og_core::Category::new(
                &rule.category.id,
                &rule.category.name,
            ))
            .with_issue_type(&rule.issue_type);
        let rm_rule = if let Some(ref sub_id) = rule.sub_id {
            rm_rule.with_sub_id(sub_id)
        } else {
            rm_rule
        };
        let rm_rule = if let Some(ref url) = rule.url {
            rm_rule.with_urls(vec![url.clone()])
        } else {
            rm_rule
        };
        let rm_rule = if let Some(ref source_file) = rule.source_file {
            rm_rule.with_source_file(source_file)
        } else {
            rm_rule
        };
        let match_kind = rm_rule.pattern_match_kind();

        let mut rm = RuleMatch::new(
            &rule.message,
            match_start,
            match_length,
            rm_rule,
            RuleMatchContext::new(context_text, context_start, context_end - context_start),
        )
        .with_replacements(replacements)
        .with_match_type(match_kind)
        .with_sentence(sentence.text().to_string());
        if let Some(ref short) = rule.short_message {
            rm = rm.with_short_message(short);
        }

        if let Some(ref filter) = rule.filter {
            return self.apply_filter(
                filter,
                &match_result.matched_texts,
                &match_result.matched_positions,
                tokens,
                sentence,
                rm,
            );
        }

        Some(rm)
    }

    fn match_pattern_tokens(
        &self,
        pattern_tokens: &[og_xml::compiler::CompiledPatternToken],
        tokens: &[&AnalyzedTokenReadings],
        start_idx: usize,
    ) -> Option<PatternMatchResult> {
        self.match_pattern_recursive(pattern_tokens, tokens, start_idx, 0, &[])
    }

    fn match_pattern_elements(
        &self,
        elements: &[og_xml::compiler::CompiledPatternElement],
        tokens: &[&AnalyzedTokenReadings],
        start_idx: usize,
    ) -> Option<PatternMatchResult> {
        self.match_elements_recursive(elements, tokens, start_idx, 0, &[])
    }

    fn backreference_matches(
        &self,
        pattern_token: &og_xml::compiler::CompiledPatternToken,
        token: &AnalyzedTokenReadings,
        matched_texts: &[String],
    ) -> Option<bool> {
        let match_no = pattern_token.match_no?;
        let ref_idx = match_no as usize;
        let ref_text = normalize_apostrophes(matched_texts.get(ref_idx)?);
        let token_text = normalize_apostrophes(token.token().token());
        let matches = if pattern_token.case_sensitive {
            token_text == ref_text
        } else {
            token_text.eq_ignore_ascii_case(&ref_text)
        };
        if pattern_token.negate {
            Some(!matches)
        } else {
            Some(matches)
        }
    }

    fn match_elements_recursive(
        &self,
        elements: &[og_xml::compiler::CompiledPatternElement],
        tokens: &[&AnalyzedTokenReadings],
        token_idx: usize,
        element_idx: usize,
        matched_texts: &[String],
    ) -> Option<PatternMatchResult> {
        if element_idx >= elements.len() {
            return Some(PatternMatchResult {
                first_start: None,
                last_end: None,
                matched_positions: Vec::new(),
                matched_texts: Vec::new(),
            });
        }

        if token_idx >= tokens.len() {
            let remaining_ok: bool = (element_idx..elements.len()).all(|i| match &elements[i] {
                og_xml::compiler::CompiledPatternElement::Token(t) => t.min == Some(0),
                _ => false,
            });
            if remaining_ok {
                return Some(PatternMatchResult {
                    first_start: None,
                    last_end: None,
                    matched_positions: Vec::new(),
                    matched_texts: Vec::new(),
                });
            }
            return None;
        }

        let element = &elements[element_idx];
        match element {
            og_xml::compiler::CompiledPatternElement::Token(pattern_token) => self
                .match_token_element(
                    pattern_token,
                    elements,
                    tokens,
                    token_idx,
                    element_idx,
                    matched_texts,
                ),
            og_xml::compiler::CompiledPatternElement::OrGroup(alternatives) => {
                for alt in alternatives {
                    let matches = if let Some(br) =
                        self.backreference_matches(alt, tokens[token_idx], matched_texts)
                    {
                        br
                    } else {
                        self.token_matches(alt, tokens[token_idx], token_idx, tokens)
                    };
                    if matches {
                        let mut new_mt = matched_texts.to_vec();
                        new_mt.push(tokens[token_idx].token().token().to_string());
                        let mut result = self.match_elements_recursive(
                            elements,
                            tokens,
                            token_idx + 1,
                            element_idx + 1,
                            &new_mt,
                        )?;
                        result.matched_positions.insert(0, token_idx);
                        result
                            .matched_texts
                            .insert(0, tokens[token_idx].token().token().to_string());
                        if result.first_start.is_none() {
                            result.first_start = Some(tokens[token_idx].token().start());
                        }
                        result.last_end = Some(tokens[token_idx].token().end());
                        return Some(result);
                    }
                }
                None
            }
            og_xml::compiler::CompiledPatternElement::AndGroup(constraints) => {
                let all_match = constraints.iter().all(|c| {
                    if let Some(br) =
                        self.backreference_matches(c, tokens[token_idx], matched_texts)
                    {
                        br
                    } else {
                        self.token_matches(c, tokens[token_idx], token_idx, tokens)
                    }
                });
                if all_match {
                    let mut new_mt = matched_texts.to_vec();
                    new_mt.push(tokens[token_idx].token().token().to_string());
                    let mut result = self.match_elements_recursive(
                        elements,
                        tokens,
                        token_idx + 1,
                        element_idx + 1,
                        &new_mt,
                    )?;
                    result.matched_positions.insert(0, token_idx);
                    result
                        .matched_texts
                        .insert(0, tokens[token_idx].token().token().to_string());
                    if result.first_start.is_none() {
                        result.first_start = Some(tokens[token_idx].token().start());
                    }
                    result.last_end = Some(tokens[token_idx].token().end());
                    Some(result)
                } else {
                    None
                }
            }
        }
    }

    fn match_token_element(
        &self,
        pattern_token: &og_xml::compiler::CompiledPatternToken,
        elements: &[og_xml::compiler::CompiledPatternElement],
        tokens: &[&AnalyzedTokenReadings],
        token_idx: usize,
        element_idx: usize,
        matched_texts: &[String],
    ) -> Option<PatternMatchResult> {
        let skip = pattern_token.skip;
        let min = pattern_token.min;
        let max = pattern_token.max;

        let token_ok = |pos: usize, mt: &[String]| -> bool {
            if let Some(br) = self.backreference_matches(pattern_token, tokens[pos], mt) {
                br && self.token_matches(pattern_token, tokens[pos], pos, tokens)
            } else {
                self.token_matches(pattern_token, tokens[pos], pos, tokens)
            }
        };

        if min == Some(0)
            && let Some(mut result) = self.match_elements_recursive(
                elements,
                tokens,
                token_idx,
                element_idx + 1,
                matched_texts,
            )
        {
            result.matched_positions.insert(0, token_idx);
            result.matched_texts.insert(0, String::new());
            return Some(result);
        }

        if skip != 0 {
            let max_skip = if skip < 0 {
                tokens.len()
            } else {
                skip as usize
            };
            for s in 0..=max_skip {
                let next_token = token_idx + s;
                if next_token < tokens.len() && token_ok(next_token, matched_texts) {
                    let mut new_mt = matched_texts.to_vec();
                    new_mt.push(tokens[next_token].token().token().to_string());
                    if let Some(mut result) = self.match_elements_recursive(
                        elements,
                        tokens,
                        next_token + 1,
                        element_idx + 1,
                        &new_mt,
                    ) {
                        result.matched_positions.insert(0, next_token);
                        result
                            .matched_texts
                            .insert(0, tokens[next_token].token().token().to_string());
                        if result.first_start.is_none() {
                            result.first_start = Some(tokens[next_token].token().start());
                        }
                        result.last_end = Some(tokens[next_token].token().end());
                        return Some(result);
                    }
                }
            }
            return None;
        }

        if min.is_some() || max.is_some() {
            let min_count = min.unwrap_or(1) as usize;
            let max_count =
                max.map(|m| m as usize)
                    .unwrap_or(if min_count > 0 { min_count } else { 1 });
            let mut matched_count = 0;
            let mut cur = token_idx;
            while matched_count < max_count && cur < tokens.len() {
                if token_ok(cur, matched_texts) {
                    matched_count += 1;
                    cur += 1;
                } else {
                    break;
                }
            }
            for count in (min_count..=matched_count).rev() {
                let mut new_mt = matched_texts.to_vec();
                for t in tokens.iter().take(token_idx + count).skip(token_idx) {
                    new_mt.push(t.token().token().to_string());
                }
                if let Some(mut result) = self.match_elements_recursive(
                    elements,
                    tokens,
                    token_idx + count,
                    element_idx + 1,
                    &new_mt,
                ) {
                    for i in (0..count).rev() {
                        result.matched_positions.insert(0, token_idx + i);
                        result
                            .matched_texts
                            .insert(0, tokens[token_idx + i].token().token().to_string());
                    }
                    if result.first_start.is_none() {
                        result.first_start = Some(tokens[token_idx].token().start());
                    }
                    result.last_end = Some(tokens[token_idx + count - 1].token().end());
                    return Some(result);
                }
            }
            return None;
        }

        if token_ok(token_idx, matched_texts) {
            let mut new_mt = matched_texts.to_vec();
            new_mt.push(tokens[token_idx].token().token().to_string());
            let mut result = self.match_elements_recursive(
                elements,
                tokens,
                token_idx + 1,
                element_idx + 1,
                &new_mt,
            )?;
            result.matched_positions.insert(0, token_idx);
            result
                .matched_texts
                .insert(0, tokens[token_idx].token().token().to_string());
            if result.first_start.is_none() {
                result.first_start = Some(tokens[token_idx].token().start());
            }
            result.last_end = Some(tokens[token_idx].token().end());
            Some(result)
        } else {
            None
        }
    }

    fn match_pattern_recursive(
        &self,
        pattern_tokens: &[og_xml::compiler::CompiledPatternToken],
        tokens: &[&AnalyzedTokenReadings],
        token_idx: usize,
        pattern_idx: usize,
        matched_texts: &[String],
    ) -> Option<PatternMatchResult> {
        if pattern_idx >= pattern_tokens.len() {
            return Some(PatternMatchResult {
                first_start: None,
                last_end: None,
                matched_positions: Vec::new(),
                matched_texts: Vec::new(),
            });
        }

        if token_idx >= tokens.len() {
            let remaining_ok: bool =
                (pattern_idx..pattern_tokens.len()).all(|i| pattern_tokens[i].min == Some(0));
            if remaining_ok {
                return Some(PatternMatchResult {
                    first_start: None,
                    last_end: None,
                    matched_positions: Vec::new(),
                    matched_texts: Vec::new(),
                });
            }
            return None;
        }

        let pattern_token = &pattern_tokens[pattern_idx];
        let text_token = tokens[token_idx];

        let matches = if let Some(br) =
            self.backreference_matches(pattern_token, text_token, matched_texts)
        {
            br && self.token_matches(pattern_token, text_token, token_idx, tokens)
        } else {
            self.token_matches(pattern_token, text_token, token_idx, tokens)
        };

        if matches {
            let token_start = text_token.token().start();
            let token_end = text_token.token().end();
            let token_text = text_token.token().token().to_string();

            let min_count = pattern_token.min.unwrap_or(1) as usize;
            let max_count = pattern_token.max.map(|m| m as usize).unwrap_or(1);
            let mut repeat_count = 1;
            let mut last_token_idx = token_idx;

            while repeat_count < max_count && last_token_idx + 1 < tokens.len() {
                let next_matches = if let Some(br) = self.backreference_matches(
                    pattern_token,
                    tokens[last_token_idx + 1],
                    matched_texts,
                ) {
                    br && self.token_matches(
                        pattern_token,
                        tokens[last_token_idx + 1],
                        last_token_idx + 1,
                        tokens,
                    )
                } else {
                    self.token_matches(
                        pattern_token,
                        tokens[last_token_idx + 1],
                        last_token_idx + 1,
                        tokens,
                    )
                };
                if next_matches {
                    last_token_idx += 1;
                    repeat_count += 1;
                } else {
                    break;
                }
            }

            if repeat_count < min_count {
                return None;
            }

            if min_count == 0
                && let Some(mut result) = self.match_pattern_recursive(
                    pattern_tokens,
                    tokens,
                    token_idx,
                    pattern_idx + 1,
                    matched_texts,
                )
            {
                result.matched_positions.insert(0, token_idx);
                result.matched_texts.insert(0, String::new());
                return Some(result);
            }

            let skip = pattern_token.skip;
            let max_skip = if skip < 0 {
                tokens.len()
            } else {
                skip as usize
            };

            let mut new_mt = matched_texts.to_vec();
            for (i, t) in tokens
                .iter()
                .enumerate()
                .take(last_token_idx.min(tokens.len() - 1) + 1)
                .skip(token_idx)
            {
                if i == token_idx {
                    new_mt.push(token_text.clone());
                } else {
                    new_mt.push(t.token().token().to_string());
                }
            }

            let next_scope_exceptions: Vec<&og_xml::compiler::CompiledException> = pattern_token
                .exceptions
                .iter()
                .filter(|exc| exc.scope.as_deref() == Some("next"))
                .collect();

            for next_offset in 0..=max_skip {
                let next_token_idx = last_token_idx + 1 + next_offset;
                if next_token_idx > tokens.len() && next_offset > 0 {
                    break;
                }
                if !next_scope_exceptions.is_empty()
                    && ((token_idx + 1)..next_token_idx.min(tokens.len() + 1)).any(|q| {
                        next_scope_exceptions
                            .iter()
                            .any(|exc| exc.negate ^ self.exception_matches(exc, tokens[q]))
                    })
                {
                    continue;
                }
                if let Some(mut rest) = self.match_pattern_recursive(
                    pattern_tokens,
                    tokens,
                    next_token_idx,
                    pattern_idx + 1,
                    &new_mt,
                ) {
                    let last_end = if last_token_idx < tokens.len() {
                        tokens[last_token_idx].token().end()
                    } else {
                        token_end
                    };

                    let mut positions = Vec::new();
                    let mut texts = Vec::new();
                    for (i, t) in tokens
                        .iter()
                        .enumerate()
                        .take(last_token_idx.min(tokens.len() - 1) + 1)
                        .skip(token_idx)
                    {
                        positions.push(i);
                        if i == token_idx {
                            texts.push(token_text.clone());
                        } else {
                            texts.push(t.token().token().to_string());
                        }
                    }

                    rest.matched_positions.splice(0..0, positions);
                    rest.matched_texts.splice(0..0, texts);
                    rest.first_start = Some(match rest.first_start {
                        Some(s) => std::cmp::min(s, token_start),
                        None => token_start,
                    });
                    rest.last_end = Some(match rest.last_end {
                        Some(e) => std::cmp::max(e, last_end),
                        None => last_end,
                    });
                    return Some(rest);
                }
            }

            if pattern_idx + 1 >= pattern_tokens.len() {
                let last_end = if last_token_idx < tokens.len() {
                    tokens[last_token_idx].token().end()
                } else {
                    token_end
                };
                let mut positions = Vec::new();
                let mut texts = Vec::new();
                for (i, t) in tokens
                    .iter()
                    .enumerate()
                    .take(last_token_idx.min(tokens.len() - 1) + 1)
                    .skip(token_idx)
                {
                    positions.push(i);
                    if i == token_idx {
                        texts.push(token_text.clone());
                    } else {
                        texts.push(t.token().token().to_string());
                    }
                }
                return Some(PatternMatchResult {
                    first_start: Some(token_start),
                    last_end: Some(last_end),
                    matched_positions: positions,
                    matched_texts: texts,
                });
            }

            None
        } else if pattern_token.min == Some(0) {
            let mut result = self.match_pattern_recursive(
                pattern_tokens,
                tokens,
                token_idx,
                pattern_idx + 1,
                matched_texts,
            )?;
            result.matched_positions.insert(0, token_idx);
            result.matched_texts.insert(0, String::new());
            Some(result)
        } else {
            None
        }
    }

    fn antipattern_first_literal(
        &self,
        antipattern: &og_xml::compiler::CompiledPattern,
    ) -> Option<(String, bool)> {
        if antipattern.has_structured_elements {
            return None;
        }
        let first = antipattern.tokens.first()?;
        if first.negate
            || first.inflected
            || first.min == Some(0)
            || first.skip != 0
            || first.compiled_regexp.is_some()
        {
            return None;
        }
        let text = first.text.as_ref()?;
        let norm = normalize_apostrophes(text);
        Some((norm.into_owned(), first.case_sensitive))
    }

    fn antipattern_matches_with_unification(
        &self,
        antipattern: &og_xml::compiler::CompiledPattern,
        tokens: &[&AnalyzedTokenReadings],
        matched_positions: &[usize],
        unifications: &[og_xml::compiler::CompiledUnification],
    ) -> bool {
        if antipattern.tokens.is_empty() && antipattern.elements.is_empty() {
            return false;
        }
        if matched_positions.is_empty() {
            let has_structured = antipattern.has_structured_elements;
            let first_literal = self.antipattern_first_literal(antipattern);
            for pos in 0..tokens.len() {
                if let Some((lit, cs)) = &first_literal {
                    let text = normalize_apostrophes(tokens[pos].token().token());
                    let hit = if *cs {
                        text == lit.as_str()
                    } else {
                        text.eq_ignore_ascii_case(lit)
                    };
                    if !hit {
                        continue;
                    }
                }
                let result = if has_structured {
                    self.match_pattern_elements(&antipattern.elements, tokens, pos)
                } else {
                    self.match_pattern_tokens(&antipattern.tokens, tokens, pos)
                };
                if let Some(anti_result) = result
                    && antipattern_unify_satisfied(
                        unifications,
                        antipattern,
                        &anti_result.matched_positions,
                        tokens,
                    )
                {
                    return true;
                }
            }
            return false;
        }

        let has_structured = antipattern.has_structured_elements;

        let match_min = *matched_positions.iter().min().unwrap();
        let match_max = *matched_positions.iter().max().unwrap();

        let first_literal = self.antipattern_first_literal(antipattern);
        for pos in 0..tokens.len() {
            if let Some((lit, cs)) = &first_literal {
                let text = normalize_apostrophes(tokens[pos].token().token());
                let hit = if *cs {
                    text == lit.as_str()
                } else {
                    text.eq_ignore_ascii_case(lit)
                };
                if !hit {
                    continue;
                }
            }
            let result = if has_structured {
                self.match_pattern_elements(&antipattern.elements, tokens, pos)
            } else {
                self.match_pattern_tokens(&antipattern.tokens, tokens, pos)
            };
            if let Some(anti_result) = result {
                if !antipattern_unify_satisfied(
                    unifications,
                    antipattern,
                    &anti_result.matched_positions,
                    tokens,
                ) {
                    continue;
                }
                let anti_min = anti_result
                    .matched_positions
                    .first()
                    .copied()
                    .unwrap_or(usize::MAX);
                let anti_max = anti_result.matched_positions.last().copied().unwrap_or(0);
                if anti_min <= match_max && anti_max >= match_min {
                    return true;
                }
            }
        }
        false
    }

    fn resolve_backreferences(&self, text: &str, matched_texts: &[String]) -> String {
        let mut result = String::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                let mut num_str = String::new();
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    num_str.push(chars[j]);
                    j += 1;
                }
                if !num_str.is_empty() {
                    if let Ok(idx) = num_str.parse::<usize>() {
                        if idx > 0 && idx <= matched_texts.len() {
                            result.push_str(&matched_texts[idx - 1]);
                        } else if idx == 0 && !matched_texts.is_empty() {
                            result.push_str(&matched_texts.join(" "));
                        }
                    }
                    i = j;
                    continue;
                }
            }
            result.push(chars[i]);
            i += 1;
        }

        result
    }

    fn token_matches(
        &self,
        pattern: &og_xml::compiler::CompiledPatternToken,
        token: &AnalyzedTokenReadings,
        token_idx: usize,
        tokens: &[&AnalyzedTokenReadings],
    ) -> bool {
        let text_matches = if let Some(ref text) = pattern.text {
            let token_text = normalize_apostrophes(token.token().token());
            let norm_text = normalize_apostrophes(text);
            let surface_matches = if pattern.case_sensitive {
                token_text == norm_text
            } else {
                token_text.eq_ignore_ascii_case(&norm_text)
            };

            if pattern.inflected {
                let lemma_matches = if surface_matches {
                    true
                } else {
                    token.readings().iter().any(|r| {
                        if let Some(lemma) = r.lemma() {
                            if pattern.case_sensitive {
                                lemma == text
                            } else {
                                lemma.eq_ignore_ascii_case(text)
                            }
                        } else {
                            false
                        }
                    })
                };
                if pattern.negate {
                    !lemma_matches
                } else {
                    lemma_matches
                }
            } else {
                if pattern.negate {
                    !surface_matches
                } else {
                    surface_matches
                }
            }
        } else if let Some(ref re) = pattern.compiled_regexp_fancy {
            let token_text = normalize_apostrophes(token.token().token());
            re.is_match(&token_text).unwrap_or_default()
        } else if let Some(ref re) = pattern.compiled_regexp {
            let token_text = normalize_apostrophes(token.token().token());
            let surface_matches = if pattern.case_sensitive {
                re.is_match(&token_text)
            } else if let Some(ref ci_re) = pattern.compiled_regexp_ci {
                ci_re.is_match(&token_text)
            } else {
                re.is_match(&token_text)
            };

            let matches = if pattern.inflected && !surface_matches {
                token.readings().iter().any(|r| {
                    if let Some(lemma) = r.lemma() {
                        if pattern.case_sensitive {
                            re.is_match(lemma)
                        } else if let Some(ref ci_re) = pattern.compiled_regexp_ci {
                            ci_re.is_match(lemma)
                        } else {
                            re.is_match(lemma)
                        }
                    } else {
                        false
                    }
                })
            } else {
                surface_matches
            };
            if pattern.negate { !matches } else { matches }
        } else {
            true
        };

        if !text_matches {
            return false;
        }

        if let Some(ref postag) = pattern.postag {
            if let Some(ref postag_re) = pattern.compiled_postag_regexp {
                let pos_matches = token.has_pos_tag_matching_regex(postag_re)
                    || (postag == "UNKNOWN"
                        && postag_re.is_match("UNKNOWN")
                        && token.all_pos_tags().is_empty());
                if pattern.negate_pos {
                    if pos_matches {
                        return false;
                    }
                } else {
                    if !pos_matches {
                        return false;
                    }
                }
            } else {
                let pos_matches = if postag == "UNKNOWN" {
                    token.all_pos_tags().is_empty()
                } else {
                    token.has_pos_tag(postag)
                };
                if pattern.negate_pos {
                    if pos_matches {
                        return false;
                    }
                } else {
                    if !pos_matches {
                        return false;
                    }
                }
            }
        }

        if let Some(ref chunk) = pattern.chunk
            && !token.has_chunk(chunk)
        {
            return false;
        }
        if let Some(ref chunk_re) = pattern.chunk_re
            && !token.has_chunk_matching(chunk_re)
        {
            return false;
        }

        if let Some(ref sb) = pattern.space_before
            && token_idx > 0
        {
            let prev_end = tokens[token_idx - 1].token().end();
            let cur_start = token.token().start();
            if sb == "no" {
                if cur_start != prev_end {
                    return false;
                }
            } else if sb == "yes" && cur_start == prev_end {
                return false;
            }
        }

        for exc in &pattern.exceptions {
            let exc_matches = match exc.scope.as_deref() {
                Some("next") if pattern.skip == 0 => {
                    if token_idx + 1 < tokens.len() {
                        self.exception_matches(exc, tokens[token_idx + 1])
                    } else {
                        false
                    }
                }
                Some("next") => false,
                Some("previous") => {
                    if token_idx > 0 {
                        self.exception_matches(exc, tokens[token_idx - 1])
                    } else {
                        false
                    }
                }
                _ => self.exception_matches(exc, token),
            };

            if exc_matches && !exc.negate {
                return false;
            }
            if !exc_matches && exc.negate {
                return false;
            }
        }

        true
    }

    fn exception_matches(
        &self,
        exc: &og_xml::compiler::CompiledException,
        token: &AnalyzedTokenReadings,
    ) -> bool {
        let token_text = normalize_apostrophes(token.token().token());
        let mut matched = true;
        let mut has_constraint = false;

        if exc.compiled_regexp.is_some()
            || exc.compiled_regexp_fancy.is_some()
            || exc.text.is_some()
        {
            has_constraint = true;
            matched &= match (&exc.compiled_regexp, &exc.compiled_regexp_fancy) {
                (Some(re), _) => re.is_match(&token_text),
                (None, Some(re)) => re.is_match(&token_text).unwrap_or(false),
                (None, None) => {
                    let text = exc.text.as_ref().expect("checked above");
                    let norm_text = normalize_apostrophes(text);
                    let surface_matches = if exc.case_sensitive {
                        token_text == norm_text
                    } else {
                        token_text.eq_ignore_ascii_case(norm_text.as_ref())
                    };
                    if exc.inflected {
                        surface_matches
                            || token.readings().iter().any(|r| {
                                if let Some(lemma) = r.lemma() {
                                    if exc.case_sensitive {
                                        lemma == text
                                    } else {
                                        lemma.eq_ignore_ascii_case(text)
                                    }
                                } else {
                                    false
                                }
                            })
                    } else {
                        surface_matches
                    }
                }
            };
        }

        if let Some(postag) = exc.postag.as_deref() {
            has_constraint = true;
            let pos_matches = if postag == "UNKNOWN" && exc.compiled_postag_regexp.is_none() {
                token.all_pos_tags().is_empty()
            } else if let Some(ref postag_re) = exc.compiled_postag_regexp {
                token.has_pos_tag_matching_regex(postag_re)
            } else {
                token.has_pos_tag(postag)
            };
            matched &= if exc.negate_pos {
                !pos_matches
            } else {
                pos_matches
            };
        }

        has_constraint && matched
    }
}

struct PatternMatchResult {
    first_start: Option<usize>,
    last_end: Option<usize>,
    matched_positions: Vec<usize>,
    matched_texts: Vec<String>,
}

fn unify_groups_satisfied(
    rule: &CompiledRule,
    match_result: &PatternMatchResult,
    tokens: &[&AnalyzedTokenReadings],
) -> bool {
    for group in &rule.pattern.unify_groups {
        let matched = unify_group_matches(
            &rule.unifications,
            &rule.pattern,
            group,
            &match_result.matched_positions,
            tokens,
        );
        if matched == group.negate {
            return false;
        }
    }
    true
}

fn antipattern_unify_satisfied(
    unifications: &[og_xml::compiler::CompiledUnification],
    antipattern: &og_xml::compiler::CompiledPattern,
    matched_positions: &[usize],
    tokens: &[&AnalyzedTokenReadings],
) -> bool {
    for group in &antipattern.unify_groups {
        let matched =
            unify_group_matches(unifications, antipattern, group, matched_positions, tokens);
        if matched == group.negate {
            return false;
        }
    }
    true
}

fn unify_group_matches(
    unifications: &[og_xml::compiler::CompiledUnification],
    rule_pattern: &og_xml::compiler::CompiledPattern,
    group: &og_xml::types::XmlUnifyGroup,
    matched_positions: &[usize],
    tokens: &[&AnalyzedTokenReadings],
) -> bool {
    let mut common: Option<std::collections::HashMap<String, std::collections::HashSet<String>>> =
        None;
    let mut group_len = 0usize;
    for pattern_token in group.token_start..group.token_end {
        group_len += 1;
        let Some(pattern) = rule_pattern.tokens.get(pattern_token) else {
            return false;
        };
        let Some(&sentence_token) = matched_positions.get(pattern_token) else {
            return false;
        };
        let Some(token) = tokens.get(sentence_token) else {
            return false;
        };
        if pattern.unify_neutral {
            continue;
        }
        let mut token_types: std::collections::HashMap<String, std::collections::HashSet<String>> =
            std::collections::HashMap::new();
        for reading in token.readings() {
            let reading_tags: Vec<&str> = reading.pos_tags().iter().map(String::as_str).collect();
            if !reading_matches_postag(pattern, &reading_tags) {
                continue;
            }
            if !reading_satisfies_features(unifications, group, &reading_tags) {
                continue;
            }
            for (feature, types) in reading_feature_types(unifications, group, &reading_tags) {
                token_types
                    .entry(feature)
                    .or_default()
                    .extend(types.iter().cloned());
            }
        }
        if token_types.is_empty() {
            return false;
        }
        common = Some(match common {
            None => token_types,
            Some(existing) => {
                let mut merged = std::collections::HashMap::new();
                for (feature, types) in existing {
                    if let Some(other) = token_types.get(&feature) {
                        let intersection: std::collections::HashSet<String> =
                            types.intersection(other).cloned().collect();
                        merged.insert(feature, intersection);
                    }
                }
                merged
            }
        });
        if common
            .as_ref()
            .is_some_and(|c| c.values().any(|v| v.is_empty()))
        {
            return false;
        }
    }
    let _ = group_len;
    common
        .as_ref()
        .is_some_and(|c| !c.is_empty() && c.values().all(|v| !v.is_empty()))
}

fn reading_matches_postag(pattern: &og_xml::compiler::CompiledPatternToken, tags: &[&str]) -> bool {
    let Some(postag) = pattern.postag.as_deref() else {
        return true;
    };
    let matches = match &pattern.compiled_postag_regexp {
        Some(re) => tags.iter().any(|t| re.is_match(t)),
        None => tags.contains(&postag),
    };
    if pattern.negate_pos {
        !matches
    } else {
        matches
    }
}

fn reading_feature_types(
    unifications: &[og_xml::compiler::CompiledUnification],
    group: &og_xml::types::XmlUnifyGroup,
    tags: &[&str],
) -> Vec<(String, Vec<String>)> {
    let relevant: Vec<&og_xml::compiler::CompiledUnification> = if group.features.is_empty() {
        unifications.iter().collect()
    } else {
        group
            .features
            .iter()
            .filter_map(|(name, _)| unifications.iter().find(|u| &u.feature == name))
            .collect()
    };
    relevant
        .iter()
        .map(|unification| {
            let allowed: Option<&Vec<String>> = group
                .features
                .iter()
                .find(|(name, _)| name == &unification.feature)
                .map(|(_, types)| types);
            let matched: Vec<String> = unification
                .equivalences
                .iter()
                .filter(|(eq_type, re)| {
                    allowed.is_none_or(|types| types.is_empty() || types.contains(eq_type))
                        && tags.iter().any(|t| re.is_match(t))
                })
                .map(|(eq_type, _)| eq_type.clone())
                .collect();
            (unification.feature.clone(), matched)
        })
        .collect()
}

fn reading_satisfies_features(
    unifications: &[og_xml::compiler::CompiledUnification],
    group: &og_xml::types::XmlUnifyGroup,
    tags: &[&str],
) -> bool {
    let relevant: Vec<&og_xml::compiler::CompiledUnification> = if group.features.is_empty() {
        unifications.iter().collect()
    } else {
        group
            .features
            .iter()
            .filter_map(|(name, _)| unifications.iter().find(|u| &u.feature == name))
            .collect()
    };
    if relevant.is_empty() {
        return false;
    }
    for unification in relevant {
        let allowed: Option<&Vec<String>> = group
            .features
            .iter()
            .find(|(name, _)| name == &unification.feature)
            .map(|(_, types)| types);
        let matched = unification
            .equivalences
            .iter()
            .filter(|(eq_type, re)| {
                allowed.is_none_or(|types| types.is_empty() || types.contains(eq_type))
                    && tags.iter().any(|t| re.is_match(t))
            })
            .count();
        if matched == 0 {
            return false;
        }
    }
    true
}

fn uppercase_first_char(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        out.push_str(&first.to_uppercase().to_string());
        out.push_str(chars.as_str());
    }
    out
}

fn equal_without_diacritics(a: &str, b: &str) -> bool {
    let fold = |s: &str| {
        s.chars()
            .flat_map(|c| c.to_lowercase())
            .map(|c| match c {
                'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                'í' | 'ì' | 'î' | 'ï' => 'i',
                'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
                'ú' | 'ù' | 'û' | 'ü' => 'u',
                'ñ' => 'n',
                'ç' => 'c',
                'ý' | 'ÿ' => 'y',
                other => other,
            })
            .collect::<String>()
    };
    fold(a) == fold(b)
}

impl Default for PatternRuleEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternRuleEngine {
    fn apply_filter(
        &self,
        filter: &og_xml::types::XmlFilter,
        matched_texts: &[String],
        _matched_positions: &[usize],
        _tokens: &[&AnalyzedTokenReadings],
        sentence: &AnalyzedSentence,
        rm: RuleMatch,
    ) -> Option<RuleMatch> {
        match filter.class.as_str() {
            "org.languagetool.rules.en.FutureDateFilter" => {
                if self.apply_future_date_filter(&filter.args, matched_texts) {
                    Some(rm)
                } else {
                    None
                }
            }
            "org.languagetool.rules.en.DateCheckFilter" => {
                if self.apply_date_check_filter(&filter.args, matched_texts) {
                    Some(rm)
                } else {
                    None
                }
            }
            "org.languagetool.rules.en.NewYearDateFilter"
            | "org.languagetool.rules.en.YMDNewYearDateFilter" => {
                if self.apply_new_year_date_filter(&filter.args, matched_texts) {
                    Some(rm)
                } else {
                    None
                }
            }
            "org.languagetool.rules.en.EnglishSuppressMisspelledSuggestionsFilter" => {
                if filter.args.contains("suppressMatch:true") {
                    None
                } else {
                    Some(rm)
                }
            }
            "org.languagetool.rules.en.EnglishNumberInWordFilter" => {
                self.apply_number_in_word_filter(&filter.args, matched_texts, rm)
            }
            "org.languagetool.rules.en.FindSuggestionsFilter" => {
                self.apply_find_suggestions_filter(&filter.args, matched_texts, rm)
            }
            "org.languagetool.rules.en.AdverbFilter" => {
                self.apply_adverb_filter(&filter.args, matched_texts, rm)
            }
            "org.languagetool.rules.en.OrdinalSuffixFilter" => self.apply_ordinal_suffix_filter(rm),
            "org.languagetool.rules.UnderlineSpacesFilter" => {
                self.apply_underline_spaces_filter(&filter.args, sentence, rm)
            }
            "org.languagetool.rules.spelling.multitoken.MultitokenSpellerFilter" => {
                self.apply_multitoken_speller_filter(matched_texts, rm)
            }
            "org.languagetool.rules.DateRangeChecker" => {
                if self.apply_date_range_filter(&filter.args, matched_texts) {
                    Some(rm)
                } else {
                    None
                }
            }
            "org.languagetool.rules.patterns.RegexAntiPatternFilter" => {
                if self.apply_regex_antipattern_filter(&filter.args, matched_texts) {
                    Some(rm)
                } else {
                    None
                }
            }
            "org.languagetool.rules.patterns.ApostropheTypeFilter" => {
                if self.apply_apostrophe_type_filter(&filter.args, matched_texts) {
                    Some(rm)
                } else {
                    None
                }
            }
            _ => Some(rm),
        }
    }

    fn parse_filter_args(&self, args: &str, matched_texts: &[String]) -> HashMap<String, String> {
        let mut result = HashMap::new();
        for part in args.split_whitespace() {
            if let Some((key, val)) = part.split_once(':') {
                let resolved = if let Some(rest) = val.strip_prefix('\\') {
                    if let Ok(idx) = rest.parse::<usize>() {
                        if idx > 0 && idx <= matched_texts.len() {
                            matched_texts[idx - 1].clone()
                        } else {
                            val.to_string()
                        }
                    } else {
                        val.to_string()
                    }
                } else if let Ok(idx) = val.parse::<usize>() {
                    if idx > 0 && idx <= matched_texts.len() {
                        matched_texts[idx - 1].clone()
                    } else {
                        val.to_string()
                    }
                } else {
                    val.to_string()
                };
                result.insert(key.to_string(), resolved);
            }
        }
        result
    }

    fn apply_future_date_filter(&self, args: &str, matched_texts: &[String]) -> bool {
        let parsed = self.parse_filter_args(args, matched_texts);
        let year_str = parsed.get("year").map(|s| s.as_str()).unwrap_or("0");
        let month_str = parsed.get("month").map(|s| s.as_str()).unwrap_or("1");
        let day_str = parsed.get("day").map(|s| s.as_str()).unwrap_or("1");

        let year = parse_date_number(year_str);
        let month = parse_month(month_str);
        let day = parse_date_number(day_str);

        if year == 0 || month == 0 {
            return false;
        }

        let current_date = current_date();
        (year, month, day) > current_date
    }

    fn apply_date_check_filter(&self, args: &str, matched_texts: &[String]) -> bool {
        let parsed = self.parse_filter_args(args, matched_texts);
        let week_day_str = parsed.get("weekDay").map(|s| s.as_str()).unwrap_or("");

        let (year, month, day) = if let Some(date_str) = parsed.get("date") {
            let parts: Vec<&str> = date_str.split('-').collect();
            if parts.len() == 3 {
                (
                    parse_date_number(parts[0]),
                    parse_date_number(parts[1]),
                    parse_date_number(parts[2]),
                )
            } else {
                return false;
            }
        } else {
            let day_str = parsed.get("day").map(|s| s.as_str()).unwrap_or("1");
            let month_str = parsed.get("month").map(|s| s.as_str()).unwrap_or("1");
            let y = parsed
                .get("year")
                .map(|s| parse_date_number(s))
                .unwrap_or_else(|| current_date().0);
            (y, parse_month(month_str), parse_date_number(day_str))
        };

        let claimed_weekday = parse_weekday(week_day_str);

        if claimed_weekday == 0 || day == 0 || month == 0 {
            return false;
        }

        let actual_weekday = compute_weekday(year, month, day);
        claimed_weekday != actual_weekday
    }

    fn apply_new_year_date_filter(&self, args: &str, matched_texts: &[String]) -> bool {
        let parsed = self.parse_filter_args(args, matched_texts);
        let year_str = parsed.get("year").map(|s| s.as_str()).unwrap_or("0");
        let year = parse_date_number(year_str);

        let current_date = current_date();
        year != current_date.0 && year > 0
    }

    fn apply_date_range_filter(&self, args: &str, matched_texts: &[String]) -> bool {
        let parsed = self.parse_filter_args(args, matched_texts);
        let x = parsed
            .get("x")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        let y = parsed
            .get("y")
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(0);
        x >= y
    }

    fn apply_regex_antipattern_filter(&self, args: &str, matched_texts: &[String]) -> bool {
        let parsed = self.parse_filter_args(args, matched_texts);
        if let Some(patterns) = parsed.get("antipatterns") {
            for pattern in patterns.split('|') {
                if let Ok(re) = regex::Regex::new(pattern) {
                    for text in matched_texts {
                        if re.is_match(text) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn apply_apostrophe_type_filter(&self, args: &str, matched_texts: &[String]) -> bool {
        let parsed = self.parse_filter_args(args, matched_texts);
        let wants_typographic = parsed
            .get("hasTypographicalApostrophe")
            .map(|v| v == "true")
            .unwrap_or(false);

        if let Some(word) = parsed.get("wordFrom") {
            let has_typographic = word.contains('\u{2019}');
            let has_typewriter = word.contains('\'');
            if wants_typographic {
                has_typographic && !has_typewriter
            } else {
                has_typewriter || !has_typographic
            }
        } else {
            true
        }
    }

    fn apply_number_in_word_filter(
        &self,
        args: &str,
        matched_texts: &[String],
        rm: RuleMatch,
    ) -> Option<RuleMatch> {
        let Some(spelling) = self.spelling.as_ref() else {
            return Some(rm);
        };
        let parsed = self.parse_filter_args(args, matched_texts);
        let Some(word) = parsed.get("word") else {
            return Some(rm);
        };
        let word_replacing_zero_o = word.replace('0', "o");
        let word_without_number: String = word.chars().filter(|c| !c.is_ascii_digit()).collect();

        let mut replacements: Vec<String> = Vec::new();
        if !spelling.is_misspelled(&word_replacing_zero_o) && word != &word_replacing_zero_o {
            replacements.push(word_replacing_zero_o);
        }
        if !spelling.is_misspelled(&word_without_number) {
            replacements.push(word_without_number.clone());
        }
        if replacements.is_empty() {
            replacements = spelling.suggestions(&word_without_number, 10);
        }
        if replacements.is_empty() {
            return None;
        }
        Some(
            rm.with_replacements(
                replacements
                    .into_iter()
                    .map(SuggestedReplacement::new)
                    .collect(),
            ),
        )
    }

    fn apply_find_suggestions_filter(
        &self,
        args: &str,
        matched_texts: &[String],
        rm: RuleMatch,
    ) -> Option<RuleMatch> {
        const MAX_SUGGESTIONS: usize = 10;
        let Some(spelling) = self.spelling.as_ref() else {
            return Some(rm);
        };
        let parsed = self.parse_filter_args(args, matched_texts);
        let Some(word_from) = parsed.get("wordFrom") else {
            return Some(rm);
        };
        let Some(desired_postag) = parsed.get("desiredPostag") else {
            return Some(rm);
        };
        let priority_postag = parsed.get("priorityPostag");
        let remove_regexp = parsed.get("removeSuggestionsRegexp");
        let suppress_match = parsed
            .get("suppressMatch")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let diacritics_mode = parsed.get("Mode").is_some_and(|m| m == "diacritics");

        let word = if word_from == "marker" {
            matched_texts.last().cloned().unwrap_or_default()
        } else {
            word_from.clone()
        };
        if word.is_empty() {
            return Some(rm);
        }

        let desired_re = regex::Regex::new(&format!("^(?:{desired_postag})$")).ok();
        let priority_re =
            priority_postag.and_then(|p| regex::Regex::new(&format!("^(?:{p})$")).ok());
        let remove_re = remove_regexp.and_then(|r| regex::Regex::new(&format!("^(?:{r})$")).ok());

        let matches_desired = |tags: &[&str]| match &desired_re {
            Some(re) => tags.iter().any(|t| re.is_match(t)),
            None => tags.iter().any(|t| t == desired_postag),
        };

        let original_readings = spelling.tag(&word);
        let original_tags: Vec<&str> = original_readings
            .iter()
            .flat_map(|t| t.pos_tags().iter().map(String::as_str).collect::<Vec<_>>())
            .collect();
        if matches_desired(&original_tags) && diacritics_mode {
            return None;
        }

        let is_word_capitalized = word.chars().next().is_some_and(|c| c.is_uppercase())
            && !word.chars().skip(1).any(|c| c.is_uppercase());
        let is_word_all_upper = !word.chars().any(|c| c.is_lowercase());

        let mut replacements: Vec<String> = Vec::new();
        let mut used_priority_pos = 0;
        for suggestion in spelling.suggestions(&word.to_lowercase(), 2 * MAX_SUGGESTIONS) {
            if replacements.len() >= 2 * MAX_SUGGESTIONS {
                break;
            }
            if suggestion == word {
                continue;
            }
            let clean = suggestion.to_lowercase();
            let suggestion_readings = spelling.tag(&clean);
            let suggestion_tags: Vec<&str> = suggestion_readings
                .iter()
                .flat_map(|t| t.pos_tags().iter().map(String::as_str).collect::<Vec<_>>())
                .collect();
            if !matches_desired(&suggestion_tags) {
                continue;
            }
            if diacritics_mode && !equal_without_diacritics(&suggestion, &word) {
                continue;
            }
            if remove_re
                .as_ref()
                .is_some_and(|re| re.is_match(&suggestion))
            {
                continue;
            }
            if replacements.contains(&suggestion) || replacements.contains(&clean) {
                continue;
            }
            let mut replacement = suggestion;
            if is_word_all_upper {
                replacement = replacement.to_uppercase();
            }
            if is_word_capitalized {
                replacement = uppercase_first_char(&replacement);
            }
            if priority_re
                .as_ref()
                .is_some_and(|re| suggestion_tags.iter().any(|t| re.is_match(t)))
            {
                replacements.insert(used_priority_pos, replacement);
                used_priority_pos += 1;
            } else {
                replacements.push(replacement);
            }
        }

        let existing: Vec<String> = rm
            .replacements()
            .iter()
            .map(|r| r.value().to_string())
            .collect();
        let match_has_finished_suggestion = existing
            .iter()
            .any(|s| !s.to_lowercase().contains("{suggestion}"));
        if replacements.is_empty() && diacritics_mode && !match_has_finished_suggestion {
            return None;
        }
        if replacements.is_empty() && suppress_match && !match_has_finished_suggestion {
            return None;
        }

        let mut definitive: Vec<String> = Vec::new();
        let mut replacements_used = false;
        for s in &existing {
            if s.contains("{suggestion}")
                || s.contains("{Suggestion}")
                || s.contains("{SUGGESTION}")
            {
                replacements_used = true;
                for s2 in &replacements {
                    if definitive.len() >= MAX_SUGGESTIONS {
                        break;
                    }
                    let candidate = if s.contains("{suggestion}") {
                        s.replace("{suggestion}", s2)
                    } else if s.contains("{Suggestion}") {
                        s.replace("{Suggestion}", &uppercase_first_char(s2))
                    } else {
                        s.replace("{SUGGESTION}", &s2.to_uppercase())
                    };
                    if !definitive.contains(&candidate) {
                        definitive.push(candidate);
                    }
                }
            } else if !definitive.contains(s) {
                definitive.push(s.clone());
            }
        }
        if !replacements_used {
            for replacement in replacements {
                if definitive.len() >= MAX_SUGGESTIONS {
                    break;
                }
                if !definitive.contains(&replacement) {
                    definitive.push(replacement);
                }
            }
        }
        Some(
            rm.with_replacements(
                definitive
                    .into_iter()
                    .map(SuggestedReplacement::new)
                    .collect(),
            ),
        )
    }

    fn apply_multitoken_speller_filter(
        &self,
        matched_texts: &[String],
        rm: RuleMatch,
    ) -> Option<RuleMatch> {
        let Some(spelling) = self.spelling.as_ref() else {
            return Some(rm);
        };
        let underlined_error: String = matched_texts.concat();
        if underlined_error.is_empty() {
            return None;
        }
        let are_tokens_accepted_by_speller = !underlined_error
            .split_whitespace()
            .any(|w| spelling.is_misspelled(w));
        let replacements =
            spelling.multitoken_suggestions(&underlined_error, are_tokens_accepted_by_speller);
        if replacements.is_empty() {
            return None;
        }
        let replacements = if underlined_error.chars().count() > 4
            && !underlined_error.chars().any(|c| c.is_lowercase())
        {
            let upper: Vec<String> = replacements
                .iter()
                .map(|r| r.to_uppercase())
                .filter(|r| *r != underlined_error)
                .collect();
            let mut deduped: Vec<String> = Vec::with_capacity(upper.len());
            for r in upper {
                if !deduped.contains(&r) {
                    deduped.push(r);
                }
            }
            deduped
        } else {
            replacements
        };
        if replacements.is_empty() {
            return None;
        }
        Some(
            rm.with_replacements(
                replacements
                    .into_iter()
                    .map(SuggestedReplacement::new)
                    .collect(),
            ),
        )
    }

    fn apply_ordinal_suffix_filter(&self, mut rm: RuleMatch) -> Option<RuleMatch> {
        let replacements = rm.replacements().to_vec();
        if let Some(first) = replacements.first() {
            let digits: String = first
                .value()
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect();
            if digits.is_empty() {
                return Some(rm);
            }
            let suffix =
                if digits.ends_with("11") || digits.ends_with("12") || digits.ends_with("13") {
                    "th"
                } else if digits.ends_with('1') {
                    "st"
                } else if digits.ends_with('2') {
                    "nd"
                } else if digits.ends_with('3') {
                    "rd"
                } else {
                    "th"
                };
            let new_suggestion = format!("{}{}", digits, suffix);
            let new_replacements = vec![SuggestedReplacement::new(&new_suggestion)];
            rm = rm.with_replacements(new_replacements);
        }
        Some(rm)
    }

    fn apply_underline_spaces_filter(
        &self,
        args: &str,
        sentence: &AnalyzedSentence,
        rm: RuleMatch,
    ) -> Option<RuleMatch> {
        let parsed = self.parse_filter_args(args, &[]);
        let mode = parsed
            .get("underlineSpaces")
            .map(|s| s.as_str())
            .unwrap_or("both");
        let text = sentence.text();
        let mut offset = rm.offset();
        let mut end = rm.offset() + rm.length();

        if (mode == "before" || mode == "both")
            && offset > 0
            && text
                .as_bytes()
                .get(offset - 1)
                .map(|&b| b.is_ascii_whitespace())
                .unwrap_or(false)
        {
            offset -= 1;
        }
        if (mode == "after" || mode == "both")
            && end + 1 < text.len()
            && text
                .as_bytes()
                .get(end)
                .map(|&b| b.is_ascii_whitespace())
                .unwrap_or(false)
        {
            end += 1;
        }

        Some(rm.with_offset(offset).with_length(end - offset))
    }

    fn apply_adverb_filter(
        &self,
        args: &str,
        matched_texts: &[String],
        rm: RuleMatch,
    ) -> Option<RuleMatch> {
        let parsed = self.parse_filter_args(args, matched_texts);
        let adverb = parsed.get("adverb").map(|s| s.as_str()).unwrap_or("");
        let noun = parsed.get("noun").map(|s| s.as_str()).unwrap_or("");

        let adjective = adverb_to_adjective(adverb);
        if let Some(adj) = adjective
            && adj != adverb
        {
            let new_suggestion = format!("{} {}", adj, noun);
            return Some(rm.with_replacements(vec![SuggestedReplacement::new(&new_suggestion)]));
        }
        Some(rm)
    }
}

fn parse_date_number(s: &str) -> u32 {
    let trimmed = s.trim().trim_end_matches(|c: char| c.is_alphabetic());
    trimmed.parse().unwrap_or(0)
}

fn adverb_to_adjective(adverb: &str) -> Option<&'static str> {
    Some(match adverb {
        "well" => "good",
        "fast" => "fast",
        "hard" => "hard",
        "late" => "late",
        "early" => "early",
        "daily" => "daily",
        "straight" => "straight",
        "simply" => "simple",
        "cheaply" => "cheap",
        "quickly" => "quick",
        "slowly" => "slow",
        "easily" => "easy",
        "angrily" => "angry",
        "happily" => "happy",
        "luckily" => "lucky",
        "terribly" => "terrible",
        "tragically" => "tragic",
        "economically" => "economic",
        "greatly" => "great",
        "highly" => "high",
        "generally" => "general",
        "differently" => "different",
        "rightly" => "right",
        "largely" => "large",
        "really" => "real",
        "directly" => "direct",
        "clearly" => "clear",
        "merely" => "mere",
        "exactly" => "exact",
        "recently" => "recent",
        "rapidly" => "rapid",
        "suddenly" => "sudden",
        "extremely" => "extreme",
        "properly" => "proper",
        "politically" => "political",
        "probably" => "probable",
        "successfully" => "successful",
        "unusually" => "unusual",
        "obviously" => "obvious",
        "currently" => "current",
        "fully" => "full",
        "accidentally" => "accidental",
        "automatically" => "automatic",
        "completely" => "complete",
        "accurately" => "accurate",
        "necessarily" => "necessary",
        "temporarily" => "temporary",
        "significantly" => "significant",
        "hastily" => "hasty",
        "immediately" => "immediate",
        "rarely" => "rare",
        "totally" => "total",
        "literally" => "literal",
        "gently" => "gentle",
        "finally" => "final",
        "increasingly" => "increasing",
        "considerably" => "considerable",
        "effectively" => "effective",
        "briefly" => "brief",
        "physically" => "physical",
        "enthusiastically" => "enthusiastic",
        "incredibly" => "incredible",
        "permanently" => "permanent",
        "entirely" => "entire",
        "surely" => "sure",
        "positively" => "positive",
        "negatively" => "negative",
        "relatively" => "relative",
        "absolutely" => "absolute",
        "socially" => "social",
        "solely" => "sole",
        "fortunately" => "fortunate",
        "unfortunately" => "unfortunate",
        "ideally" => "ideal",
        "privately" => "private",
        "unreasonably" => "unreasonable",
        "personally" => "personal",
        "basically" => "basic",
        "definitely" => "definite",
        "potentially" => "potential",
        "manually" => "manual",
        "continuously" => "continuous",
        "sadly" => "sad",
        "eventually" => "eventual",
        "possibly" => "possible",
        "visually" => "visual",
        "usually" => "usual",
        "normally" => "normal",
        "steadily" => "steady",
        "actively" => "active",
        "statistically" => "statistical",
        "culturally" => "cultural",
        "vividly" => "vivid",
        "partially" => "partial",
        "seriously" => "serious",
        "verbally" => "verbal",
        "shortly" => "short",
        "mildly" => "mild",
        "secretly" => "secret",
        "especially" => "especial",
        "specially" => "special",
        "previously" => "previous",
        "traditionally" => "traditional",
        "individually" => "individual",
        "carefully" => "careful",
        "essentially" => "essential",
        "originally" => "original",
        "alarmingly" => "alarming",
        "newly" => "new",
        "structurally" => "structural",
        "globally" => "global",
        "seamlessly" => "seamless",
        "sustainably" => "sustainable",
        "coldly" => "cold",
        "densely" => "dense",
        "calmly" => "calm",
        "widely" => "wide",
        "heavily" => "heavy",
        "honestly" => "honest",
        "desperately" => "desperate",
        "apparently" => "apparent",
        "uniquely" => "unique",
        "critically" => "critical",
        "equally" => "equal",
        "strongly" => "strong",
        "thoroughly" => "thorough",
        "technically" => "technical",
        "swiftly" => "swift",
        "occasionally" => "occasional",
        "specifically" => "specific",
        "subtly" => "subtle",
        "actually" => "actual",
        "particularly" => "particular",
        "nicely" => "nice",
        "progressively" => "progressive",
        "genuinely" => "genuine",
        "deeply" => "deep",
        "spiritually" => "spiritual",
        "purely" => "pure",
        "oddly" => "odd",
        "professionally" => "professional",
        "consistently" => "consistent",
        "truly" => "true",
        "commonly" => "common",
        "safely" => "safe",
        "internally" => "internal",
        "annually" => "annual",
        "brightly" => "bright",
        "officially" => "official",
        "perfectly" => "perfect",
        "overly" => "over",
        "brilliantly" => "brilliant",
        "exclusively" => "exclusive",
        "commercially" => "commercial",
        "weirdly" => "weird",
        "routinely" => "routine",
        "naturally" => "natural",
        "lightly" => "light",
        "primarily" => "primary",
        "peacefully" => "peaceful",
        "thankfully" => "thankful",
        "reliably" => "reliable",
        "hugely" => "huge",
        "strictly" => "strict",
        "morally" => "moral",
        "voluntarily" => "voluntary",
        "typically" => "typical",
        "playfully" => "playful",
        "wonderfully" => "wonderful",
        "emotionally" => "emotional",
        "efficiently" => "efficient",
        "mentally" => "mental",
        "credibly" => "credible",
        "rashly" => "rash",
        "financially" => "financial",
        "urgently" => "urgent",
        "historically" => "historical",
        "tightly" => "tight",
        "greedily" => "greedy",
        "fluently" => "fluent",
        "ordinarily" => "ordinary",
        "inevitably" => "inevitable",
        "partly" => "partial",
        "initially" => "initial",
        "justly" => "just",
        "plausibly" => "plausible",
        "massively" => "massive",
        "approximately" => "approximate",
        "extraordinarily" => "extraordinary",
        "warmly" => "warm",
        "nearly" => "near",
        "virtually" => "virtual",
        "regularly" => "regular",
        "deliberately" => "deliberate",
        "reasonably" => "reasonable",
        "similarly" => "similar",
        "flexibly" => "flexible",
        "softly" => "soft",
        "responsibly" => "responsible",
        "sweetly" => "sweet",
        "comfortably" => "comfortable",
        "uncomfortably" => "uncomfortable",
        "undoubtedly" => "undoubted",
        "humanly" => "human",
        "intrinsically" => "intrinsic",
        "substantially" => "substantial",
        "suspiciously" => "suspicious",
        "loudly" => "loud",
        "moderately" => "moderate",
        "gravely" => "grave",
        "digitally" => "digital",
        "finely" => "fine",
        "respectfully" => "respectful",
        "diagonally" => "diagonal",
        "additionally" => "additional",
        "sexually" => "sexual",
        "remarkably" => "remarkable",
        "acutely" => "acute",
        "linearly" => "linear",
        "unbelievably" => "unbelievable",
        "merrily" => "merry",
        "concretely" => "concrete",
        "intuitively" => "intuitive",
        "powerfully" => "powerful",
        "fairly" => "fair",
        "wholly" => "whole",
        "keenly" => "keen",
        "unconsciously" => "unconscious",
        "consciously" => "conscious",
        "humanely" => "humane",
        "rudely" => "rude",
        "incorrectly" => "incorrect",
        "correctly" => "correct",
        "mistakenly" => "mistaken",
        "wrongly" => "wrong",
        "willingly" => "willing",
        "hopefully" => "hopeful",
        "truthfully" => "truthful",
        "attractively" => "attractive",
        "supposedly" => "supposed",
        "imperfectly" => "imperfect",
        "wildly" => "wild",
        "hotly" => "hot",
        "genetically" => "genetic",
        "distinctly" => "distinct",
        "medically" => "medical",
        "certainly" => "certain",
        "beautifully" => "beautiful",
        "firmly" => "firm",
        "gradually" => "gradual",
        "grossly" => "gross",
        "memorably" => "memorable",
        "strangely" => "strange",
        "harshly" => "harsh",
        "proudly" => "proud",
        "politely" => "polite",
        "ethically" => "ethical",
        "noticeably" => "noticeable",
        "mainly" => "main",
        "popularly" => "popular",
        "improperly" => "improper",
        "cruelly" => "cruel",
        "optimally" => "optimal",
        "surprisingly" => "surprising",
        "severely" => "severe",
        "frequently" => "frequent",
        "rationally" => "rational",
        "separately" => "separate",
        "smoothly" => "smooth",
        "indirectly" => "indirect",
        "scientifically" => "scientific",
        "publicly" => "public",
        "tenderly" => "tender",
        "arguably" => "arguable",
        "horribly" => "horrible",
        "figuratively" => "figurative",
        "constantly" => "constant",
        "reluctantly" => "reluctant",
        "externally" => "external",
        "intellectually" => "intellectual",
        "dramatically" => "dramatic",
        "freshly" => "fresh",
        "casually" => "casual",
        "enormously" => "enormous",
        "alternatively" => "alternative",
        "gladly" => "glad",
        "bitterly" => "bitter",
        "faintly" => "faint",
        "humbly" => "humble",
        "promptly" => "prompt",
        "neatly" => "neat",
        "insanely" => "insane",
        "firstly" => "first",
        "philosophically" => "philosophical",
        _ => return None,
    })
}

fn current_date() -> (u32, u32, u32) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let z = days as i64 + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as u32, m as u32, d as u32)
}

fn parse_month(s: &str) -> u32 {
    let lower = s.to_lowercase();
    match lower.as_str() {
        "1" | "01" => 1,
        "2" | "02" => 2,
        "3" | "03" => 3,
        "4" | "04" => 4,
        "5" | "05" => 5,
        "6" | "06" => 6,
        "7" | "07" => 7,
        "8" | "08" => 8,
        "9" | "09" => 9,
        "10" => 10,
        "11" => 11,
        "12" => 12,
        n if n.starts_with("jan") => 1,
        n if n.starts_with("feb") => 2,
        n if n.starts_with("mar") => 3,
        n if n.starts_with("apr") => 4,
        n if n.starts_with("may") => 5,
        n if n.starts_with("jun") => 6,
        n if n.starts_with("jul") => 7,
        n if n.starts_with("aug") => 8,
        n if n.starts_with("sep") => 9,
        n if n.starts_with("oct") => 10,
        n if n.starts_with("nov") => 11,
        n if n.starts_with("dec") => 12,
        _ => 0,
    }
}

fn parse_weekday(s: &str) -> u32 {
    let lower = s.to_lowercase();
    let prefix = if lower.len() >= 2 {
        &lower[..2]
    } else {
        &lower
    };
    match prefix {
        "su" => 1,
        "mo" => 2,
        "tu" => 3,
        "we" => 4,
        "th" => 5,
        "fr" => 6,
        "sa" => 7,
        _ => 0,
    }
}

fn compute_weekday(year: u32, month: u32, day: u32) -> u32 {
    let (y, m) = if month < 3 {
        (year - 1, month + 12)
    } else {
        (year, month)
    };
    let h = (day + (13 * (m + 1)) / 5 + y + y / 4 - y / 100 + y / 400) % 7;
    match h {
        0 => 7,
        1 => 1,
        n => n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_xml::compiler::XmlCompiler;

    fn compile_and_match(xml: &str, sentence_text: &str) -> Vec<RuleMatch> {
        let compiler = XmlCompiler::new();
        let rule_set = compiler.compile_file(xml).unwrap();
        let engine = PatternRuleEngine::new();

        let sentence = make_analyzed_sentence(sentence_text);
        let mut matches = Vec::new();
        for rule in &rule_set.rules {
            matches.extend(engine.match_rule(rule, &sentence));
        }
        matches
    }

    fn make_analyzed_sentence(text: &str) -> og_core::AnalyzedSentence {
        use og_core::{AnalyzedToken, AnalyzedTokenReadings};
        let mut sentence = og_core::AnalyzedSentence::new(text, 0, text.len());
        let mut tokens = Vec::new();
        let mut pos = 0;
        let chars: Vec<char> = text.chars().collect();
        while pos < chars.len() {
            while pos < chars.len() && chars[pos].is_whitespace() {
                pos += 1;
            }
            if pos >= chars.len() {
                break;
            }
            let start_byte = text[..text
                .char_indices()
                .nth(pos)
                .map(|(i, _)| i)
                .unwrap_or(text.len())]
                .len();
            if chars[pos].is_alphanumeric() || chars[pos] == '\'' {
                let word_start = pos;
                while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '\'') {
                    pos += 1;
                }
                let word: String = chars[word_start..pos].iter().collect();
                let end_byte = text[..text
                    .char_indices()
                    .nth(pos)
                    .map(|(i, _)| i)
                    .unwrap_or(text.len())]
                    .len();
                let at = AnalyzedToken::new(&word, start_byte, end_byte);
                tokens.push(AnalyzedTokenReadings::new(at));
            } else {
                let ch = chars[pos].to_string();
                pos += 1;
                let end_byte = text[..text
                    .char_indices()
                    .nth(pos)
                    .map(|(i, _)| i)
                    .unwrap_or(text.len())]
                    .len();
                let at = AnalyzedToken::new(&ch, start_byte, end_byte);
                tokens.push(AnalyzedTokenReadings::new(at));
            }
        }
        sentence.set_tokens(tokens);
        sentence
    }

    #[test]
    fn test_simple_match() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>hello</token>
                        <token>world</token>
                    </pattern>
                    <message>Use <suggestion>goodbye world</suggestion></message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "hello world");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].message(), "Use goodbye world");
    }

    #[test]
    fn test_no_match() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>hello</token>
                        <token>world</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "foo bar");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_regexp_match() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token regexp="yes">hel+o</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "hello");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_negate_match() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token negate="yes">bad</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "good");
        assert_eq!(matches.len(), 1);

        let no_matches = compile_and_match(xml, "bad");
        assert!(no_matches.is_empty());
    }

    #[test]
    fn test_antipattern_blocks_match() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <antipattern>
                        <token>deep</token>
                        <token>clean</token>
                    </antipattern>
                    <pattern>
                        <token>clean</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "clean the house");
        assert_eq!(matches.len(), 1);

        let blocked = compile_and_match(xml, "deep clean");
        assert!(blocked.is_empty());
    }

    #[test]
    fn test_antipattern_with_inflected() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <antipattern>
                        <token>free</token>
                        <token inflected="yes">fall</token>
                    </antipattern>
                    <pattern>
                        <token>fall</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let blocked = compile_and_match(xml, "free fall");
        assert!(blocked.is_empty());
    }

    #[test]
    fn test_marker_limits_error_span() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>the</token>
                        <marker>
                            <token>bad</token>
                        </marker>
                        <token>word</token>
                    </pattern>
                    <message>Fix <suggestion>good</suggestion></message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "the bad word");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].offset(), 3);
        assert_eq!(matches[0].length(), 4);
    }

    #[test]
    fn test_backreference_in_suggestion() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>foo</token>
                        <token>bar</token>
                    </pattern>
                    <message>Use <suggestion>\1 \2</suggestion></message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "foo bar");
        assert_eq!(matches.len(), 1);
        let repl: Vec<&str> = matches[0]
            .replacements()
            .iter()
            .map(|r| r.value())
            .collect();
        assert_eq!(repl, vec!["foo bar"]);
    }

    #[test]
    fn test_backreference_with_literal() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>Ponzi</token>
                        <token regexp="yes">schemes?</token>
                    </pattern>
                    <message>Use <suggestion>Ponzi \2</suggestion></message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "Ponzi scheme");
        assert_eq!(matches.len(), 1);
        let repl: Vec<&str> = matches[0]
            .replacements()
            .iter()
            .map(|r| r.value())
            .collect();
        assert_eq!(repl, vec!["Ponzi scheme"]);
    }

    #[test]
    fn test_skip_tokens() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token skip="2">start</token>
                        <token>end</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "start the end");
        assert_eq!(matches.len(), 1);

        let direct = compile_and_match(xml, "start end");
        assert_eq!(direct.len(), 1);
    }

    #[test]
    fn test_optional_token_min_zero() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>hello</token>
                        <token min="0">the</token>
                        <token>world</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let with = compile_and_match(xml, "hello the world");
        assert_eq!(with.len(), 1);

        let without = compile_and_match(xml, "hello world");
        assert_eq!(without.len(), 1);
    }

    #[test]
    fn test_exception_scope_next() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>word<exception scope="next">end</exception></token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "word start");
        assert_eq!(matches.len(), 1);

        let blocked = compile_and_match(xml, "word end");
        assert!(blocked.is_empty());
    }

    #[test]
    fn test_negate_any_token() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>start</token>
                        <token negate="yes"/>
                        <token>end</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "start middle end");
        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn test_multiple_antipatterns() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <antipattern>
                        <token>nordic</token>
                        <token>ski</token>
                    </antipattern>
                    <antipattern>
                        <token>water</token>
                        <token>ski</token>
                    </antipattern>
                    <pattern>
                        <token>ski</token>
                    </pattern>
                    <message>Test</message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "I like to ski");
        assert_eq!(matches.len(), 1);

        let blocked1 = compile_and_match(xml, "nordic ski");
        assert!(blocked1.is_empty());

        let blocked2 = compile_and_match(xml, "water ski");
        assert!(blocked2.is_empty());
    }

    #[test]
    fn test_marker_with_multiple_tokens() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>I</token>
                        <marker>
                            <token>wants</token>
                            <token>a</token>
                        </marker>
                        <token>drink</token>
                    </pattern>
                    <message>Use <suggestion>I want a</suggestion></message>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "I wants a drink");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].offset(), 1);
        assert_eq!(matches[0].length(), 8);
    }

    #[test]
    fn test_pattern_rule_match_type_follows_issue_type() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="STYLE_KIND" name="Test">
                    <pattern>
                        <token>teh</token>
                    </pattern>
                    <message>Did you mean <suggestion>the</suggestion>?</message>
                </rule>
            </category>
        </rules>"#;
        let compiler = og_xml::compiler::XmlCompiler::new();
        let mut rule_set = compiler.compile_file(xml).unwrap();
        let sentence = make_analyzed_sentence("teh word");

        let mut style_rule = rule_set.rules.remove(0);
        style_rule.issue_type = "style".to_string();
        let matches = PatternRuleEngine::new().match_rule(&style_rule, &sentence);
        assert_eq!(matches.len(), 1);
        let json = serde_json::to_value(&matches[0]).unwrap();
        assert_eq!(json["type"]["typeName"], "Hint");
        assert_eq!(json["rule"]["issueType"], "style");

        let mut grammar_rule = style_rule.clone();
        grammar_rule.issue_type = "grammar".to_string();
        let matches = PatternRuleEngine::new().match_rule(&grammar_rule, &sentence);
        let json = serde_json::to_value(&matches[0]).unwrap();
        assert_eq!(json["type"]["typeName"], "Other");
    }

    #[test]
    #[ignore]
    fn test_parallel_matching() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST" name="Test">
                    <pattern>
                        <token>error</token>
                    </pattern>
                    <message>Fix</message>
                </rule>
            </category>
        </rules>"#;

        let compiler = og_xml::compiler::XmlCompiler::new();
        let rule_set = compiler.compile_file(xml).unwrap();
        let rules: Vec<&og_xml::compiler::CompiledRule> = rule_set.rules.iter().collect();

        let mut engine = PatternRuleEngine::new();
        engine.build_index(&rules);

        let sentences: Vec<og_core::AnalyzedSentence> = vec![
            make_analyzed_sentence("this is an error here"),
            make_analyzed_sentence("no problems here"),
            make_analyzed_sentence("another error found"),
        ];

        let matches = engine.match_indexed_rules_parallel(&rules, &sentences);
        assert_eq!(matches.len(), 2);
    }

    #[test]
    #[ignore]
    fn test_indexed_rules_performance() {
        let grammar_path = "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/rules/en/grammar.xml";
        if !std::path::Path::new(grammar_path).exists() {
            eprintln!("Skipping benchmark: grammar.xml not found");
            return;
        }

        let xml = std::fs::read_to_string(grammar_path).unwrap();
        let compiler = og_xml::compiler::XmlCompiler::new();
        let rule_set = compiler.compile_file(&xml).unwrap();
        let rules: Vec<&og_xml::compiler::CompiledRule> = rule_set.rules.iter().collect();

        let mut engine = PatternRuleEngine::new();
        engine.build_index(&rules);

        eprintln!(
            "Indexed {} rules, {} unique first-tokens, {} catch-all rules",
            rules.len(),
            engine.word_index.len(),
            engine.catch_all_rule_indices.len()
        );

        let sentence = make_analyzed_sentence("The quick brown fox jumps over the lazy dog.");

        let start = std::time::Instant::now();
        let matches = engine.match_indexed_rules(&rules, &sentence);
        let elapsed = start.elapsed();
        eprintln!(
            "Matched {} rules against test sentence in {:?}",
            rules.len(),
            elapsed
        );
        eprintln!("Found {} matches", matches.len());

        assert!(
            elapsed.as_millis() < 5000,
            "Matching took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_ordinal_suffix_filter() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="ORDINAL_SUFFIX" name="Ordinal suffix">
                    <pattern>
                        <token regexp="yes">[0-9]+(st|nd|rd|th)</token>
                    </pattern>
                    <message>Did you mean <suggestion>\1</suggestion>?</message>
                    <filter class="org.languagetool.rules.en.OrdinalSuffixFilter"/>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "The 21nd of May");
        assert!(!matches.is_empty());
        let repl: Vec<&str> = matches[0]
            .replacements()
            .iter()
            .map(|r| r.value())
            .collect();
        assert!(repl.contains(&"21st"), "Expected '21st', got {:?}", repl);

        let matches = compile_and_match(xml, "The 22st of May");
        assert!(!matches.is_empty());
        let repl: Vec<&str> = matches[0]
            .replacements()
            .iter()
            .map(|r| r.value())
            .collect();
        assert!(repl.contains(&"22nd"), "Expected '22nd', got {:?}", repl);

        let matches = compile_and_match(xml, "The 11nd item");
        assert!(!matches.is_empty());
        let repl: Vec<&str> = matches[0]
            .replacements()
            .iter()
            .map(|r| r.value())
            .collect();
        assert!(repl.contains(&"11th"), "Expected '11th', got {:?}", repl);

        let matches = compile_and_match(xml, "The 3th item");
        assert!(!matches.is_empty());
        let repl: Vec<&str> = matches[0]
            .replacements()
            .iter()
            .map(|r| r.value())
            .collect();
        assert!(repl.contains(&"3rd"), "Expected '3rd', got {:?}", repl);
    }

    #[test]
    fn test_underline_spaces_filter_before() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="UNDERLINE_BEFORE" name="Underline before">
                    <pattern>
                        <token>hello</token>
                    </pattern>
                    <message>Test</message>
                    <filter class="org.languagetool.rules.UnderlineSpacesFilter" args="underlineSpaces:before"/>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "say hello world");
        assert!(!matches.is_empty());
        assert_eq!(
            matches[0].offset(),
            3,
            "Expected offset 3 (including preceding space)"
        );
    }

    #[test]
    fn test_underline_spaces_filter_after() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="UNDERLINE_AFTER" name="Underline after">
                    <pattern>
                        <token>hello</token>
                    </pattern>
                    <message>Test</message>
                    <filter class="org.languagetool.rules.UnderlineSpacesFilter" args="underlineSpaces:after"/>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "say hello world");
        assert!(!matches.is_empty());
        assert_eq!(
            matches[0].length(),
            7,
            "Expected length 7 (including trailing space)"
        );
    }

    #[test]
    fn test_adverb_filter() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="ADVERB_TEST" name="Adverb test">
                    <pattern>
                        <token>philosophically</token>
                        <token>question</token>
                    </pattern>
                    <message>Use adjective, not adverb.</message>
                    <suggestion>\1 \2</suggestion>
                    <filter class="org.languagetool.rules.en.AdverbFilter" args="adverb:\1 noun:\2"/>
                </rule>
            </category>
        </rules>"#;

        let matches = compile_and_match(xml, "a philosophically question here");
        assert!(!matches.is_empty());
        let repl: Vec<&str> = matches[0]
            .replacements()
            .iter()
            .map(|r| r.value())
            .collect();
        assert!(
            repl.contains(&"philosophical question"),
            "Expected 'philosophical question', got {:?}",
            repl
        );
    }
}

#[cfg(test)]
mod unify_tests {
    use super::*;
    use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};
    use og_xml::compiler::XmlCompiler;

    fn compile(xml: &str) -> Vec<CompiledRule> {
        XmlCompiler::new().compile_file(xml).expect("compile").rules
    }

    fn sentence(parts: &[(&str, &[&str])]) -> AnalyzedSentence {
        let mut tokens = vec![AnalyzedTokenReadings::new(
            AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
        )];
        let mut offset = 0;
        for (text, tags) in parts {
            let readings: Vec<AnalyzedToken> = tags
                .iter()
                .map(|t| {
                    AnalyzedToken::new(*text, offset, offset + text.len())
                        .with_pos_tags(vec![t.to_string()])
                })
                .collect();
            let primary = AnalyzedToken::new(*text, offset, offset + text.len())
                .with_pos_tags(tags.iter().map(|t| t.to_string()).collect());
            tokens.push(AnalyzedTokenReadings::new(primary).with_readings(readings));
            offset += text.len() + 1;
        }
        let mut s = AnalyzedSentence::new("", 0, offset);
        s.set_tokens(tokens);
        s
    }

    fn run(xml: &str, parts: &[(&str, &[&str])]) -> usize {
        let rules = compile(xml);
        PatternRuleEngine::new()
            .match_rule(&rules[0], &sentence(parts))
            .len()
    }

    const DEFS: &str = r#"
    <unification feature="number">
        <equivalence type="sg"><token postag="NN.*:sg.*|VB.*:sg.*" postag_regexp="yes"/></equivalence>
        <equivalence type="pl"><token postag="NN.*:pl.*|VB.*:pl.*" postag_regexp="yes"/></equivalence>
    </unification>
    <unification feature="gender">
        <equivalence type="m"><token postag="(NN|VB).*:m.*" postag_regexp="yes"/></equivalence>
        <equivalence type="f"><token postag="(NN|VB).*:f.*" postag_regexp="yes"/></equivalence>
    </unification>"#;

    fn xml(unify_body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rules lang="pl">{DEFS}
    <category id="GRAMMAR" name="Grammar">
        <rule id="UNIFY_TEST" name="unify">
            <pattern>{unify_body}</pattern>
            <message>Unification.</message>
        </rule>
    </category>
</rules>"#
        )
    }

    #[test]
    fn bare_unify_enforces_all_declared_features() {
        let body = r#"<unify><token postag="VB.*" postag_regexp="yes"/><token postag="NN.*:pl.*" postag_regexp="yes"/></unify>"#;
        let x = xml(body);
        assert_eq!(
            run(&x, &[("widzi", &["VB:sg:1:f"]), ("psy", &["NN:pl:m1"])]),
            0
        );
        assert_eq!(
            run(&x, &[("widz", &["VB:pl:1:f"]), ("psy", &["NN:pl:f1"])]),
            1
        );
    }

    #[test]
    fn unify_uses_any_reading_of_a_token() {
        let body = r#"<unify><token postag="VB.*" postag_regexp="yes"/><token postag="NN.*:pl.*" postag_regexp="yes"/></unify>"#;
        let x = xml(body);
        assert_eq!(
            run(
                &x,
                &[
                    ("widzi", &["VB:sg:1:f", "VB:pl:3:f"]),
                    ("psy", &["NN:pl:f1"])
                ]
            ),
            1
        );
    }

    #[test]
    fn same_reading_must_satisfy_every_feature() {
        let body = r#"<unify><token postag="NN.*" postag_regexp="yes"/><token postag="NN.*:pl.*" postag_regexp="yes"/></unify>"#;
        let x = xml(body);
        assert_eq!(
            run(
                &x,
                &[("dom", &["NN:sg:m", "NN:pl:f"]), ("psy", &["NN:pl:n1"])]
            ),
            0,
            "no single reading of 'dom' is both plural and gender-consistent"
        );
    }

    #[test]
    fn negated_unify_inverts_the_constraint() {
        let body = r#"<unify negate="yes"><token postag="VB.*" postag_regexp="yes"/><token postag="NN.*:pl.*" postag_regexp="yes"/></unify>"#;
        let x = xml(body);
        assert_eq!(
            run(&x, &[("widzi", &["VB:sg:1:f"]), ("psy", &["NN:pl:m1"])]),
            1
        );
        assert_eq!(
            run(&x, &[("widz", &["VB:pl:1:f"]), ("psy", &["NN:pl:f1"])]),
            0
        );
    }

    #[test]
    fn unify_ignore_tokens_are_neutral() {
        let body = r#"<unify><token postag="VB.*" postag_regexp="yes"/><unify-ignore><token>psy</token></unify-ignore><token postag="NN.*:pl.*" postag_regexp="yes"/></unify>"#;
        let x = xml(body);
        assert_eq!(
            run(
                &x,
                &[
                    ("widzi", &["VB:sg:1:f"]),
                    ("psy", &["NN:pl:m1"]),
                    ("psy", &["NN:pl:m1"])
                ]
            ),
            0,
            "the neutral token must not help the sg/pl mismatch"
        );
        assert_eq!(
            run(
                &x,
                &[
                    ("widz", &["VB:pl:1:f"]),
                    ("psy", &["NN:pl:f1"]),
                    ("psy", &["NN:pl:f1"])
                ]
            ),
            1
        );
    }

    #[test]
    fn feature_restriction_limits_the_unification() {
        let body = r#"<unify><feature id="number"><type id="sg"/></feature><token postag="NN.*" postag_regexp="yes"/><token postag="NN.*" postag_regexp="yes"/></unify>"#;
        let x = xml(body);
        assert_eq!(run(&x, &[("dom", &["NN:sg:m"]), ("dom", &["NN:sg:f"])]), 1);
        assert_eq!(run(&x, &[("dom", &["NN:sg:m"]), ("psy", &["NN:pl:n1"])]), 0);
    }

    #[test]
    fn undefined_feature_fails_the_group() {
        let body = r#"<unify><feature id="casus"/><token postag="VB.*" postag_regexp="yes"/><token postag="NN.*:pl.*" postag_regexp="yes"/></unify>"#;
        let x = xml(body);
        assert_eq!(
            run(&x, &[("widz", &["VB:pl:1"]), ("psy", &["NN:pl:n1"])]),
            0
        );
    }
}
