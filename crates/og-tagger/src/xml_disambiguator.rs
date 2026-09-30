use og_core::analyzed::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};
use og_core::checker::Disambiguator;
use og_xml::compiler::{
    CompiledPattern, CompiledPatternElement, CompiledPatternToken, PosMatcher, XmlCompiler,
};
use og_xml::disambig_parser::DisambigXmlParser;
use og_xml::disambig_types::{CompiledDisambigRule, DisambigAction, DisambigRule};

enum FirstTokenGate {
    None,
    Text { text: String, case_sensitive: bool },
    LiteralSet(std::collections::HashSet<String>),
    OrLiteralSet(std::collections::HashSet<String>),
    PostagPlain { tag: String, negate_pos: bool },
}

pub struct XmlDisambiguator {
    rules: Vec<CompiledDisambigRule>,
    unifications: Vec<(String, Vec<(String, PosMatcher)>)>,
    action_regexes: Vec<DisambigActionRegex>,
    gates: Vec<FirstTokenGate>,
    first_literal: Vec<Option<FirstLiteralFilter>>,
    first_regex: Vec<Option<FirstRegexFilter>>,
    english_tagger: Option<std::sync::Arc<crate::english_tagger::EnglishTagger>>,
}

struct FirstRegexFilter {
    re: regex::Regex,
    inflected: bool,
}

struct FirstLiteralFilter {
    text: String,
    case_sensitive: bool,
    inflected: bool,
}

enum DisambigActionRegex {
    Remove(Vec<RemoveWordMatcher>),
    RemovePostag(Option<regex::Regex>, Option<fancy_regex::Regex>),
    Filter(Option<regex::Regex>, Option<fancy_regex::Regex>),
    None,
}

struct RemoveWordMatcher {
    regex: Option<regex::Regex>,
    literal: Option<String>,
    lemma: Option<String>,
}

impl XmlDisambiguator {
    pub fn from_xml(xml: &str) -> Result<Self, String> {
        Self::from_xml_with_english(xml, None)
    }

    pub fn from_xml_with_english(
        xml: &str,
        english_tagger: Option<std::sync::Arc<crate::english_tagger::EnglishTagger>>,
    ) -> Result<Self, String> {
        let parser = DisambigXmlParser::new();
        let rule_set = parser.parse(xml).map_err(|e| e.to_string())?;

        let compiler = XmlCompiler::new();
        let compiled_rules: Vec<CompiledDisambigRule> = rule_set
            .rules
            .into_iter()
            .map(|r| compile_disambig_rule(&compiler, r))
            .collect();

        let mut unifications: Vec<(String, Vec<(String, PosMatcher)>)> = Vec::new();
        for uni in &rule_set.unifications {
            let mut compiled = Vec::new();
            for eq in &uni.equivalences {
                if let Some(re) = PosMatcher::new_anchored(&format!("^(?:{})$", eq.postag)) {
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
                DisambigAction::RemovePostag(postag) => {
                    let anchored = format!("^(?:{})$", postag);
                    DisambigActionRegex::RemovePostag(
                        regex::Regex::new(&anchored).ok(),
                        fancy_regex::Regex::new(&anchored).ok(),
                    )
                }
                DisambigAction::Filter { postag } => {
                    let anchored = format!("^(?:{postag})$");
                    DisambigActionRegex::Filter(
                        regex::Regex::new(&anchored).ok(),
                        fancy_regex::Regex::new(&anchored).ok(),
                    )
                }
                _ => DisambigActionRegex::None,
            })
            .collect();

        let gates = compiled_rules.iter().map(build_first_token_gate).collect();

        let first_literal = {
            let rules = &compiled_rules;
            (0..rules.len())
                .map(|i| {
                    let first = rules[i].pattern.tokens.first()?;
                    if rules[i].pattern.has_structured_elements
                        || first.negate
                        || first.min == Some(0)
                        || first.compiled_regexp.is_some()
                    {
                        return None;
                    }
                    let text = first.text.as_ref()?;
                    Some(FirstLiteralFilter {
                        text: text.clone(),
                        case_sensitive: first.case_sensitive,
                        inflected: first.inflected,
                    })
                })
                .collect()
        };

        let first_regex: Vec<Option<FirstRegexFilter>> = {
            let rules = &compiled_rules;
            (0..rules.len())
                .map(|i| {
                    let first = rules[i].pattern.tokens.first()?;
                    if rules[i].pattern.has_structured_elements
                        || first.negate
                        || first.min == Some(0)
                        || first.text.is_some()
                    {
                        return None;
                    }
                    let re = first.compiled_regexp.as_ref()?;
                    let re_to_use = if !first.case_sensitive {
                        first.compiled_regexp_ci.as_ref().unwrap_or(re)
                    } else {
                        re
                    };
                    Some(FirstRegexFilter {
                        re: re_to_use.clone(),
                        inflected: first.inflected,
                    })
                })
                .collect()
        };

        Ok(Self {
            rules: compiled_rules,
            unifications,
            action_regexes,
            gates,
            first_literal,
            first_regex,
            english_tagger,
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
    if let Some(og_xml::compiler::CompiledPatternElement::OrGroup(group)) =
        rule.pattern.elements.first()
    {
        let mut set = std::collections::HashSet::new();
        let mut usable = !group.is_empty();
        for alt in group {
            if alt.negate
                || alt.min == Some(0)
                || alt.inflected
                || alt.compiled_regexp.is_some()
                || alt.text.as_deref().is_none_or(str::is_empty)
            {
                usable = false;
                break;
            }
            set.insert(alt.text.clone().unwrap_or_default().to_lowercase());
        }
        if usable {
            return FirstTokenGate::OrLiteralSet(set);
        }
        return FirstTokenGate::None;
    }
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
        for (rule_idx, rule) in self.rules.iter().enumerate() {
            if rule.pattern.tokens.is_empty() && rule.pattern.elements.is_empty() {
                continue;
            }
            let mut pending_ignore_spelling: Vec<(usize, usize)> = Vec::new();
            let mut pending_actions: Vec<(usize, &DisambigAction, &DisambigActionRegex)> =
                Vec::new();
            let mut pending_unify: Vec<(usize, AnalyzedToken, Vec<AnalyzedToken>)> = Vec::new();
            let mut pending_filterall: Vec<(usize, &CompiledPatternToken)> = Vec::new();
            let mut pending_immunize: Vec<usize> = Vec::new();
            let synthesized_tokens: Vec<CompiledPatternToken> = if rule.pattern.tokens.is_empty()
                && !rule.pattern.elements.is_empty()
            {
                rule.pattern
                    .elements
                    .iter()
                    .filter_map(|e| match e {
                        og_xml::compiler::CompiledPatternElement::Token(t) => Some((**t).clone()),
                        og_xml::compiler::CompiledPatternElement::OrGroup(alts) => {
                            alts.first().cloned()
                        }
                        og_xml::compiler::CompiledPatternElement::AndGroup(cs) => {
                            cs.first().cloned()
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            {
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

                let Some(first) = synthesized_tokens.first().or(rule.pattern.tokens.first()) else {
                    continue;
                };
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
                    FirstTokenGate::OrLiteralSet(set) => {
                        if !lower_texts.iter().any(|l| set.contains(l)) {
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
                    let mut first_preverified = false;
                    if let Some(f) = self.first_literal[rule_idx].as_ref() {
                        let t = tokens[start_idx];
                        let surface = if f.case_sensitive {
                            t.token().token() == f.text
                        } else {
                            t.token().token().eq_ignore_ascii_case(&f.text)
                        };
                        let ok = if f.inflected {
                            surface
                                || t.readings().iter().any(|r| {
                                    r.lemma().is_some_and(|lemma| {
                                        if f.case_sensitive {
                                            lemma == f.text
                                        } else {
                                            lemma.eq_ignore_ascii_case(&f.text)
                                        }
                                    })
                                })
                        } else {
                            surface
                        };
                        if !ok {
                            continue;
                        }
                    }
                    if let Some(f) = self.first_regex[rule_idx].as_ref() {
                        let tok = tokens[start_idx];
                        let surface = f.re.is_match(tok.token().token());
                        let matched = if f.inflected {
                            surface
                                || tok
                                    .readings()
                                    .iter()
                                    .any(|r| r.lemma().is_some_and(|lemma| f.re.is_match(lemma)))
                        } else {
                            surface
                        };
                        if !matched {
                            continue;
                        }
                        first_preverified = true;
                    }
                    let effective_pattern = &rule.pattern;
                    let mp_result = if first_preverified
                        && !effective_pattern.case_sensitive
                        && !effective_pattern.has_structured_elements
                    {
                        match_tokens_preverified(&effective_pattern.tokens, &tokens, start_idx)
                    } else {
                        match_pattern(effective_pattern, &tokens, start_idx)
                    };
                    if let Some(match_result) = mp_result {
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

                        if rule.filter_class.as_deref()
                            == Some("org.languagetool.rules.IsEnglishWordFilter")
                            && !self.passes_english_word_filter(rule, &tokens, &match_result)
                        {
                            continue;
                        }

                        let mut unify_survivors: Option<Vec<(usize, Vec<u32>)>> = None;
                        if let Some(ref spec) = rule.unify {
                            let feature_list: Vec<&str> = if spec.features.is_empty() {
                                vec![spec.feature.as_str()]
                            } else {
                                spec.features.iter().map(String::as_str).collect()
                            };
                            let mut features: Vec<(String, Vec<&str>)> =
                                Vec::with_capacity(feature_list.len());
                            let mut eq_matchers: std::collections::HashMap<
                                (&str, &str),
                                &PosMatcher,
                            > = std::collections::HashMap::new();
                            let mut all_found = true;
                            for f in &feature_list {
                                match self.unifications.iter().find(|(name, _)| name == f) {
                                    Some((_, eqs)) => {
                                        for (ty, re) in eqs {
                                            eq_matchers.insert((f, ty.as_str()), re);
                                        }
                                        features.push((
                                            (*f).to_string(),
                                            eqs.iter().map(|(ty, _)| ty.as_str()).collect(),
                                        ));
                                    }
                                    None => {
                                        all_found = false;
                                        break;
                                    }
                                }
                            }
                            if !all_found {
                                continue;
                            }
                            let has_structured = rule.pattern.has_structured_elements;
                            let (start, end) = if has_structured {
                                (spec.elem_start, spec.elem_end)
                            } else {
                                (spec.token_start, spec.token_end)
                            };
                            if end <= start {
                                if spec.negate {
                                    unify_survivors = None;
                                } else {
                                    continue;
                                }
                            } else {
                                let indexed_unify: Vec<(usize, usize)> = match_result
                                    .matched_positions
                                    .iter()
                                    .enumerate()
                                    .filter(|(i, _)| *i >= start && *i < end)
                                    .map(|(i, &p)| (p, i))
                                    .collect();
                                if indexed_unify.len() != end - start && !spec.negate {
                                    continue;
                                }
                                let mut plans: Vec<og_xml::unifier::UnifyElemPlan> =
                                    Vec::with_capacity(end - start);
                                let mut fed_positions: Vec<usize> = Vec::with_capacity(end - start);
                                for &(pos, pattern_idx) in &indexed_unify {
                                    let token = tokens[pos];
                                    let neutral = if has_structured {
                                        match rule.pattern.elements.get(pattern_idx) {
                                            Some(
                                                og_xml::compiler::CompiledPatternElement::Token(t),
                                            ) => t.unify_neutral,
                                            _ => false,
                                        }
                                    } else {
                                        rule.pattern
                                            .tokens
                                            .get(pattern_idx)
                                            .is_some_and(|t| t.unify_neutral)
                                    };
                                    if neutral {
                                        plans.push(og_xml::unifier::UnifyElemPlan {
                                            elem_idx: pattern_idx,
                                            neutral_readings: vec![token.readings().len()],
                                            reading_sets: Vec::new(),
                                        });
                                        fed_positions.push(pos);
                                        continue;
                                    }
                                    let criteria: Vec<&CompiledPatternToken> =
                                        if has_structured {
                                            match rule.pattern.elements.get(pattern_idx) {
                                            Some(
                                                og_xml::compiler::CompiledPatternElement::Token(t),
                                            ) => vec![t.as_ref()],
                                            Some(og_xml::compiler::CompiledPatternElement::AndGroup(
                                                cs,
                                            )) => match cs.first() {
                                                Some(head) => vec![head],
                                                None => continue,
                                            },
                                            Some(og_xml::compiler::CompiledPatternElement::OrGroup(
                                                alts,
                                            )) => alts.iter().collect(),
                                            None => continue,
                                        }
                                        } else {
                                            match rule.pattern.tokens.get(pattern_idx) {
                                                Some(t) => vec![t],
                                                None => continue,
                                            }
                                        };
                                    let surface = og_xml::unifier::surface_text(token);
                                    let untagged = og_xml::unifier::has_no_tag_reading(token);
                                    let mut all_variants: Vec<
                                        Vec<Vec<og_xml::unifier::UnifyReading<'_>>>,
                                    > = Vec::with_capacity(criteria.len());
                                    for criterion in &criteria {
                                        let mut reading_tags: Vec<
                                            og_xml::unifier::UnifyReading<'_>,
                                        > = Vec::with_capacity(token.readings().len());
                                        for (ri, reading) in token.readings().iter().enumerate() {
                                            if og_xml::unifier::reading_matches_element(
                                                criterion, surface, reading, untagged,
                                            ) {
                                                reading_tags.push(og_xml::unifier::UnifyReading {
                                                    tags: reading
                                                        .pos_tags()
                                                        .iter()
                                                        .map(String::as_str)
                                                        .collect(),
                                                    id: ri as u32,
                                                });
                                            }
                                        }
                                        if !reading_tags.is_empty() {
                                            all_variants.push(vec![reading_tags]);
                                        }
                                    }
                                    if all_variants.is_empty() {
                                        continue;
                                    }
                                    plans.push(og_xml::unifier::UnifyElemPlan {
                                        elem_idx: pattern_idx,
                                        neutral_readings: Vec::new(),
                                        reading_sets: all_variants,
                                    });
                                    fed_positions.push(pos);
                                }
                                let mut uni = og_xml::unifier::Unifier::new(features, eq_matchers);
                                if !og_xml::unifier::eval_unify_plans(
                                    &plans,
                                    0,
                                    end - 1,
                                    spec.negate,
                                    &mut uni,
                                ) {
                                    continue;
                                }
                                if matches!(rule.disambig, DisambigAction::Unify) && !spec.negate {
                                    if fed_positions.len() != end - start {
                                        continue;
                                    }
                                    let Some(survivors) = uni.unified_readings() else {
                                        continue;
                                    };
                                    unify_survivors = Some(
                                        fed_positions
                                            .into_iter()
                                            .zip(survivors)
                                            .filter(|(_, ids)| !ids.is_empty())
                                            .collect(),
                                    );
                                }
                            }
                        }

                        let has_marker = matches!(
                            (rule.pattern.marker_start, rule.pattern.marker_end),
                            (Some(_), Some(_))
                        );
                        let elem_bounds = if rule.pattern.has_structured_elements {
                            (rule.pattern.marker_elem_start, rule.pattern.marker_elem_end)
                        } else {
                            (None, None)
                        };
                        let effective_marker = if let (Some(ms), Some(me)) = elem_bounds {
                            (Some(ms), Some(me))
                        } else {
                            (rule.pattern.marker_start, rule.pattern.marker_end)
                        };
                        let indexed_targets: Vec<(usize, usize)> =
                            if let (Some(ms), Some(me)) = effective_marker {
                                match_result
                                    .matched_positions
                                    .iter()
                                    .enumerate()
                                    .filter(|(i, _)| *i >= ms && *i < me)
                                    .map(|(i, &pos)| (pos, i))
                                    .collect()
                            } else {
                                match_result
                                    .matched_positions
                                    .iter()
                                    .enumerate()
                                    .map(|(i, &pos)| (pos, i))
                                    .collect()
                            };
                        let target_indices: Vec<usize> = if has_marker {
                            indexed_targets.iter().map(|&(pos, _)| pos).collect()
                        } else {
                            indexed_targets
                                .first()
                                .map(|&(pos, _)| vec![pos])
                                .unwrap_or_default()
                        };

                        if matches!(rule.disambig, DisambigAction::FilterAll) {
                            for (pos, pattern_idx) in indexed_targets {
                                pending_filterall.push((
                                    pos,
                                    &rule.pattern.tokens[pattern_idx
                                        .min(rule.pattern.tokens.len().saturating_sub(1))],
                                ));
                            }
                        } else if matches!(rule.disambig, DisambigAction::Immunize) {
                            pending_immunize.extend(match_result.matched_positions.iter().copied());
                        } else if matches!(rule.disambig, DisambigAction::RemovePostag(_)) {
                            if let Some(&first) = target_indices.first() {
                                pending_actions.push((
                                    first,
                                    &rule.disambig,
                                    &self.action_regexes[rule_idx],
                                ));
                            }
                        } else if matches!(rule.disambig, DisambigAction::IgnoreSpelling) {
                            if let (Some(&(first, _)), Some(&(last, _))) =
                                (indexed_targets.first(), indexed_targets.last())
                            {
                                pending_ignore_spelling.push((first, last));
                            }
                        } else if matches!(rule.disambig, DisambigAction::Unify)
                            && let Some(ref survivors) = unify_survivors
                        {
                            let mut entries: Vec<(usize, AnalyzedToken, Vec<AnalyzedToken>)> =
                                Vec::with_capacity(survivors.len());
                            for (pos, ids) in survivors.iter() {
                                let atr = tokens[*pos];
                                if ids.len() == atr.readings().len() {
                                    continue;
                                }
                                let base = atr.token().clone();
                                let kept: Vec<AnalyzedToken> = ids
                                    .iter()
                                    .filter_map(|&id| atr.readings().get(id as usize).cloned())
                                    .collect();
                                if kept.is_empty() {
                                    continue;
                                }
                                entries.push((*pos, base, kept));
                            }
                            pending_unify.extend(entries);
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
                if std::env::var("OG_TRACE_DISAMBIG").is_ok() {
                    let token = sentence.non_whitespace_tokens()
                        [non_ws_idx.min(sentence.non_whitespace_tokens().len().saturating_sub(1))];
                    eprintln!(
                        "DISAMBIG[{}] {:?} on '{}' -> {:?}",
                        rule_idx,
                        rule.id,
                        token.token().token(),
                        action
                    );
                }
                apply_disambiguation(sentence.tokens_mut(), non_ws_idx, action, matcher);
            }

            for (non_ws_idx, pattern_token) in pending_filterall {
                apply_filterall(sentence.tokens_mut(), non_ws_idx, pattern_token);
            }

            let tokens = sentence.tokens_mut();
            let nw: Vec<usize> = (0..tokens.len())
                .filter(|&i| !tokens[i].is_whitespace())
                .collect();
            for (first, last) in pending_ignore_spelling {
                if let (Some(&start), Some(&end)) = (nw.get(first), nw.get(last)) {
                    for token in &mut tokens[start..=end] {
                        token.ignore_spelling();
                    }
                }
            }
            for non_ws_idx in pending_immunize {
                if let Some(&real_idx) = nw.get(non_ws_idx) {
                    tokens[real_idx].immunize();
                }
            }
            for (non_ws_idx, base, kept) in pending_unify {
                if let Some(&real_idx) = nw.get(non_ws_idx) {
                    let chunks = tokens[real_idx].chunks().to_vec();
                    let ignored_by_speller = tokens[real_idx].is_ignored_by_speller();
                    let was_immunized = tokens[real_idx].is_immunized();
                    tokens[real_idx] = AnalyzedTokenReadings::new(base).with_readings(kept);
                    tokens[real_idx].set_chunks(chunks);
                    if ignored_by_speller {
                        tokens[real_idx].ignore_spelling();
                    }
                    if was_immunized {
                        tokens[real_idx].immunize();
                    }
                }
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

    if has_structured {
        match_elements(elements, tokens, start_idx)
    } else {
        match_tokens(&pattern.tokens, tokens, start_idx)
    }
}

fn compile_disambig_element(
    compiler: &XmlCompiler,
    element: &og_xml::types::XmlPatternElement,
    pattern_case_sensitive: bool,
) -> og_xml::compiler::CompiledPatternElement {
    use og_xml::compiler::CompiledPatternElement;
    use og_xml::types::XmlPatternElement;
    match element {
        XmlPatternElement::Token(t) => CompiledPatternElement::Token(Box::new(
            compiler.compile_token_with_default(t, pattern_case_sensitive),
        )),
        XmlPatternElement::OrGroup(g) => CompiledPatternElement::OrGroup(
            g.alternatives
                .iter()
                .map(|t| compiler.compile_token_with_default(t, pattern_case_sensitive))
                .collect(),
        ),
        XmlPatternElement::AndGroup(g) => CompiledPatternElement::AndGroup(
            g.constraints
                .iter()
                .map(|t| compiler.compile_token_with_default(t, pattern_case_sensitive))
                .collect(),
        ),
    }
}

fn match_tokens(
    pattern_tokens: &[CompiledPatternToken],
    tokens: &[&AnalyzedTokenReadings],
    start_idx: usize,
) -> Option<MatchResult> {
    match_tokens_recursive(pattern_tokens, tokens, start_idx, 0, false)
}

fn match_tokens_preverified(
    pattern_tokens: &[CompiledPatternToken],
    tokens: &[&AnalyzedTokenReadings],
    start_idx: usize,
) -> Option<MatchResult> {
    match_tokens_recursive(pattern_tokens, tokens, start_idx, 0, true)
}

fn match_tokens_recursive(
    pattern_tokens: &[CompiledPatternToken],
    tokens: &[&AnalyzedTokenReadings],
    token_idx: usize,
    pattern_idx: usize,
    first_text_verified: bool,
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

    let first_matched = if pattern_idx == 0 && first_text_verified {
        token_matches_text_verified(pt, tokens, token_idx, token_idx > 0)
    } else {
        token_matches(pt, tokens, token_idx, token_idx > 0)
    };

    if first_matched {
        if pt.skip < 0 {
            let mut next_token = token_idx + 1;
            while next_token <= tokens.len() {
                if let Some(mut result) = match_tokens_recursive(
                    pattern_tokens,
                    tokens,
                    next_token,
                    pattern_idx + 1,
                    false,
                ) {
                    result.matched_positions.insert(0, token_idx);
                    return Some(result);
                }
                next_token += 1;
            }
        } else {
            let skip = pt.skip as usize;
            for dist in 0..=skip {
                let next_token = token_idx + 1 + dist;
                if next_token > tokens.len() {
                    break;
                }
                if let Some(mut result) = match_tokens_recursive(
                    pattern_tokens,
                    tokens,
                    next_token,
                    pattern_idx + 1,
                    false,
                ) {
                    result.matched_positions.insert(0, token_idx);
                    return Some(result);
                }
            }
        }
    }

    if is_optional {
        return match_tokens_recursive(
            pattern_tokens,
            tokens,
            token_idx,
            pattern_idx + 1,
            first_text_verified && pattern_idx == 0,
        );
    }

    None
}

fn token_matches_text_verified(
    pt: &CompiledPatternToken,
    tokens: &[&AnalyzedTokenReadings],
    token_idx: usize,
    has_previous: bool,
) -> bool {
    if token_idx >= tokens.len() {
        return false;
    }
    let token = tokens[token_idx];
    if token.is_immunized() {
        return false;
    }

    if has_previous && token_idx > 0 {
        let prev = tokens[token_idx - 1];
        let spaced = token.token().start() != prev.token().end();
        match pt.space_before.as_deref() {
            Some("no") if spaced => return false,
            Some("yes") if !spaced => return false,
            _ => {}
        }
    }

    token_matches_tail(
        pt,
        token,
        token.token().token(),
        token_idx > 0 && {
            let prev = tokens[token_idx - 1];
            token.token().start() != prev.token().end()
        },
    )
}

fn readings_match_postag_regex(
    plain: Option<&regex::Regex>,
    fancy: Option<&fancy_regex::Regex>,
    token: &AnalyzedTokenReadings,
) -> bool {
    if let Some(re) = plain {
        return token
            .readings()
            .iter()
            .any(|r| r.pos_tags().iter().any(|t| re.is_match(t)));
    }
    if let Some(re) = fancy {
        return token
            .readings()
            .iter()
            .any(|r| r.pos_tags().iter().any(|t| re.is_match(t).unwrap_or(false)));
    }
    false
}

fn token_matches_tail(
    pt: &CompiledPatternToken,
    token: &AnalyzedTokenReadings,
    text: &str,
    token_space_before: bool,
) -> bool {
    let pos_matches = if let Some(ref postag) = pt.postag {
        if pt.compiled_postag_regexp.is_some() || pt.compiled_postag_regexp_fancy.is_some() {
            let has_matching = readings_match_postag_regex(
                pt.compiled_postag_regexp.as_ref(),
                pt.compiled_postag_regexp_fancy.as_ref(),
                token,
            );
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
        if exc
            .space_before
            .is_some_and(|required| required != token_space_before)
        {
            continue;
        }
        let text_exc = if let Some(ref exc_text) = exc.text {
            if let Some(ref re) = exc.compiled_regexp {
                re.is_match(text)
            } else {
                text.eq_ignore_ascii_case(exc_text)
            }
        } else {
            false
        };
        let reading_matches = |r: &og_core::AnalyzedToken| {
            if let Some(ref postag) = exc.postag {
                if exc.compiled_postag_regexp.is_some()
                    || exc.compiled_postag_regexp_fancy.is_some()
                {
                    let plain = exc
                        .compiled_postag_regexp
                        .as_ref()
                        .is_some_and(|re| r.pos_tags().iter().any(|t| re.is_match(t)));
                    let fancy = exc.compiled_postag_regexp_fancy.as_ref().is_some_and(|re| {
                        r.pos_tags().iter().any(|t| re.is_match(t).unwrap_or(false))
                    });
                    plain || fancy
                } else {
                    r.pos_tags().iter().any(|t| t == postag)
                }
            } else {
                false
            }
        };
        let pos_exc = token.readings().iter().any(|r| reading_matches(r));

        let has_text_constraint = exc.text.is_some();
        let has_pos_constraint = exc.postag.is_some();
        let pos_ok = if exc.negate_pos {
            token.readings().iter().any(|r| !reading_matches(r))
        } else {
            pos_exc
        };
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

fn match_elements(
    elements: &[CompiledPatternElement],
    tokens: &[&AnalyzedTokenReadings],
    start_idx: usize,
) -> Option<MatchResult> {
    let mut positions = Vec::new();
    let mut token_idx = start_idx;

    for (elem_idx, elem) in elements.iter().enumerate() {
        if token_idx >= tokens.len() {
            return None;
        }

        match elem {
            CompiledPatternElement::Token(pt) => {
                if pt.skip != 0 && token_matches(pt, tokens, token_idx, token_idx > start_idx) {
                    if pt.skip < 0 {
                        let mut next = token_idx + 1;
                        while next <= tokens.len() {
                            if let Some(rest) =
                                match_elements(&elements[elem_idx + 1..], tokens, next)
                            {
                                let mut result = MatchResult {
                                    matched_positions: positions.clone(),
                                };
                                result.matched_positions.push(token_idx);
                                result.matched_positions.extend(rest.matched_positions);
                                return Some(result);
                            }
                            next += 1;
                        }
                    } else {
                        let mut off = 0usize;
                        while off <= pt.skip as usize {
                            let next = token_idx + 1 + off;
                            if next > tokens.len() {
                                break;
                            }
                            if let Some(rest) =
                                match_elements(&elements[elem_idx + 1..], tokens, next)
                            {
                                let mut result = MatchResult {
                                    matched_positions: positions.clone(),
                                };
                                result.matched_positions.push(token_idx);
                                result.matched_positions.extend(rest.matched_positions);
                                return Some(result);
                            }
                            off += 1;
                        }
                    }
                    return None;
                } else if token_matches(pt, tokens, token_idx, token_idx > start_idx) {
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
    if token.is_immunized() {
        return false;
    }
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

    token_matches_tail(
        pt,
        token,
        text,
        has_previous && token_idx > 0 && {
            let prev = tokens[token_idx - 1];
            token.token().start() != prev.token().end()
        },
    )
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

impl XmlDisambiguator {
    fn passes_english_word_filter(
        &self,
        rule: &CompiledDisambigRule,
        tokens: &[&AnalyzedTokenReadings],
        match_result: &MatchResult,
    ) -> bool {
        let Some(tagger) = self.english_tagger.as_ref() else {
            return false;
        };
        let Some(args) = rule.filter_args.as_deref() else {
            return false;
        };
        let Some(form_positions) = args.split(',').find_map(|kv| {
            let (k, v) = kv.split_once(':')?;
            (k.trim() == "formPositions").then_some(v.to_string())
        }) else {
            return false;
        };
        for pos_str in form_positions.split(',') {
            let Ok(one_based) = pos_str.trim().parse::<usize>() else {
                return false;
            };
            let Some(&token_idx) = match_result
                .matched_positions
                .get(one_based.saturating_sub(1))
            else {
                return false;
            };
            let Some(token) = tokens.get(token_idx) else {
                return false;
            };
            let readings = tagger.tag_word(token.token().token(), 0, 0);
            if readings.is_empty() {
                return false;
            }
        }
        true
    }
}

fn apply_filterall(
    tokens: &mut [AnalyzedTokenReadings],
    token_idx: usize,
    pattern_token: &CompiledPatternToken,
) {
    let Some(postag) = pattern_token.postag.as_deref() else {
        return;
    };
    let re = pattern_token
        .compiled_postag_regexp
        .as_ref()
        .map(|re| re.clone())
        .or_else(|| regex::Regex::new(&format!("^(?:{})$", postag)).ok());
    let Some(re) = re else {
        return;
    };
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
    let Some(real_idx) = real_idx else {
        return;
    };
    let prior_chunks = tokens[real_idx].chunks().to_vec();
    let ignored_by_speller = tokens[real_idx].is_ignored_by_speller();
    let was_immunized = tokens[real_idx].is_immunized();
    let base = tokens[real_idx].token().clone();
    let had_sent_end = current_had_sent_end(&[], &tokens[real_idx]);
    let current_readings = tokens[real_idx].readings().to_vec();
    let kept: Vec<AnalyzedToken> = current_readings
        .into_iter()
        .filter(|r| r.pos_tags().iter().any(|t| re.is_match(t)))
        .collect();
    if kept.is_empty() {
        return;
    }
    let mut updated = AnalyzedTokenReadings::new(base).with_readings(kept);
    restore_sent_end_if_lost(&mut updated, had_sent_end);
    updated.set_chunks(prior_chunks);
    if ignored_by_speller {
        updated.ignore_spelling();
    }
    if was_immunized {
        updated.immunize();
    }
    tokens[real_idx] = updated;
}

fn current_had_sent_end(filtered: &[AnalyzedToken], token: &AnalyzedTokenReadings) -> bool {
    let still_present = filtered
        .iter()
        .any(|r| r.pos_tags().iter().any(|t| t == "SENT_END"));
    !still_present
        && token
            .readings()
            .iter()
            .any(|r| r.pos_tags().iter().any(|t| t == "SENT_END"))
}

fn restore_sent_end_if_lost(updated: &mut AnalyzedTokenReadings, had_sent_end: bool) {
    if had_sent_end {
        updated.add_sent_end();
    }
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
    let ignored_by_speller = tokens[real_idx].is_ignored_by_speller();
    let was_immunized = tokens[real_idx].is_immunized();
    match action {
        DisambigAction::SetPos(pos) => {
            let base = tokens[real_idx].token().clone();
            let surviving_lemma = tokens[real_idx]
                .readings()
                .iter()
                .find(|r| r.pos_tags().iter().any(|t| t == pos))
                .and_then(|r| r.lemma().map(str::to_string));
            let mut new_reading = AnalyzedToken::new(base.token(), base.start(), base.end());
            new_reading.set_pos_tags(vec![pos.clone()]);
            new_reading.set_lemma(surviving_lemma.or_else(|| base.lemma().map(|l| l.to_string())));
            let mut updated = AnalyzedTokenReadings::new(base).with_readings(vec![new_reading]);
            updated.set_chunks(prior_chunks.clone());
            if ignored_by_speller {
                updated.ignore_spelling();
            }
            if was_immunized {
                updated.immunize();
            }
            tokens[real_idx] = updated;
        }
        DisambigAction::Replace(wds) => {
            let base = tokens[real_idx].token().clone();
            let new_readings: Vec<AnalyzedToken> = wds
                .iter()
                .map(|w| {
                    let token_text = if w.text.is_empty() {
                        base.token().to_string()
                    } else {
                        w.text.clone()
                    };
                    let mut reading = AnalyzedToken::new(&token_text, base.start(), base.end());
                    if let Some(pos) = &w.pos {
                        reading.set_pos_tags(vec![pos.clone()]);
                    }
                    reading.set_lemma(Some(w.lemma.clone().unwrap_or(token_text)));
                    reading
                })
                .collect();
            if !new_readings.is_empty() {
                let mut updated = AnalyzedTokenReadings::new(base).with_readings(new_readings);
                updated.set_chunks(prior_chunks.clone());
                if ignored_by_speller {
                    updated.ignore_spelling();
                }
                if was_immunized {
                    updated.immunize();
                }
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
            let mut filtered = filtered;
            let had_sent_end = current_had_sent_end(&filtered, &tokens[real_idx]);
            if filtered.is_empty() {
                filtered.push(AnalyzedToken::new(base.token(), base.start(), base.end()));
            }
            let mut updated = AnalyzedTokenReadings::new(base).with_readings(filtered);
            restore_sent_end_if_lost(&mut updated, had_sent_end);
            updated.set_chunks(prior_chunks.clone());
            if ignored_by_speller {
                updated.ignore_spelling();
            }
            if was_immunized {
                updated.immunize();
            }
            tokens[real_idx] = updated;
        }
        DisambigAction::RemovePostag(_) => {
            let DisambigActionRegex::RemovePostag(re, re_fancy) = matcher else {
                return;
            };
            if re.is_none() && re_fancy.is_none() {
                return;
            }
            let tag_matches = |t: &str| -> bool {
                re.as_ref().is_some_and(|re| re.is_match(t))
                    || re_fancy
                        .as_ref()
                        .is_some_and(|re| re.is_match(t).unwrap_or(false))
            };
            let base = tokens[real_idx].token().clone();
            let current_readings = tokens[real_idx].readings().to_vec();
            let mut filtered: Vec<AnalyzedToken> = current_readings
                .into_iter()
                .filter(|r| !r.pos_tags().iter().any(|t| tag_matches(t)))
                .collect();
            let had_sent_end = current_had_sent_end(&filtered, &tokens[real_idx]);
            if filtered.is_empty() {
                filtered.push(AnalyzedToken::new(base.token(), base.start(), base.end()));
            }
            let mut updated = AnalyzedTokenReadings::new(base).with_readings(filtered);
            restore_sent_end_if_lost(&mut updated, had_sent_end);
            updated.set_chunks(prior_chunks.clone());
            if ignored_by_speller {
                updated.ignore_spelling();
            }
            if was_immunized {
                updated.immunize();
            }
            tokens[real_idx] = updated;
        }
        DisambigAction::Add(wds) => {
            let current_readings = tokens[real_idx].readings().to_vec();
            let base = tokens[real_idx].token().clone();
            let mut new_readings: Vec<AnalyzedToken> =
                Vec::with_capacity(current_readings.len() + wds.len());
            if let Some((last, head)) = current_readings.split_last() {
                new_readings.extend_from_slice(head);
                if !last.pos_tags().is_empty() {
                    new_readings.push(last.clone());
                }
            }
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
            if ignored_by_speller {
                updated.ignore_spelling();
            }
            if was_immunized {
                updated.immunize();
            }
            tokens[real_idx] = updated;
        }
        DisambigAction::Filter { .. } => {
            let DisambigActionRegex::Filter(re, re_fancy) = matcher else {
                return;
            };
            if re.is_none() && re_fancy.is_none() {
                return;
            }
            let tag_matches = |t: &str| -> bool {
                re.as_ref().is_some_and(|re| re.is_match(t))
                    || re_fancy
                        .as_ref()
                        .is_some_and(|re| re.is_match(t).unwrap_or(false))
            };
            let base = tokens[real_idx].token().clone();
            let current_readings = tokens[real_idx].readings().to_vec();
            let filtered: Vec<AnalyzedToken> = current_readings
                .into_iter()
                .filter(|r| r.pos_tags().iter().any(|t| tag_matches(t)))
                .collect();
            let had_sent_end = current_had_sent_end(&filtered, &tokens[real_idx]);
            if !filtered.is_empty() {
                let mut updated = AnalyzedTokenReadings::new(base).with_readings(filtered);
                restore_sent_end_if_lost(&mut updated, had_sent_end);
                updated.set_chunks(prior_chunks.clone());
                if ignored_by_speller {
                    updated.ignore_spelling();
                }
                if was_immunized {
                    updated.immunize();
                }
                tokens[real_idx] = updated;
            }
        }
        DisambigAction::FilterAll => {
            let base = tokens[real_idx].token().clone();
            let empty_reading = AnalyzedToken::new(base.token(), base.start(), base.end());
            let mut updated = AnalyzedTokenReadings::new(base).with_readings(vec![empty_reading]);
            updated.set_chunks(prior_chunks.clone());
            if ignored_by_speller {
                updated.ignore_spelling();
            }
            if was_immunized {
                updated.immunize();
            }
            tokens[real_idx] = updated;
        }
        DisambigAction::IgnoreSpelling => {}
        DisambigAction::Unify => {}
        DisambigAction::Immunize => {}
    }
}

fn compile_disambig_rule(compiler: &XmlCompiler, rule: DisambigRule) -> CompiledDisambigRule {
    CompiledDisambigRule {
        id: rule.id,
        name: rule.name,
        filter_class: rule.filter_class,
        filter_args: rule.filter_args,
        pattern: CompiledPattern {
            marker_elem_start: rule.pattern.marker_elem_start,
            marker_elem_end: rule.pattern.marker_elem_end,
            unify_groups: Vec::new(),
            tokens: rule
                .pattern
                .tokens
                .iter()
                .map(|t| compiler.compile_token_with_default(t, rule.pattern.case_sensitive))
                .collect(),
            elements: rule
                .pattern
                .elements
                .iter()
                .map(|e| compile_disambig_element(compiler, e, rule.pattern.case_sensitive))
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
                    .map(|t| compiler.compile_token_with_default(t, ap.case_sensitive))
                    .collect(),
                elements: ap
                    .elements
                    .iter()
                    .map(|e| compile_disambig_element(compiler, e, ap.case_sensitive))
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
    fn negate_pos_exception_fires_on_any_non_matching_reading() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="de">
    <rule id="PRP_NOM" name="Remove NOM after pure preposition">
        <pattern>
            <token postag_regexp="yes" postag="PRP.+">
                <exception postag="PRP.+" postag_regexp="yes" negate_pos="yes"/>
            </token>
            <marker>
                <and>
                    <token postag_regexp="yes" postag=".+NOM:.+"/>
                    <token postag_regexp="yes" postag=".+(AKK|DAT|GEN):.+"/>
                </and>
            </marker>
        </pattern>
        <disambig action="remove" postag=".+:NOM:.+"/>
    </rule>
</rules>"#;
        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        let mut mixed = make_sentence(vec![
            ("mit", vec!["PRP:TMP+MOD+CAU:DAT", "ZUS"]),
            ("Wasser", vec!["SUB:NOM:SIN:NEU", "SUB:AKK:SIN:NEU"]),
        ]);
        disambig.disambiguate(&mut mixed);
        assert!(
            mixed.tokens()[1].has_pos_tag("SUB:NOM:SIN:NEU"),
            "a preposition with a non-PRP reading must be excluded by the negate_pos exception"
        );
        let mut pure = make_sentence(vec![
            ("für", vec!["PRP:AKK"]),
            ("Wasser", vec!["SUB:NOM:SIN:NEU", "SUB:AKK:SIN:NEU"]),
        ]);
        disambig.disambiguate(&mut pure);
        assert!(
            !pure.tokens()[1].has_pos_tag("SUB:NOM:SIN:NEU"),
            "a pure preposition still triggers the NOM removal"
        );
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
    fn test_ignore_spelling_marks_full_match_range() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="CRON" name="ignore cron job">
        <pattern>
            <token>cron</token>
            <token regexp="yes">jobs?</token>
        </pattern>
        <disambig action="ignore_spelling"/>
    </rule>
    <rule id="CA" name="ignore ca in ca n't">
        <pattern>
            <marker>
                <token>ca</token>
            </marker>
            <token spacebefore="no">n't</token>
        </pattern>
        <disambig action="ignore_spelling"/>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();

        let mut sentence = AnalyzedSentence::new("cron job", 0, 8);
        let mut tokens = Vec::new();
        tokens.push(AnalyzedTokenReadings::new(
            AnalyzedToken::new("cron", 0, 4).with_pos_tags(vec!["NN".to_string()]),
        ));
        tokens.push(AnalyzedTokenReadings::new(
            AnalyzedToken::new("job", 5, 8).with_pos_tags(vec!["NN".to_string()]),
        ));
        sentence.set_tokens(tokens);
        disambig.disambiguate(&mut sentence);
        assert!(sentence.tokens()[0].is_ignored_by_speller());
        assert!(sentence.tokens()[1].is_ignored_by_speller());
        assert!(sentence.tokens()[0].has_pos_tag("NN"));

        let mut sentence = AnalyzedSentence::new("ca n't", 0, 5);
        let mut tokens = Vec::new();
        tokens.push(AnalyzedTokenReadings::new(
            AnalyzedToken::new("ca", 0, 2).with_pos_tags(vec!["NN".to_string()]),
        ));
        tokens.push(AnalyzedTokenReadings::new(
            AnalyzedToken::new("n't", 2, 5).with_pos_tags(vec!["RB".to_string()]),
        ));
        sentence.set_tokens(tokens);
        disambig.disambiguate(&mut sentence);
        assert!(sentence.tokens()[0].is_ignored_by_speller());
        assert!(!sentence.tokens()[1].is_ignored_by_speller());
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
    fn skip_means_up_to_n_tokens_not_exactly_n() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="de">
    <rule id="SKIP_ANTI" name="antipattern with skip">
        <antipattern>
            <token skip="2">ich</token>
            <token>geb</token>
        </antipattern>
        <pattern>
            <marker>
                <token>geb</token>
            </marker>
        </pattern>
        <disambig action="remove" postag="VER:.+"/>
    </rule>
</rules>"#;

        let disambiguator = XmlDisambiguator::from_xml(xml).unwrap();
        let mut sentence = make_sentence(vec![("Ich", vec!["PRO"]), ("geb", vec!["VER:1", "SUB"])]);

        let geb = sentence.tokens_mut()[1].token().clone();
        sentence.tokens_mut()[1] = AnalyzedTokenReadings::new(geb).with_readings(vec![
            AnalyzedToken::new("geb", 4, 7).with_pos_tags(vec!["VER:1".to_string()]),
            AnalyzedToken::new("geb", 4, 7).with_pos_tags(vec!["SUB".to_string()]),
        ]);
        disambiguator.disambiguate(&mut sentence);
        assert!(
            sentence.tokens()[1].has_pos_tag("VER:1"),
            "skip=2 antipattern must match at distance 1 and block the removal"
        );

        let mut far = make_sentence(vec![
            ("Ich", vec!["PRO"]),
            ("x", vec!["ADV"]),
            ("y", vec!["ADV"]),
            ("z", vec!["ADV"]),
            ("geb", vec!["VER:1", "SUB"]),
        ]);
        let geb = far.tokens_mut()[4].token().clone();
        far.tokens_mut()[4] = AnalyzedTokenReadings::new(geb).with_readings(vec![
            AnalyzedToken::new("geb", 8, 11).with_pos_tags(vec!["VER:1".to_string()]),
            AnalyzedToken::new("geb", 8, 11).with_pos_tags(vec!["SUB".to_string()]),
        ]);
        disambiguator.disambiguate(&mut far);
        assert!(
            !far.tokens()[3].has_pos_tag("VER:1"),
            "beyond the skip range the antipattern must not block the removal"
        );
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
    fn test_immunize_action_marks_all_matched_tokens() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="de">
    <rule id="LOS_ANGELES" name="Los Angeles">
        <pattern>
            <token>Los</token>
            <token>Angeles</token>
        </pattern>
        <disambig action="immunize"/>
    </rule>
    <rule id="AFTER" name="filter after immunize">
        <pattern>
            <token>Angeles</token>
        </pattern>
        <disambig action="replace"><wd pos="NN"/></disambig>
    </rule>
</rules>"#;

        let disambig = XmlDisambiguator::from_xml(xml).unwrap();
        let mut sentence = make_sentence(vec![("Los", vec!["NE"]), ("Angeles", vec!["NE"])]);

        assert!(!sentence.tokens()[0].is_immunized());
        assert!(!sentence.tokens()[1].is_immunized());

        disambig.disambiguate(&mut sentence);

        assert!(sentence.tokens()[0].is_immunized());
        assert!(sentence.tokens()[1].is_immunized());
        assert!(
            sentence.tokens()[1].has_pos_tag("NE"),
            "immunized token must be invisible to later rules"
        );
    }

    #[test]
    fn test_filter_action_requires_full_postag_match() {
        let exact = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="FILT" name="Filter exact">
        <pattern>
            <token>run</token>
        </pattern>
        <disambig action="filter" postag="NN"/>
    </rule>
</rules>"#;
        let disambig = XmlDisambiguator::from_xml(exact).unwrap();
        let mut sentence = make_sentence(vec![("run", vec!["NN", "VB", "NNP"])]);
        disambig.disambiguate(&mut sentence);
        let token = &sentence.tokens()[0];
        assert!(token.has_pos_tag("NN"));
        assert!(!token.has_pos_tag("VB"), "partial matches must be dropped");
        assert!(!token.has_pos_tag("NNP"), "postag N must not match NNP");

        let no_match = r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="en">
    <rule id="FILT" name="Filter no full match">
        <pattern>
            <token>run</token>
        </pattern>
        <disambig action="filter" postag="V"/>
    </rule>
</rules>"#;
        let disambig = XmlDisambiguator::from_xml(no_match).unwrap();
        let mut sentence = make_sentence(vec![("run", vec!["NN", "VB"])]);
        disambig.disambiguate(&mut sentence);
        let token = &sentence.tokens()[0];
        assert!(
            token.has_pos_tag("NN") && token.has_pos_tag("VB"),
            "no full match means the filter is a no-op"
        );
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

    const DE_UNIFICATIONS: &str = r#"
    <unification feature="number">
        <equivalence type="sg"><token postag=".*:SIN:.*" postag_regexp="yes"/></equivalence>
        <equivalence type="pl"><token postag=".*:PLU:.*" postag_regexp="yes"/></equivalence>
    </unification>
    <unification feature="case">
        <equivalence type="nom"><token postag=".*:NOM:.*" postag_regexp="yes"/></equivalence>
        <equivalence type="akk"><token postag=".*:AKK:.*" postag_regexp="yes"/></equivalence>
        <equivalence type="dat"><token postag=".*:DAT:.*" postag_regexp="yes"/></equivalence>
        <equivalence type="gen"><token postag=".*:GEN:.*" postag_regexp="yes"/></equivalence>
    </unification>
    <unification feature="gender">
        <equivalence type="mas"><token postag=".*:(MAS|NOG).*" postag_regexp="yes"/></equivalence>
        <equivalence type="fem"><token postag=".*:(FEM|NOG).*" postag_regexp="yes"/></equivalence>
        <equivalence type="neu"><token postag=".*:(NEU|NOG).*" postag_regexp="yes"/></equivalence>
    </unification>"#;

    fn det_noun_rule(unify_attrs: &str, extra: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="de">{unifications}
    <rule id="UNIFY_DET_SUB" name="det sub">
        <pattern>
            <unify{unify_attrs}>
                <feature id="number"/><feature id="case"/><feature id="gender"/>
                <token regexp="yes">der|die|das</token>
                <token postag="SUB:.*" postag_regexp="yes"/>
            </unify>
        </pattern>
        {extra}
    </rule>
</rules>"#,
            unifications = DE_UNIFICATIONS,
            unify_attrs = unify_attrs,
            extra = extra
        )
    }

    #[test]
    fn test_unify_threads_all_declared_features_per_reading() {
        let disambiguator =
            XmlDisambiguator::from_xml(&det_noun_rule("", r#"<disambig action="unify"/>"#))
                .unwrap();
        let mut sentence = make_sentence(vec![
            ("Der", vec!["ART:NOM:SIN:MAS", "ART:GEN:PLU:NEU"]),
            ("Haus", vec!["SUB:NOM:SIN:NEU", "SUB:NOM:SIN:MAS"]),
        ]);
        disambiguator.disambiguate(&mut sentence);
        let der = &sentence.tokens()[0];
        assert_eq!(der.readings().len(), 1);
        assert!(der.has_pos_tag("ART:NOM:SIN:MAS"));
        let haus = &sentence.tokens()[1];
        assert_eq!(haus.readings().len(), 1);
        assert!(haus.has_pos_tag("SUB:NOM:SIN:MAS"));
    }

    #[test]
    fn test_unify_leaves_readings_untouched_when_sequence_does_not_unify() {
        let disambiguator =
            XmlDisambiguator::from_xml(&det_noun_rule("", r#"<disambig action="unify"/>"#))
                .unwrap();
        let mut sentence = make_sentence(vec![
            ("Der", vec!["ART:NOM:SIN:MAS"]),
            ("Harz", vec!["SUB:AKK:SIN:NEU", "SUB:NOM:SIN:NEU"]),
        ]);
        disambiguator.disambiguate(&mut sentence);
        assert_eq!(sentence.tokens()[0].readings().len(), 1);
        assert_eq!(sentence.tokens()[1].readings().len(), 2);
    }

    #[test]
    fn test_unify_ignore_position_keeps_all_its_readings() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<rules lang="de">{unifications}
    <rule id="UNIFY_NEUTRAL" name="neutral">
        <pattern>
            <unify>
                <feature id="number"/>
                <token postag="ART:.*" postag_regexp="yes"/>
                <unify-ignore>
                    <token>und</token>
                </unify-ignore>
                <token postag="SUB:.*" postag_regexp="yes"/>
            </unify>
        </pattern>
        <disambig action="unify"/>
    </rule>
</rules>"#,
            unifications = DE_UNIFICATIONS
        );
        let disambiguator = XmlDisambiguator::from_xml(&xml).unwrap();
        let mut sentence = make_sentence(vec![
            ("die", vec!["ART:NOM:SIN:MAS", "ART:NOM:PLU:MAS"]),
            ("und", vec!["KON", "SUB:NOM:PLU:NEU"]),
            ("Haus", vec!["SUB:NOM:SIN:NEU"]),
        ]);
        disambiguator.disambiguate(&mut sentence);
        assert!(sentence.tokens()[0].has_pos_tag("ART:NOM:SIN:MAS"));
        assert!(
            !sentence.tokens()[0].has_pos_tag("ART:NOM:PLU:MAS"),
            "the plural article finds no singular partner"
        );
        assert_eq!(
            sentence.tokens()[1].readings().len(),
            2,
            "neutral positions keep every reading"
        );
        assert!(sentence.tokens()[2].has_pos_tag("SUB:NOM:SIN:NEU"));
    }

    #[test]
    fn test_negated_unify_blocks_the_rule_when_the_sequence_unifies() {
        let disambiguator = XmlDisambiguator::from_xml(&det_noun_rule(
            r#" negate="yes""#,
            r#"<disambig action="immunize"/>"#,
        ))
        .unwrap();
        let mut unified = make_sentence(vec![
            ("Der", vec!["ART:NOM:SIN:MAS"]),
            ("Haus", vec!["SUB:NOM:SIN:MAS"]),
        ]);
        disambiguator.disambiguate(&mut unified);
        assert!(
            !unified.tokens()[0].is_immunized(),
            "unifying sequence must fail a negated unify rule"
        );

        let mut split = make_sentence(vec![
            ("Der", vec!["ART:NOM:SIN:MAS"]),
            ("Harz", vec!["SUB:NOM:SIN:NEU"]),
        ]);
        disambiguator.disambiguate(&mut split);
        assert!(split.tokens()[0].is_immunized());
    }
}
