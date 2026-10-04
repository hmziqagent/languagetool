use crate::pl::test_support::PolishPipeline;
use og_rules::pattern_rule::PatternRuleEngine;
use og_xml::compiler::{CompiledRule, CompiledRuleSet, XmlCompiler};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn rules_dir() -> PathBuf {
    crate::pl::pl_rules_dir().expect("pl rules dir")
}

fn grammar_path() -> PathBuf {
    rules_dir().join("grammar.xml")
}

pub(crate) fn suite_engine() -> PatternRuleEngine {
    let resource_dir = crate::pl::tagger::pl_resource_dir().expect("pl resource dir");
    let core_resource_dir = crate::engine::find_lt_core_resource_dir().expect("core resource dir");
    let dictionary = og_spell::Dictionary::from_shared_words(std::sync::Arc::new(
        og_spell::morfologik_view::MorfologikView::new(
            og_tagger::generic_tagger::shared_dictionary(&resource_dir, "polish")
                .expect("polish tagger dict"),
        ),
    ));
    let support = og_rules::spelling_support::SpellingSupport::new_for_language(
        std::sync::Arc::new(dictionary),
        &core_resource_dir,
        &resource_dir,
        "pl",
    );
    PatternRuleEngine::new().with_spelling_support(std::sync::Arc::new(support))
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
fn polish_grammar_examples_with_pl_pipeline() {
    evaluate_examples_file(
        &grammar_path(),
        "grammar.xml",
        "/tmp/test_failures_pl_module.txt",
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
    let pipeline = PolishPipeline::shared();
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
        "pl module pipeline [{file_label}]: {}/{} passed ({:.1}%) - no_match: {}, fp: {}, marker: {} [{:.1}s]",
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

    fn compiled_rules_from(path: PathBuf) -> &'static CompiledRuleSet {
        static GRAMMAR: OnceLock<CompiledRuleSet> = OnceLock::new();
        let slot = if path.ends_with("grammar.xml") {
            &GRAMMAR
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
        let pipeline = PolishPipeline::shared();
        let mut out = Vec::new();
        for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
            for m in engine.match_rule(rule, &sentence) {
                out.push((m.offset() + sentence_start, m.length()));
            }
        }
        out
    }

    #[test]
    fn suite_engine_resolves_the_polish_spelling_support() {
        let engine = suite_engine();
        let _ = engine;
    }

    fn rule_matches_of(rules_path: PathBuf, rule_id: &str, text: &str) -> Vec<(usize, usize)> {
        rule_matches(rules_path, rule_id, text)
    }

    #[test]
    fn polishtest_comma_rules_fire_through_the_pl_disambiguation() {
        let text = "Kupi\u{142}em wino chcia\u{142}em te\u{17c} pi\u{107} wod\u{119}.";
        assert!(
            !rule_matches_of(grammar_path(), "ZDANIA_ZLOZONE", text).is_empty(),
            "the missing-comma example must fire once interp:comma is comma-only: {text}"
        );
        let text = "Kupi\u{142}em wino, chcia\u{142}em te\u{17c} pi\u{107} wod\u{119}.";
        assert!(
            rule_matches_of(grammar_path(), "ZDANIA_ZLOZONE", text).is_empty(),
            "the correct comma form stays clean: {text}"
        );
    }

    #[test]
    fn polishtest_agreement_rules_fire_through_the_pipeline() {
        let text = "Odr\u{f3}\u{17c}ni\u{142} je od poj\u{119}cia solipsyzm metodologicznego.";
        assert!(
            !rule_matches(grammar_path(), "SUBST_ADJ_UNIFY", text).is_empty(),
            "the agreement pattern fires on the corpus example: {text}"
        );
    }

    #[test]
    fn polishtest_correct_agreement_example_stays_clean() {
        let text = "Lubi\u{119} pi\u{142}k\u{119} no\u{17c}n\u{105}.";
        assert!(
            rule_matches(grammar_path(), "ADJ_SUBST_ADJ_UNIFY", text).is_empty(),
            "the agreed example stays clean: {text}"
        );
    }
}
