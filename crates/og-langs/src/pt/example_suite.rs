use crate::pt::filters::{apply_engine_pt_filter, is_pt_filter};
use crate::pt::test_support::PortuguesePipeline;
use og_core::{AnalyzedSentence, AnalyzedTokenReadings, RuleMatch};
use og_rules::pattern_rule::PatternRuleEngine;
use og_rules::postponed_adjective::LangSynthesizer;
use og_xml::compiler::{CompiledRule, CompiledRuleSet, XmlCompiler};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn rules_dir() -> PathBuf {
    crate::pt::filters::pt_rules_dir().expect("pt rules dir")
}

fn grammar_path() -> PathBuf {
    rules_dir().join("grammar.xml")
}

fn style_path() -> PathBuf {
    rules_dir().join("style.xml")
}

pub(crate) fn suite_engine() -> PatternRuleEngine {
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
    let resource_dir = crate::pt::tagger::pt_resource_dir()?;
    LangSynthesizer::for_language(&resource_dir, "pt")
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

fn token_matches(
    token: &og_xml::compiler::CompiledPatternToken,
    atr: &AnalyzedTokenReadings,
) -> bool {
    let surface = atr.token().token();
    let text_ok = match &token.text {
        Some(text) => {
            let matched = if let Some(re) = &token.compiled_regexp_fancy {
                re.is_match(surface).unwrap_or(false)
            } else if token.case_sensitive {
                token
                    .compiled_regexp
                    .as_ref()
                    .is_some_and(|re| re.is_match(surface))
                    || surface == text
            } else {
                token
                    .compiled_regexp_ci
                    .as_ref()
                    .or(token.compiled_regexp.as_ref())
                    .is_some_and(|re| re.is_match(surface))
                    || surface.eq_ignore_ascii_case(text)
            };
            if token.negate { !matched } else { matched }
        }
        None => true,
    };
    let postag_ok = match &token.postag {
        Some(postag) => {
            let matched = atr
                .readings()
                .iter()
                .flat_map(|r| r.pos_tags().iter())
                .any(|t| {
                    if let Some(re) = &token.compiled_postag_regexp_fancy {
                        re.is_match(t).unwrap_or(false)
                    } else if let Some(re) = &token.compiled_postag_regexp {
                        re.is_match(t)
                    } else {
                        t == postag
                    }
                });
            if token.negate_pos { !matched } else { matched }
        }
        None => true,
    };
    text_ok && postag_ok
}

fn element_satisfied(
    element: &og_xml::compiler::CompiledPatternElement,
    atr: &AnalyzedTokenReadings,
) -> bool {
    use og_xml::compiler::CompiledPatternElement;
    match element {
        CompiledPatternElement::Token(t) => token_matches(t, atr),
        CompiledPatternElement::OrGroup(alts) => alts.iter().any(|t| token_matches(t, atr)),
        CompiledPatternElement::AndGroup(all) => all.iter().all(|t| token_matches(t, atr)),
    }
}

fn element_optional(element: &og_xml::compiler::CompiledPatternElement) -> bool {
    use og_xml::compiler::CompiledPatternElement;
    match element {
        CompiledPatternElement::Token(t) => t.min == Some(0),
        CompiledPatternElement::OrGroup(alts) => alts.iter().any(|t| t.min == Some(0)),
        CompiledPatternElement::AndGroup(all) => all.iter().all(|t| t.min == Some(0)),
    }
}

pub(crate) fn matched_window(
    rule: &CompiledRule,
    tokens: &[&AnalyzedTokenReadings],
    rm: &RuleMatch,
) -> (Vec<usize>, Vec<String>) {
    let end = rm.offset() + rm.length();
    let overlap: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.token().start() < end && t.token().end() > rm.offset())
        .map(|(i, _)| i)
        .collect();
    let texts_of = |positions: &[usize]| {
        (
            positions.to_vec(),
            positions
                .iter()
                .map(|&i| tokens[i].token().token().to_string())
                .collect::<Vec<String>>(),
        )
    };
    let pattern = &rule.pattern;
    let (Some(ms), Some(me)) = (pattern.marker_elem_start, pattern.marker_elem_end) else {
        return texts_of(&overlap);
    };
    let marker_elements = me.saturating_sub(ms);
    let (Some(&first), Some(&last)) = (overlap.first(), overlap.last()) else {
        return texts_of(&overlap);
    };
    if marker_elements == 0 || overlap.len() != marker_elements {
        return texts_of(&overlap);
    }
    let elements = &pattern.elements;
    let mut start = first;
    let mut k = ms;
    while k > 0 {
        k -= 1;
        let Some(idx) = start.checked_sub(1).filter(|&i| i < tokens.len()) else {
            break;
        };
        if element_satisfied(&elements[k], tokens[idx]) || !element_optional(&elements[k]) {
            start = idx;
        }
    }
    let mut stop = last;
    let mut k = me;
    while k < elements.len() {
        let Some(idx) = stop.checked_add(1).filter(|&i| i < tokens.len()) else {
            break;
        };
        if element_satisfied(&elements[k], tokens[idx]) || !element_optional(&elements[k]) {
            stop = idx;
            k += 1;
        } else {
            break;
        }
    }
    let positions: Vec<usize> = (start..=stop).collect();
    let mut matched: Vec<usize> = Vec::with_capacity(positions.len());
    let mut texts: Vec<String> = Vec::with_capacity(elements.len());
    let mut cursor = 0usize;
    for (ei, element) in elements.iter().enumerate() {
        if ei == ms {
            cursor = first - start;
        } else if ei == me {
            cursor = last - start + 1;
        }
        let consumed = if ei >= ms && ei < me {
            positions.get(cursor).copied()
        } else {
            positions
                .get(cursor)
                .copied()
                .filter(|&p| element_satisfied(element, tokens[p]) || !element_optional(element))
        };
        match consumed {
            Some(p) => {
                matched.push(p);
                texts.push(tokens[p].token().token().to_string());
                cursor += 1;
            }
            None => texts.push(texts.last().cloned().unwrap_or_default()),
        }
    }
    if matched.is_empty() {
        return texts_of(&positions);
    }
    (matched, texts)
}

pub(crate) fn apply_suite_filter(
    rule: &CompiledRule,
    filter: &og_xml::types::XmlFilter,
    sentence: &AnalyzedSentence,
    tokens: &[&AnalyzedTokenReadings],
    synthesizer: Option<&LangSynthesizer>,
    m: RuleMatch,
) -> Option<RuleMatch> {
    if !is_pt_filter(&filter.class) {
        return Some(m);
    }
    let (matched_positions, matched_texts) = matched_window(rule, tokens, &m);
    apply_engine_pt_filter(
        rule,
        filter,
        &matched_texts,
        &matched_positions,
        tokens,
        sentence.text(),
        m,
        synthesizer,
        "pt-PT",
    )
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
    evaluate_examples_file(
        &grammar_path(),
        "grammar.xml",
        Some("/tmp/test_failures_pt_module.txt"),
    );
}

#[test]
#[ignore]
fn portuguese_style_examples_with_pt_pipeline() {
    evaluate_examples_file(
        &style_path(),
        "style.xml",
        Some("/tmp/test_failures_pt_style.txt"),
    );
}

fn evaluate_examples_file(rules_path: &Path, file_label: &str, artifact: Option<&str>) {
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
    let synthesizer = suite_synthesizer();
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
                let pt_filter = aux.filter.as_ref().filter(|f| is_pt_filter(&f.class));
                for (sentence, sentence_start) in pipeline.analyze_offset_text(&clean_text) {
                    let tokens: Vec<&AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
                    for m in engine.match_rule(aux, &sentence) {
                        let Some(mut m) = (match pt_filter {
                            Some(filter) => apply_suite_filter(
                                aux,
                                filter,
                                &sentence,
                                &tokens,
                                synthesizer.as_ref(),
                                m,
                            ),
                            None => Some(m),
                        }) else {
                            continue;
                        };
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
        "pt module pipeline [{file_label}]: {}/{} passed ({:.1}%) - no_match: {}, fp: {}, marker: {} [{:.1}s]",
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
    if file_label == "grammar.xml" {
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
    }
    if let Some(artifact) = artifact {
        let _ = std::fs::write(artifact, fail_lines.join("\n") + "\n");
    }
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
    use std::sync::OnceLock;

    fn compiled_rules_from(path: PathBuf) -> &'static CompiledRuleSet {
        static GRAMMAR: OnceLock<CompiledRuleSet> = OnceLock::new();
        static STYLE: OnceLock<CompiledRuleSet> = OnceLock::new();
        let slot = if path.ends_with("style.xml") {
            &STYLE
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

    fn filtered_rule_matches(
        rules_path: PathBuf,
        rule_id: &str,
        text: &str,
    ) -> Vec<(usize, usize, Vec<String>)> {
        let rule = compiled_rules_from(rules_path)
            .rules
            .iter()
            .find(|r| r.id == rule_id)
            .unwrap_or_else(|| panic!("rule {rule_id} not compiled"));
        let engine = suite_engine();
        let synthesizer = suite_synthesizer();
        let pipeline = PortuguesePipeline::shared();
        let mut out = Vec::new();
        for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
            let tokens: Vec<&AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
            for m in engine.match_rule(rule, &sentence) {
                let Some(m) = (match rule.filter.as_ref() {
                    Some(filter) => apply_suite_filter(
                        rule,
                        filter,
                        &sentence,
                        &tokens,
                        synthesizer.as_ref(),
                        m,
                    ),
                    None => Some(m),
                }) else {
                    continue;
                };
                out.push((
                    m.offset() + sentence_start,
                    m.length(),
                    m.replacements()
                        .iter()
                        .map(|r| r.value().to_string())
                        .collect(),
                ));
            }
        }
        out
    }

    fn pt_filter_rule_matches(
        rules_path: PathBuf,
        rule_id: &str,
        text: &str,
    ) -> (usize, Vec<(usize, usize, Vec<String>)>) {
        let engine = suite_engine();
        let synthesizer = suite_synthesizer();
        let pipeline = PortuguesePipeline::shared();
        let mut raw = 0usize;
        let mut filtered = Vec::new();
        for rule in compiled_rules_from(rules_path).rules.iter() {
            if rule.id != rule_id {
                continue;
            }
            let Some(filter) = rule.filter.as_ref().filter(|f| is_pt_filter(&f.class)) else {
                continue;
            };
            let bare = {
                let mut b = rule.clone();
                b.filter = None;
                b
            };
            for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
                raw += engine.match_rule(&bare, &sentence).len();
                let tokens: Vec<&AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
                for m in engine.match_rule(rule, &sentence) {
                    let Some(m) = apply_suite_filter(
                        rule,
                        filter,
                        &sentence,
                        &tokens,
                        synthesizer.as_ref(),
                        m,
                    ) else {
                        continue;
                    };
                    filtered.push((
                        m.offset() + sentence_start,
                        m.length(),
                        m.replacements()
                            .iter()
                            .map(|r| r.value().to_string())
                            .collect(),
                    ));
                }
            }
        }
        (raw, filtered)
    }

    #[test]
    fn suite_filter_drives_the_style_synthesizer_rule_end_to_end() {
        let matches = filtered_rule_matches(
            style_path(),
            "TER_PARTICIPIO-PASSADO_V2",
            "Eu tenho pesquisado o assunto há décadas.",
        );
        assert!(
            matches
                .iter()
                .any(|(_, _, reps)| reps.iter().any(|r| r == "pesquiso")),
            "lemmaFrom:2 (pesquisado→pesquisar) with postagFrom:1 (tenho→VMIP1S0) must synthesize 'pesquiso': {matches:?}"
        );
    }

    #[test]
    fn suite_filter_pairs_regexp_groups_for_the_toponym_rule() {
        let matches = filtered_rule_matches(
            grammar_path(),
            "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PARENTESES",
            "Nasceu em Niterói (RJ).",
        );
        assert_eq!(
            matches,
            vec![(18, 5, vec!["\u{2013}RJ".to_string()])],
            "the marked group 2 covers the optional space plus '(RJ)'"
        );
    }

    #[test]
    fn suite_filter_nulls_the_ten_diacritics_runner_false_positives() {
        for text in [
            "Alguém fabrica aves que voam por entre as labaredas de azul.",
            "Ninguém nos deu uma oportunidade.",
            "Ninguém sabia disso a não ser o Tom.",
            "Por isso a alma dele sofre.",
            "Qualquer um seria melhor do que ninguém.",
            "Enfim, na iminência da derrota total, teve ainda o desplante de declarar que nada ficaria na mesma depois do Apito, pois tinha conseguido incomodar e assustar... os inocentes.",
            "Mas pq diabos uma figura dessas seria agora plasmada à imagem de um (ex)juiz em manifestações de rua?",
            "Ninguém sabia nada sobre isso.",
            "Uma pedra nos servia de assento.",
            "Você nos incomoda com sua música.",
        ] {
            let (raw, filtered) = pt_filter_rule_matches(grammar_path(), "DIACRITICS", text);
            assert!(raw > 0, "the engine must fire raw on '{text}'");
            assert!(
                filtered.is_empty(),
                "ConfusionCheckFilter must null the correct example '{text}': {filtered:?}"
            );
        }
    }

    #[test]
    fn suite_filter_nulls_the_eight_separadores_runner_false_positives() {
        let cases = [
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PARENTESES",
                "O candidato a prefeito Fernando Gabeira (RJ).",
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Adaptador 135W AC com cabo de 3 pinos.",
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Autônomo de Saneamento de Pelotas -PROCAP",
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Cidade Alerta GO.",
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Nasceu em Niterói–RJ.",
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Nativo de Ooga Booga do Foo Bar/RJ.",
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Nativo de Oogabooga/RJ.",
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Foram 157 km2 de mata derrubados em nove Estados (AC, AM, AP, MA, MT, PA, TO, RO e RR).",
            ),
        ];
        for (id, text) in cases {
            let (raw, filtered) = pt_filter_rule_matches(grammar_path(), id, text);
            assert!(raw > 0, "the engine must fire raw on '{text}' ({id})");
            assert!(
                filtered.is_empty(),
                "BrazilianToponymFilter must null the correct example '{text}': {filtered:?}"
            );
        }
    }

    #[test]
    fn suite_filter_nulls_the_two_irregular_participle_runner_false_positives() {
        for text in [
            "Depois de ter discretamente limpado o carro, foi desculpado.",
            "No totalitarismo, as liberdades do indivíduo eram suprimidas e o povo era subordinado ao poder sem limites do …",
        ] {
            let (raw, filtered) =
                pt_filter_rule_matches(grammar_path(), "IRREGULAR_PAST_PARTICIPLES", text);
            assert!(raw > 0, "the engine must fire raw on '{text}'");
            assert!(
                filtered.is_empty(),
                "RegularIrregularParticipleFilter must null the correct example '{text}': {filtered:?}"
            );
        }
    }
}
