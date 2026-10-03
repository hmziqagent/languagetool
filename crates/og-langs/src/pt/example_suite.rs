use crate::pt::test_support::PortuguesePipeline;
use og_rules::pattern_rule::PatternRuleEngine;
use og_xml::compiler::XmlCompiler;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

fn grammar_path() -> PathBuf {
    crate::engine::find_lt_resource_dir()
        .expect("lt resource dir")
        .join("pt/src/main/resources/org/languagetool/rules/pt/grammar.xml")
}

fn build_pattern_engine() -> PatternRuleEngine {
    let resource_dir = crate::pt::tagger::pt_resource_dir().expect("pt resource dir");
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
        "pt",
    );
    let synth = og_rules::postponed_adjective::LangSynthesizer::for_language(&resource_dir, "pt")
        .map(Arc::new);
    let mut engine = PatternRuleEngine::new().with_spelling_support(Arc::new(support));
    if let Some(synth) = synth {
        engine = engine
            .with_synthesizer(synth.clone())
            .with_suggestion_synthesizer(synth);
    }
    engine
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
    NoMatch,
    FalsePositive,
    Marker,
}

#[test]
#[ignore]
fn portuguese_grammar_examples_with_pt_pipeline() {
    let xml = std::fs::read_to_string(grammar_path()).expect("grammar.xml");
    let raw_example_count = xml.matches("<example").count();
    let without_comments = strip_xml_comments(&xml);
    let comment_example_count = raw_example_count - without_comments.matches("<example").count();
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

    let compiled_example_count: usize = rule_set.rules.iter().map(|r| r.examples.len()).sum();
    let evaluated_example_count: usize = groups
        .iter()
        .map(|g| g.first().map(|r| r.examples.len()).unwrap_or(0))
        .sum();

    let engine = build_pattern_engine();
    let pipeline = PortuguesePipeline::shared();
    og_rules::pattern_rule::set_test_date_hack(true);

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut outcome_counts: HashMap<Outcome, usize> = HashMap::new();
    let mut fail_lines: Vec<String> = Vec::new();
    let mut fail_by_rule: HashMap<String, usize> = HashMap::new();
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
                    for mut m in engine.match_rule(aux, &sentence) {
                        m.set_offset_length(m.offset() + sentence_start, m.length());
                        matches.push(m);
                    }
                }
            }
            let failure: Option<(Outcome, String)> = match example.example_type {
                og_xml::types::XmlExampleType::Incorrect => {
                    if matches.is_empty() {
                        Some((
                            Outcome::NoMatch,
                            "Expected match on incorrect example but got none".to_string(),
                        ))
                    } else if let Some((m_start, m_end)) = marker_range {
                        let first = &matches[0];
                        let match_start = first.offset();
                        let match_end = match_start + first.length();
                        if match_start <= m_start && match_end >= m_end {
                            None
                        } else {
                            Some((
                                Outcome::Marker,
                                format!(
                                    "Match {}..{} does not contain marker {}..{}",
                                    match_start, match_end, m_start, m_end
                                ),
                            ))
                        }
                    } else {
                        None
                    }
                }
                og_xml::types::XmlExampleType::Correct => {
                    if matches.is_empty() {
                        None
                    } else {
                        Some((
                            Outcome::FalsePositive,
                            format!(
                                "Expected no match on correct example but got {}",
                                matches.len()
                            ),
                        ))
                    }
                }
                og_xml::types::XmlExampleType::TriggersError => None,
            };
            match failure {
                None => passed += 1,
                Some((outcome, message)) => {
                    failed += 1;
                    *outcome_counts.entry(outcome).or_default() += 1;
                    *fail_by_rule.entry(rule.id.clone()).or_default() += 1;
                    fail_lines.push(format!("FAIL: [{}] {}: '{}'", rule.id, message, clean_text));
                }
            }
        }
    }

    eprintln!(
        "pt module pipeline: {}/{} passed ({:.1}%) - no_match: {}, fp: {}, marker: {} [{:.1}s]",
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
    eprintln!(
        "accepted-excluded residue: {raw_example_count} <example> tags in grammar.xml ({comment_example_count} inside XML comments), {compiled_example_count} on compiled rules, {} on non-first rules of duplicate (id,sub_id) groups, {evaluated_example_count} evaluated",
        compiled_example_count - evaluated_example_count,
    );
    let mut top_rules: Vec<(String, usize)> = fail_by_rule.into_iter().collect();
    top_rules.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    eprintln!(
        "top failing rules: {}",
        top_rules
            .iter()
            .take(12)
            .map(|(id, n)| format!("{id}({n})"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let _ = std::fs::write(
        "/tmp/test_failures_pt_module.txt",
        fail_lines.join("\n") + "\n",
    );
    assert!(passed + failed > 0, "suite must evaluate examples");
}

fn strip_xml_comments(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        match after.find("-->") {
            Some(end) => {
                rest = &after[end + 3..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}
