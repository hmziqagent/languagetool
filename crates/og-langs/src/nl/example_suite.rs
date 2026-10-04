use crate::nl::test_support::DutchPipeline;
use og_rules::pattern_rule::PatternRuleEngine;
use og_rules::postponed_adjective::LangSynthesizer;
use og_xml::compiler::{CompiledRule, CompiledRuleSet, XmlCompiler};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn rules_dir() -> PathBuf {
    crate::nl::nl_rules_dir().expect("nl rules dir")
}

fn grammar_path() -> PathBuf {
    rules_dir().join("grammar.xml")
}

fn style_path() -> PathBuf {
    rules_dir().join("style.xml")
}

fn variant_grammar_path() -> PathBuf {
    rules_dir().join("nl-NL/grammar.xml")
}

pub(crate) fn suite_engine() -> PatternRuleEngine {
    let resource_dir = crate::nl::tagger::nl_resource_dir().expect("nl resource dir");
    let core_resource_dir = crate::engine::find_lt_core_resource_dir().expect("core resource dir");
    let dictionary = crate::nl::speller::shared_speller_dictionary()
        .unwrap_or_else(|| Arc::new(og_spell::Dictionary::new()));
    let support = og_rules::spelling_support::SpellingSupport::new_for_language(
        dictionary,
        &core_resource_dir,
        &resource_dir,
        "nl",
    );
    let synth = suite_synthesizer().map(Arc::new);
    let mut engine = PatternRuleEngine::new().with_spelling_support(Arc::new(support));
    if let Some(synth) = synth {
        engine = engine
            .with_synthesizer(synth.clone())
            .with_suggestion_synthesizer(synth);
    }
    engine
}

pub(crate) fn suite_synthesizer() -> Option<LangSynthesizer> {
    let resource_dir = crate::nl::tagger::nl_resource_dir()?;
    LangSynthesizer::for_language(&resource_dir, "nl")
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
fn dutch_grammar_examples_with_nl_pipeline() {
    evaluate_examples_file(
        &grammar_path(),
        "grammar.xml",
        "/tmp/test_failures_nl_module.txt",
    );
}

#[test]
#[ignore]
fn dutch_style_examples_with_nl_pipeline() {
    evaluate_examples_file(
        &style_path(),
        "style.xml",
        "/tmp/test_failures_nl_style.txt",
    );
}

#[test]
#[ignore]
fn dutch_variant_grammar_examples_with_nl_pipeline() {
    evaluate_examples_file(
        &variant_grammar_path(),
        "nl-NL/grammar.xml",
        "/tmp/test_failures_nl_variant.txt",
    );
}

fn evaluate_examples_file(rules_path: &Path, file_label: &str, artifact: &str) {
    let xml = std::fs::read_to_string(rules_path).expect("rules xml");
    let raw_example_count = xml.matches("<example").count();
    let without_comments = strip_xml_comments(&xml);
    let comment_example_count = raw_example_count - without_comments.matches("<example").count();
    let rule_set = XmlCompiler::new()
        .compile_file_with_base(&xml, rules_path.parent())
        .expect("compile rules xml");

    let mut group_index: HashMap<(String, Option<String>), usize> = HashMap::new();
    let mut groups: Vec<Vec<&CompiledRule>> = Vec::new();
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

    let engine = suite_engine();
    let pipeline = DutchPipeline::shared();
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
                                    match_start, match_end, m_start, m_end,
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
                                matches.len(),
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
        "nl module pipeline [{file_label}]: {}/{} passed ({:.1}%) - no_match: {}, fp: {}, marker: {} [{:.1}s]",
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
        "accepted-excluded residue [{file_label}]: {raw_example_count} <example> tags ({comment_example_count} inside XML comments), {compiled_example_count} on compiled rules, {} on non-first rules of duplicate (id,sub_id) groups, {evaluated_example_count} evaluated",
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
    let _ = std::fs::write(artifact, fail_lines.join("\n") + "\n");
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

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::Rule as _;
    use std::sync::OnceLock;

    fn compiled_rules_from(path: PathBuf) -> &'static CompiledRuleSet {
        static GRAMMAR: OnceLock<CompiledRuleSet> = OnceLock::new();
        static STYLE: OnceLock<CompiledRuleSet> = OnceLock::new();
        static VARIANT: OnceLock<CompiledRuleSet> = OnceLock::new();
        let slot = if path.ends_with("style.xml") {
            &STYLE
        } else if path.ends_with("nl-NL/grammar.xml") {
            &VARIANT
        } else {
            &GRAMMAR
        };
        slot.get_or_init(|| {
            let xml = std::fs::read_to_string(&path).unwrap();
            XmlCompiler::new()
                .compile_file_with_base(&xml, path.parent())
                .expect("compile rules xml")
        })
    }

    fn rule_matches(rules_path: PathBuf, rule_id: &str, text: &str) -> Vec<(usize, usize)> {
        let rule = compiled_rules_from(rules_path)
            .rules
            .iter()
            .find(|r| r.id == rule_id)
            .unwrap_or_else(|| panic!("rule {rule_id} not compiled"));
        let engine = suite_engine();
        let pipeline = DutchPipeline::shared();
        let mut out = Vec::new();
        for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
            for m in engine.match_rule(rule, &sentence) {
                out.push((m.offset() + sentence_start, m.length()));
            }
        }
        out
    }

    #[test]
    fn dutchtest_demo_text_rules_fire_through_the_pipeline() {
        let text = "Bij voorbeeld om een collega te vertellen wat er vrijdag gebeurd is.";
        assert!(
            !rule_matches(grammar_path(), "BIJ_VOORBEELD", text).is_empty(),
            "the demo text must trigger BIJ_VOORBEELD"
        );
        let text = "Dit is fout !";
        assert!(
            !rule_matches(grammar_path(), "SPATIE_LEESTEKEN", text).is_empty(),
            "the pinned example must trigger SPATIE_LEESTEKEN on 'fout !'"
        );
        let text = "LanguageTool laat stijlwesties zo optimaal mogelijk zien in het blauw.";
        assert!(
            !rule_matches(style_path(), "ZO_OPTIMAAL_MOGELIJK", text).is_empty(),
            "the demo text must trigger ZO_OPTIMAAL_MOGELIJK"
        );

        let pipeline = DutchPipeline::shared();
        let uppercase = og_rules::native_rules::UppercaseSentenceStartRule::new();
        let demo = "wist u al dat u synoniemen kunt oproepen met een dubbelklik op een woord ?";
        let sentence = pipeline.analyze_range(demo, 0, demo.len());
        assert!(
            !uppercase.match_sentence(&sentence).is_empty(),
            "the demo text must trigger UPPERCASE_SENTENCE_START on 'wist'"
        );

        let simple_replace = crate::nl::simple_replace::dutch_simple_replace_rule();
        let text = "LanguageTool is een absolute musthave.";
        let sentence = pipeline.analyze_range(text, 0, text.len());
        let matches = simple_replace.match_sentence(&sentence);
        assert!(
            matches.len() == 1
                && matches[0]
                    .rule()
                    .id()
                    .contains("NL_SIMPLE_REPLACE_MUSTHAVE")
                && matches[0].replacements()[0].value() == "must-have",
            "{matches:?}"
        );
    }
}
