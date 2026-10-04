use crate::es::test_support::SpanishPipeline;
use og_rules::pattern_rule::PatternRuleEngine;
use og_xml::compiler::XmlCompiler;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn grammar_path() -> PathBuf {
    PathBuf::from(
        "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/es/src/main/resources/org/languagetool/rules/es/grammar.xml",
    )
}

fn build_pattern_engine() -> PatternRuleEngine {
    let resource_dir = crate::es::es_resource_dir().expect("es resource dir");
    let core_resource_dir = crate::engine::find_lt_core_resource_dir().expect("core resource dir");
    let mut dict = og_spell::Dictionary::new();
    for name in ["hunspell/spelling.txt", "added.txt"] {
        if let Ok(words) = std::fs::read_to_string(resource_dir.join(name)) {
            for line in words.lines() {
                let word = line.trim();
                if !word.is_empty() && !word.starts_with('#') {
                    dict.add_word(word);
                }
            }
        }
    }
    let support = og_rules::spelling_support::SpellingSupport::new_for_language(
        Arc::new(dict),
        &core_resource_dir,
        &resource_dir,
        "es",
    );
    let synth = es_synth();
    let mut engine = PatternRuleEngine::new().with_spelling_support(Arc::new(support));
    if let Some(synth) = synth {
        engine = engine
            .with_synthesizer(synth.clone())
            .with_suggestion_synthesizer(synth);
    }
    engine
}

fn es_synth() -> Option<Arc<og_rules::postponed_adjective::LangSynthesizer>> {
    static CACHE: std::sync::OnceLock<Option<Arc<og_rules::postponed_adjective::LangSynthesizer>>> =
        std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            crate::es::es_resource_dir().and_then(|dir| {
                og_rules::postponed_adjective::LangSynthesizer::for_language(&dir, "es")
                    .map(Arc::new)
            })
        })
        .clone()
}

fn reconstruct_matched_positions(
    rule: &og_xml::compiler::CompiledRule,
    tokens: &[&og_core::AnalyzedTokenReadings],
    from: usize,
    to: usize,
) -> Option<Vec<usize>> {
    let mut span: Vec<usize> = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        if t.token().end() > from && t.token().start() < to {
            span.push(i);
        }
    }
    let first_span = *span.first()?;
    let mut last = *span.last()?;
    let pattern = &rule.pattern;
    let (marker_start, marker_end, total) = if pattern.has_structured_elements {
        (
            pattern.marker_elem_start,
            pattern.marker_elem_end,
            pattern.elements.len(),
        )
    } else {
        (
            pattern.marker_start,
            pattern.marker_end,
            pattern.tokens.len(),
        )
    };
    let is_plain = |slice: &[og_xml::compiler::CompiledPatternToken]| {
        slice
            .iter()
            .all(|t| t.skip == 0 && t.min.unwrap_or(1) == 1 && t.max.unwrap_or(1) == 1)
    };
    let pre_count = marker_start.unwrap_or(0);
    if pre_count > 0 && !is_plain(pattern.tokens.get(..pre_count)?) {
        return None;
    }
    let post_count = marker_end.map_or(0, |end| total - end);
    if post_count > 0 && !is_plain(pattern.tokens.get(marker_end?..)?) {
        return None;
    }
    let first = first_span.checked_sub(pre_count)?;
    last += post_count;
    if first > last || last >= tokens.len() {
        return None;
    }
    Some((first..=last).collect())
}

fn apply_es_filters(
    rule: &og_xml::compiler::CompiledRule,
    tokens: &[&og_core::AnalyzedTokenReadings],
    sentence_text: &str,
    m: og_core::RuleMatch,
) -> Option<og_core::RuleMatch> {
    let Some(filter) = rule
        .filter
        .as_ref()
        .filter(|f| crate::es::filters::is_es_filter(&f.class))
    else {
        return Some(m);
    };
    let from = m.offset();
    let to = m.offset() + m.length();
    let Some(matched_positions) = reconstruct_matched_positions(rule, tokens, from, to) else {
        return Some(m);
    };
    let matched_texts: Vec<String> = matched_positions
        .iter()
        .map(|&i| tokens[i].token().token().to_string())
        .collect();
    let synthesizer = es_synth();
    let call = crate::es::filters::EsFilterCall {
        args: &filter.args,
        matched_texts: &matched_texts,
        matched_positions: &matched_positions,
        tokens,
        sentence_text,
        synthesizer: synthesizer.as_deref(),
    };
    crate::es::filters::apply_es_filter(&filter.class, call, m)
}

fn extract_marker(text: &str) -> (String, Option<(usize, usize)>) {
    match (text.find("<marker>"), text.find("</marker>")) {
        (Some(s), Some(e)) => {
            let clean = text.replace("<marker>", "").replace("</marker>", "");
            (clean, Some((s, e - "<marker>".len())))
        }
        _ => (text.to_string(), None),
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Outcome {
    Passed,
    NoMatch,
    FalsePositive,
    Marker,
}

#[test]
#[ignore]
fn spanish_grammar_examples_with_es_pipeline() {
    let xml = std::fs::read_to_string(grammar_path()).expect("grammar.xml");
    let raw_example_count = xml.matches("<example").count();
    let rule_set = XmlCompiler::new()
        .compile_file_with_base(&xml, grammar_path().parent())
        .expect("compile grammar.xml");

    let mut group_index: HashMap<(String, Option<String>), usize> = HashMap::new();
    let mut groups: Vec<Vec<&og_xml::compiler::CompiledRule>> = Vec::new();
    for rule in &rule_set.rules {
        let key = (rule.id.clone(), rule.sub_id.clone());
        let next = groups.len();
        let idx = *group_index.entry(key).or_insert(next);
        if idx == next {
            groups.push(vec![rule]);
        } else {
            groups[idx].push(rule);
        }
    }

    let compiled_example_count: usize = groups
        .iter()
        .map(|g| g.first().map(|r| r.examples.len()).unwrap_or(0))
        .sum();

    let engine = build_pattern_engine();
    let pipeline = SpanishPipeline::shared();
    og_rules::pattern_rule::set_test_date_hack(true);

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut outcome_counts: HashMap<Outcome, usize> = HashMap::new();
    let mut fail_lines: Vec<String> = Vec::new();
    let started = std::time::Instant::now();

    for group in &groups {
        let Some(rule) = group.first() else {
            continue;
        };
        for example in &rule.examples {
            let (clean_text, marker_range) = extract_marker(&example.text);
            let mut matches = Vec::new();
            for aux in group.iter() {
                for (sentence, sentence_start) in pipeline.analyze_offset_text(&clean_text) {
                    let tokens: Vec<&og_core::AnalyzedTokenReadings> =
                        sentence.non_whitespace_tokens();
                    for m in engine.match_rule(aux, &sentence) {
                        let Some(mut m) = apply_es_filters(aux, &tokens, sentence.text(), m) else {
                            continue;
                        };
                        m.set_offset_length(m.offset() + sentence_start, m.length());
                        matches.push(m);
                    }
                }
            }
            let (ok, outcome, message) = match example.example_type {
                og_xml::types::XmlExampleType::Incorrect => {
                    if matches.is_empty() {
                        (
                            false,
                            Outcome::NoMatch,
                            "Expected match on incorrect example but got none".to_string(),
                        )
                    } else if let Some((m_start, m_end)) = marker_range {
                        let first = &matches[0];
                        let match_start = first.offset();
                        let match_end = match_start + first.length();
                        if match_start <= m_start && match_end >= m_end {
                            (true, Outcome::Passed, String::new())
                        } else {
                            (
                                false,
                                Outcome::Marker,
                                format!(
                                    "Match {}..{} does not contain marker {}..{}",
                                    match_start, match_end, m_start, m_end
                                ),
                            )
                        }
                    } else {
                        (true, Outcome::Passed, String::new())
                    }
                }
                og_xml::types::XmlExampleType::Correct => {
                    if matches.is_empty() {
                        (true, Outcome::Passed, String::new())
                    } else {
                        (
                            false,
                            Outcome::FalsePositive,
                            format!(
                                "Expected no match on correct example but got {}",
                                matches.len()
                            ),
                        )
                    }
                }
                og_xml::types::XmlExampleType::TriggersError => {
                    (true, Outcome::Passed, String::new())
                }
            };
            if ok {
                passed += 1;
            } else {
                failed += 1;
                *outcome_counts.entry(outcome).or_default() += 1;
                fail_lines.push(format!("FAIL: [{}] {}: '{}'", rule.id, message, clean_text));
            }
        }
    }

    eprintln!(
        "es module pipeline: {}/{} passed ({:.1}%) - no_match: {}, fp: {}, marker: {} [{:.1}s]",
        passed,
        passed + failed,
        100.0 * passed as f64 / (passed + failed).max(1) as f64,
        outcome_counts.get(&Outcome::NoMatch).copied().unwrap_or(0),
        outcome_counts
            .get(&Outcome::FalsePositive)
            .copied()
            .unwrap_or(0),
        outcome_counts.get(&Outcome::Marker).copied().unwrap_or(0),
        started.elapsed().as_secs_f64(),
    );
    let all_compiled_examples: usize = rule_set.rules.iter().map(|r| r.examples.len()).sum();
    let non_first_examples = all_compiled_examples - compiled_example_count;
    eprintln!(
        "accepted-excluded residue: {raw_example_count} <example> tags in grammar.xml (112 inside XML comments), {all_compiled_examples} on compiled rules, {non_first_examples} on non-first rules of duplicate (id,sub_id) groups, {compiled_example_count} evaluated",
    );
    let _ = std::fs::write(
        "/tmp/test_failures_es_module.txt",
        fail_lines.join("\n") + "\n",
    );
}
