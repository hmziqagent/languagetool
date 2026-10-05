use crate::ca::example_suite::CatalanPipeline;
use og_rules::pattern_rule::PatternRuleEngine;
use og_xml::compiler::{
    CompiledPattern, CompiledPatternElement, CompiledRule, CompiledRuleSet, XmlCompiler,
};

pub(crate) fn compiled_rules() -> &'static CompiledRuleSet {
    static GRAMMAR: std::sync::OnceLock<CompiledRuleSet> = std::sync::OnceLock::new();
    GRAMMAR.get_or_init(|| {
        let path = crate::ca::example_suite::ca_rules_dir()
            .expect("ca rules dir")
            .join("grammar.xml");
        let xml = std::fs::read_to_string(&path).expect("ca grammar.xml");
        XmlCompiler::new()
            .compile_file_with_base(&xml, path.parent())
            .expect("compile ca grammar.xml")
    })
}

pub(crate) fn suite_engine() -> PatternRuleEngine {
    PatternRuleEngine::new()
}

fn group_of(rule_id: &str) -> Vec<&'static CompiledRule> {
    compiled_rules()
        .rules
        .iter()
        .filter(|r| r.id == rule_id)
        .collect()
}

pub(crate) fn probe(rule_id: &str, text: &str) {
    let pipeline = CatalanPipeline::shared();
    let engine = suite_engine();
    eprintln!("=== probe [{rule_id}] '{text}' ===");
    for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
        eprintln!(
            "-- sentence [{sentence_start}..{}] '{:?}'",
            sentence_start + sentence.text().len(),
            sentence.text()
        );
        for t in sentence.non_whitespace_tokens() {
            let readings: Vec<String> = t
                .readings()
                .iter()
                .map(|r| format!("{}[{}]", r.lemma().unwrap_or("-"), r.pos_tags().join("+")))
                .collect();
            eprintln!(
                "   {}@{}..{} {:?} => {}",
                t.token().start() + sentence_start,
                t.token().start(),
                t.token().end(),
                t.token().token(),
                readings.join(", ")
            );
        }
        for rule in group_of(rule_id) {
            let matches = engine.match_rule(rule, &sentence);
            eprintln!(
                "   raw matches for {} (sub {:?}): {}",
                rule.id,
                rule.sub_id.as_deref(),
                matches.len()
            );
            for m in &matches {
                eprintln!(
                    "      off {} len {} msg {} replacements {:?}",
                    m.offset() + sentence_start,
                    m.length(),
                    m.message(),
                    m.replacements()
                        .iter()
                        .map(|r| r.value())
                        .collect::<Vec<_>>()
                );
            }
        }
    }
}

pub(crate) fn probe_with_engine_trace(rule_id: &str, text: &str) {
    probe_with_engine_trace_sub(rule_id, None, text);
}

pub(crate) fn probe_with_engine_trace_sub(rule_id: &str, sub_id: Option<&str>, text: &str) {
    let pipeline = CatalanPipeline::shared();
    let engine = suite_engine();
    eprintln!("=== engine trace [{rule_id}::{sub_id:?}] '{text}' ===");
    for rule in group_of(rule_id)
        .iter()
        .filter(|r| sub_id.is_none_or(|s| r.sub_id.as_deref() == Some(s)))
    {
        for (sentence, _) in pipeline.analyze_offset_text(text) {
            og_rules::pattern_trace::set_enabled(true);
            og_rules::pattern_trace::clear();
            let _ = engine.match_rule(rule, &sentence);
            for ev in og_rules::pattern_trace::take() {
                eprintln!(
                    "   [elem {} token {} {:?}] {}",
                    ev.pattern_idx, ev.token_idx, ev.token, ev.cause
                );
            }
            og_rules::pattern_trace::set_enabled(false);
        }
    }
}

pub(crate) fn pattern_of(rule_id: &str) {
    for rule in compiled_rules().rules.iter().filter(|r| r.id == rule_id) {
        eprintln!(
            "== rule {} (sub {:?}) pattern tokens: {} antipatterns: {}",
            rule.id,
            rule.sub_id.as_deref(),
            rule.pattern.elements.len(),
            rule.antipatterns.len()
        );
        for (i, el) in rule.pattern.tokens.iter().enumerate() {
            eprintln!(
                "   [{i}] text={:?} postag={:?} inflected={} regexp={} skip={} negate={} min={:?} max={:?} exceptions={}",
                el.text.as_deref(),
                el.postag.as_deref(),
                el.inflected,
                el.is_regexp,
                el.skip,
                el.negate,
                el.min,
                el.max,
                el.exceptions.len()
            );
        }
        for (a, ap) in rule.antipatterns.iter().enumerate() {
            for (i, el) in ap.tokens.iter().enumerate() {
                eprintln!(
                    "   anti[{a}][{i}] text={:?} re={:?} postag={:?} inflected={} cs={} skip={} min={:?} max={:?} exceptions={}",
                    el.text.as_deref(),
                    el.compiled_regexp.as_ref().map(|r| r.as_str()),
                    el.postag.as_deref(),
                    el.inflected,
                    el.case_sensitive,
                    el.skip,
                    el.min,
                    el.max,
                    el.exceptions.len()
                );
            }
        }
    }
}

fn token_needs_readings(t: &og_xml::compiler::CompiledPatternToken) -> bool {
    t.postag.is_some() || t.compiled_postag_regexp.is_some() || t.inflected
}

fn element_needs_readings(e: &CompiledPatternElement) -> bool {
    match e {
        CompiledPatternElement::Token(t) => token_needs_readings(t),
        CompiledPatternElement::OrGroup(g) => g.iter().any(token_needs_readings),
        CompiledPatternElement::AndGroup(g) => g.iter().any(token_needs_readings),
    }
}

pub(crate) fn pattern_needs_readings(pattern: &CompiledPattern) -> bool {
    pattern.tokens.iter().any(token_needs_readings)
        || pattern.elements.iter().any(element_needs_readings)
        || !pattern.unify_groups.is_empty()
}

pub(crate) fn rule_needs_readings(rule: &CompiledRule) -> bool {
    pattern_needs_readings(&rule.pattern)
}

fn count_matches(rule_id: &str, text: &str) -> usize {
    count_matches_sub(rule_id, None, text)
}

fn count_matches_sub(rule_id: &str, sub_id: Option<&str>, text: &str) -> usize {
    let pipeline = CatalanPipeline::shared();
    let engine = suite_engine();
    let mut matches = 0usize;
    for rule in group_of(rule_id)
        .iter()
        .filter(|r| sub_id.is_none_or(|s| r.sub_id.as_deref() == Some(s)))
    {
        for (sentence, _) in pipeline.analyze_offset_text(text) {
            matches += engine.match_rule(rule, &sentence).len();
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mes1_antipattern_immunizes_its_own_example() {
        assert_eq!(
            count_matches("MES1", "Destaca juliol com a mes sec."),
            0,
            "positive control: the shared engine's antipattern machinery handles MES1's \
             antipattern (month-name regexp with skip=3 plus exception scope=next \
             inflected=ser, then 'mes', then the weather-adjective regexp, grammar.xml:\
             72590-72596) - antipattern_immunize_spans over match_pattern_tokens honors \
             skip (pattern_rule.rs:1583-1616) and scope=next exceptions (:1610-1629), and \
             the &mesos_any; entity expands via XmlRuleParser::expand_entities_with_base \
             (og-xml parser.rs:1306) - if this pin flips the engine regressed"
        );
    }

    #[test]
    fn mes1_residue_pin_still_fires_the_plain_correct_example() {
        assert_eq!(
            count_matches("MES1", "El mes que ve."),
            1,
            "the MES1 fp family (76 fails, the largest in the surface bucket): Java \
             suppresses this match only through <filter class='org.languagetool.rules.\
             SuppressIfAnyRuleMatchesFilter' args='ruleIDs:MES2'>, which re-analyzes the \
             sentence with each suggestion applied and drops the match when MES2 fires at \
             an overlapping position (SuppressIfAnyRuleMatchesFilter.java:43-58) - on \
             'El m\u{e9}s que ve.' MES2's DET/m\u{e9}s/de/month sub-rules fire and veto. \
             Our engine's filter dispatch (og-rules pattern_rule.rs:2675 apply_filter) has \
             no arm for this class, and every MES2 sub-rule pattern is postag-based, so \
             the veto additionally needs the missing ca-ES.dict - this pin flips to 0 only \
             when BOTH the engine arm and the dictionary exist"
        );
    }

    #[test]
    fn inflected_surface_rules_stay_lemma_starved() {
        assert_eq!(
            count_matches("ENRECORDAR", "No t'enrecordes?"),
            0,
            "ENRECORDAR (21 fails): <token inflected='yes'>enrecordar</token> matches \
             lemma-or-surface (PatternToken.java:364-374 getTestToken); with the dict \
             'enrecordes' carries lemma 'enrecordar' and matches, on our added.txt-only \
             tagger the token has no readings and surface 'enrecordes' != 'enrecordar' - \
             the accepted-excluded dict family, not an engine gap (our fallback at \
             pattern_rule.rs:2245-2255 mirrors getTestToken)"
        );
        assert_eq!(
            count_matches("LHI_SEL", "L'hi denominava aix\u{ed}."),
            0,
            "LHI_SEL (10 fails): the third pattern token <token inflected='yes'>\
             denominar</token> needs the lemma reading of 'denominava' (PatternToken.\
             java:364-374); the surface tokens l'|li and hi|'l match fine, only the \
             inflected verb is dict-starved"
        );
    }

    #[test]
    fn mesos_minuscula_sub1_suppressed_by_a_dict_dependent_antipattern() {
        assert_eq!(
            count_matches_sub("MESOS_MINUSCULA", Some("1"), "El 3 de Juliol de 1985."),
            0,
            "MESOS_MINUSCULA sub 1 (9 fails, no_match side): the pattern token \
             (?-i)&mesos_any_maj; matches 'Juliol' (verified by probe_round20_inline_\
             case_flag_regex) and the match is dropped by the case-sensitive antipattern \
             at grammar.xml:85338-85343: \\\\p{{Lu}}.* + [min=0 max=4: \\\\p{{Lu}}.*|de|del|\
             d'|l'|\\\\d+] + month, which matches 'El' + ['3','de'] + 'Juliol' and \
             immunizes the span; in Java 'El' carries a D.* reading that the antipattern's \
             <exception postag='SPS00|D.*|C.*'> vetoes, so the antipattern cannot start - \
             dict-starved, the untagged 'El' cannot exercise the veto"
        );
        assert_eq!(
            count_matches_sub("MESOS_MINUSCULA", Some("1"), "el 3 de Juliol de 1985."),
            1,
            "lowercase control: 'el' fails the case-sensitive \\\\p{{Lu}}.* first token, so \
             the antipattern cannot fire and the same pattern matches - proving the \
             engine's antipattern, min/max and case-sensitivity handling are correct and \
             the suppression is purely the missing D.* reading"
        );
    }

    #[test]
    fn aleshores_residue_pin_still_fires_the_correct_example() {
        assert_eq!(
            count_matches("ALESHORES", "A les hores convingudes"),
            1,
            "the fp twin of the MESOS family: Java suppresses this correct example \
             through ALESHORES' antipattern hores + <token postag='A..[FC][PN].|V.P..PF.'\
             postag_regexp='yes'/>, which needs the adjective/participle reading of \
             'convingudes' (grammar.xml:119849-119853); dict-starved the antipattern \
             cannot fire and the surface pattern a+les+hores matches - flips to 0 when \
             ca-ES.dict lands"
        );
    }

    #[test]
    fn classifier_counts_inflected_tokens_as_dict_dependent() {
        let surface = compiled_rules()
            .rules
            .iter()
            .find(|r| r.id == "MES1")
            .expect("MES1 compiled");
        assert!(!rule_needs_readings(surface));
        let enrecordar = compiled_rules()
            .rules
            .iter()
            .find(|r| r.id == "ENRECORDAR")
            .expect("ENRECORDAR compiled");
        assert!(
            enrecordar.pattern.tokens.iter().any(|t| t.inflected),
            "ENRECORDAR must carry inflected tokens"
        );
        assert!(rule_needs_readings(enrecordar));
    }

    #[ignore]
    #[test]
    fn probe_round20_engine_side_breakdown() {
        let Ok(lines) = std::fs::read_to_string("/tmp/test_failures_ca_module.txt") else {
            eprintln!("no /tmp/test_failures_ca_module.txt - run the module suite first");
            return;
        };
        let mut fails: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for line in lines.lines() {
            if let Some(rest) = line.strip_prefix("FAIL: [") {
                if let Some((id, _)) = rest.split_once(']') {
                    *fails.entry(id.to_string()).or_default() += 1;
                }
            }
        }
        let mut engine_side: Vec<(String, usize)> = Vec::new();
        let mut dict_side = 0usize;
        for (id, n) in &fails {
            let mut sub_rules = 0usize;
            let mut surface_subs = 0usize;
            for r in compiled_rules().rules.iter().filter(|r| &r.id == id) {
                sub_rules += 1;
                surface_subs += usize::from(!rule_needs_readings(r));
            }
            if sub_rules > 0 && surface_subs == sub_rules {
                engine_side.push((id.clone(), *n));
            } else {
                dict_side += n;
            }
        }
        engine_side.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let total: usize = fails.values().sum();
        let engine_total: usize = engine_side.iter().map(|(_, n)| n).sum();
        eprintln!(
            "corrected attribution: {total} failures -> {dict_side} on dict-dependent rules \
             (postag OR inflected in every sub-rule, per PatternToken.java:364-374 and \
             AbstractPatternRulePerformer.java:204-218), {engine_total} on fully surface rules"
        );
        eprintln!(
            "surface residue by rule: {}",
            engine_side
                .iter()
                .take(40)
                .map(|(id, n)| format!("{id}({n})"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }

    #[ignore]
    #[test]
    fn probe_round20_mes1_antipattern_trace() {
        pattern_of("MES1");
        probe("MES1", "Destaca juliol com a mes sec.");
        probe_with_engine_trace("MES1", "Destaca juliol com a mes sec.");
        probe("MES1", "El mes que ve.");
        probe(
            "MES1",
            "Per mes detalls, vegeu la nostra pol\u{ed}tica de privacitat.",
        );
    }

    #[ignore]
    #[test]
    fn probe_round20_enrecordar_readings() {
        pattern_of("ENRECORDAR");
        pattern_of("LHI_SEL");
        probe("ENRECORDAR", "No t'enrecordes?");
        probe("LHI_SEL", "L'hi denominava aix\u{ed}.");
    }

    #[ignore]
    #[test]
    fn probe_round20_inline_case_flag_regex() {
        let month_re = "^(?:(?-i)Gener|Febrer|Mar\u{e7}|Abril|Maig|Juny|Juliol|Agost|Setembre|Octubre|Novembre|Desembre)$";
        match regex::RegexBuilder::new(month_re)
            .case_insensitive(true)
            .build()
        {
            Ok(re) => eprintln!(
                "plain ci build: OK is_match(Juliol)={} is_match(juliol)={} is_match(GENER)={}",
                re.is_match("Juliol"),
                re.is_match("juliol"),
                re.is_match("GENER")
            ),
            Err(e) => eprintln!("plain ci build: FAIL {e}"),
        }
        match og_xml::java_case_fold::compile_java_ci(month_re) {
            Some(re) => eprintln!(
                "java_case_fold: OK is_match(Juliol)={} is_match(juliol)={}",
                re.is_match("Juliol"),
                re.is_match("juliol")
            ),
            None => eprintln!("java_case_fold: None"),
        }
        let compiled = compiled_rules()
            .rules
            .iter()
            .find(|r| r.id == "MESOS_MINUSCULA" && r.sub_id.as_deref() == Some("1"))
            .unwrap();
        let tok = &compiled.pattern.tokens[0];
        eprintln!(
            "MESOS_MINUSCULA.1 token0: cs_re={} ci_re={} fancy={}",
            tok.compiled_regexp.is_some(),
            tok.compiled_regexp_ci.is_some(),
            tok.compiled_regexp_fancy.is_some()
        );
        if let Some(re) = &tok.compiled_regexp_ci {
            eprintln!(
                "ci is_match(Juliol)={} is_match(juliol)={}",
                re.is_match("Juliol"),
                re.is_match("juliol")
            );
        }
    }

    #[ignore]
    #[test]
    fn probe_round20_mesos_minuscula_suppression_bisect() {
        let pipeline = CatalanPipeline::shared();
        let engine = suite_engine();
        for text in [
            "El 3 de Juliol de 1985.",
            "el 3 de Juliol de 1985.",
            "Vaig al Juliol.",
            "3 de Juliol",
            "X de Juliol",
            "X 3 de Juliol",
            "el tres de Juliol",
        ] {
            let per_sub: Vec<String> = group_of("MESOS_MINUSCULA")
                .iter()
                .map(|rule| {
                    let mut n = 0usize;
                    for (sentence, _) in pipeline.analyze_offset_text(text) {
                        n += engine.match_rule(rule, &sentence).len();
                    }
                    format!("{}={}", rule.sub_id.clone().unwrap_or_default(), n)
                })
                .collect();
            eprintln!("MESOS_MINUSCULA on '{text}': {}", per_sub.join(" "));
        }
        probe_with_engine_trace_sub("MESOS_MINUSCULA", Some("1"), "El 3 de Juliol de 1985.");
    }
}
