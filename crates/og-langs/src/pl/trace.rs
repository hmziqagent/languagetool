use crate::pl::test_support::PolishPipeline;
use og_rules::pattern_rule::PatternRuleEngine;
use og_xml::compiler::{CompiledRule, CompiledRuleSet, XmlCompiler};

pub(crate) fn trace_engine() -> PatternRuleEngine {
    crate::pl::example_suite::suite_engine()
}

pub(crate) fn compiled_rules() -> &'static CompiledRuleSet {
    static GRAMMAR: std::sync::OnceLock<CompiledRuleSet> = std::sync::OnceLock::new();
    GRAMMAR.get_or_init(|| {
        let path = crate::pl::pl_rules_dir()
            .expect("pl rules dir")
            .join("grammar.xml");
        let xml = std::fs::read_to_string(&path).expect("grammar.xml");
        XmlCompiler::new()
            .compile_file_with_base(&xml, path.parent())
            .expect("compile pl grammar.xml")
    })
}

fn group_of(rule_id: &str) -> Vec<&'static CompiledRule> {
    compiled_rules()
        .rules
        .iter()
        .filter(|r| r.id == rule_id)
        .collect()
}

pub(crate) fn probe_with_engine_trace(rule_id: &str, text: &str) {
    let pipeline = PolishPipeline::shared();
    let engine = trace_engine();
    eprintln!("=== engine trace [{rule_id}] '{text}' ===");
    for rule in group_of(rule_id) {
        for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
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
            let _ = sentence_start;
        }
    }
}

pub(crate) fn probe(rule_id: &str, text: &str) {
    let pipeline = PolishPipeline::shared();
    let engine = trace_engine();
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
                    "      off {} len {} msg {}",
                    m.offset() + sentence_start,
                    m.length(),
                    m.message()
                );
            }
        }
    }
}

pub(crate) fn pattern_of(rule_id: &str) {
    for rule in compiled_rules().rules.iter().filter(|r| r.id == rule_id) {
        eprintln!(
            "== rule {} (sub {:?}) pattern tokens: {}",
            rule.id,
            rule.sub_id.as_deref(),
            rule.pattern.elements.len()
        );
        for (i, el) in rule.pattern.tokens.iter().enumerate() {
            eprintln!(
                "   [{i}] text={:?} postag={:?} regexp={} skip={} negate={} min={:?} max={:?} space={:?}",
                el.text.as_deref(),
                el.postag.as_deref(),
                el.is_regexp,
                el.skip,
                el.negate,
                el.min,
                el.max,
                el.space_before.as_deref()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Residue map for the 45 remaining grammar.xml failures (5,206/5,251 = 99.1%):
    //
    // ZDANIA_ZLOZONE(12)  comma-between-clauses residue; the 8 sub-rules need the
    //                      scope="next" gap semantics of Java's AbstractPatternRulePerformer
    //                      (checked per READING of the candidate token, not across the
    //                      whole bridged range as the Rust engine does).
    // DNI_TYGODNIA(4)      weekday spelling; needs the &weekdays; entity + capitalization
    //                      matcher nuance (java_case_fold on Polish ł/Ł).
    // BRAK_KROPKI(3)       sentence-final period; SENT_END-tag reading interaction.
    // ZAGNIEZDZONE_CUDZYSLOWY(1)
    //                      shared engine normalizes U+2018/U+2019 to "'" on the surface
    //                      side of regexp elements only (og-xml/src/unifier.rs:8), so the
    //                      pattern [‘„] can never match the ‘ token; Java matches the
    //                      surface raw (PatternToken.getTestToken).
    // ADJ_SUBST_ADJ_UNIFY(1)
    //                      unify (number/gender/case) evaluation order for the masc/fem
    //                      adjective disagreement chain.
    // singles              proper-noun agreement, prefix words, letter-case rules.

    #[ignore]
    #[test]
    fn probe_nested_quotes_both_examples() {
        probe(
            "ZAGNIEZDZONE_CUDZYSLOWY",
            "To jest jak wiadomo \u{201e}cytat \u{201e}w cytacie\u{201d}, kt\u{f3}ry jest d\u{142}ugi\u{201d}.",
        );
        probe(
            "ZAGNIEZDZONE_CUDZYSLOWY",
            "\u{201e}Ka\u{17c}dy noemat ma \u{2018}tre\u{15b}\u{107}\u{2019}\u{201d}.",
        );
    }

    #[ignore]
    #[test]
    fn probe_interp_entity_compilation() {
        let interp = "[\\p{Punct}\\-\\u{2013}\\u{2212}\\u{2014}\\u{2015}\\u{201e}\\u{00ab}\\u{00bb}\\u{2026}\\u{2019}&&[^\\)]]+";
        eprintln!("interp entity: {interp}");
        match regex::Regex::new(interp) {
            Ok(re) => eprintln!(
                "regex crate: OK; is_match(.)={} is_match(\u{201e})={} is_match(\u{2026})={} is_match(w)={}",
                re.is_match("."),
                re.is_match("\u{201e}"),
                re.is_match("\u{2026}"),
                re.is_match("w")
            ),
            Err(e) => eprintln!("regex crate: FAIL {e}"),
        }
        for rule in compiled_rules()
            .rules
            .iter()
            .filter(|r| r.id == "ZDANIA_ZLOZONE")
            .take(2)
        {
            for (i, tok) in rule.pattern.tokens.iter().enumerate() {
                eprintln!(
                    "ZDANIA_ZLOZE elem {i}: text={:?} postag={:?} re={:?} fancy={:?}",
                    tok.text.as_deref(),
                    tok.postag.as_deref(),
                    tok.compiled_regexp.as_ref().map(|r| r.as_str()),
                    tok.compiled_regexp_fancy.as_ref().map(|r| r.as_str()),
                );
            }
        }
    }

    #[ignore]
    #[test]
    fn probe_interp_comma_rule_firing() {
        use og_core::checker::Disambiguator as _;
        let resource_dir = crate::pl::tagger::pl_resource_dir().expect("pl resource dir");
        let text = "To jest co\u{15b}, co wiedzia\u{142}em od dawna.";
        let mut xml_stage = crate::pl::test_support::analyze_undisambiguated(text, 0, text.len());
        if let Some(xml) = std::fs::read_to_string(resource_dir.join("disambiguation.xml"))
            .ok()
            .and_then(|x| {
                let normalized = crate::pl::normalize_disambiguation_xml(&x);
                og_tagger::XmlDisambiguator::from_xml_with_english(&normalized, None).ok()
            })
        {
            xml.disambiguate(&mut xml_stage);
        }
        for t in xml_stage.non_whitespace_tokens() {
            let readings: Vec<String> = t
                .readings()
                .iter()
                .map(|r| format!("{}[{}]", r.lemma().unwrap_or("-"), r.pos_tags().join("+")))
                .collect();
            eprintln!("   {:?} => {}", t.token().token(), readings.join(", "));
        }
    }

    #[ignore]
    #[test]
    fn probe_punct_rule_parsing() {
        let xml = std::fs::read_to_string(
            crate::pl::tagger::pl_resource_dir()
                .expect("pl resource dir")
                .join("disambiguation.xml"),
        )
        .unwrap();
        let parsed = og_tagger::XmlDisambiguator::from_xml_with_english(&xml, None)
            .expect("parse disambiguation.xml");
        eprintln!("disambiguator rule count: {}", parsed.rule_count());
        if let Some(idx) = parsed
            .rule_ids()
            .iter()
            .position(|id| *id == "PUNCT_NO_DOT")
        {
            eprintln!("PUNCT_NO_DOT idx {idx}");
            eprintln!("{}", parsed.rule_debug(idx));
            eprintln!("{}", parsed.rule_pattern_debug(idx));
            eprintln!("{}", parsed.rule_debug(idx));
            eprintln!("{}", parsed.rule_pattern_debug(idx));
        }
        let text = "To jest co\u{15b}, co wiedzia\u{142}em.";
        let mut sentence = crate::pl::test_support::analyze_undisambiguated(text, 0, text.len());
        if let Some(idx) = parsed
            .rule_ids()
            .iter()
            .position(|id| *id == "PUNCT_NO_DOT")
        {
            parsed.apply_rule(&mut sentence, idx);
            for t in sentence.non_whitespace_tokens() {
                let readings: Vec<String> = t
                    .readings()
                    .iter()
                    .map(|r| format!("{}[{}]", r.lemma().unwrap_or("-"), r.pos_tags().join("+")))
                    .collect();
                eprintln!(
                    "after-rule {:?} => {}",
                    t.token().token(),
                    readings.join(", ")
                );
            }
        }
    }

    #[ignore]
    #[test]
    fn probe_parser_exception_text_shapes() {
        let mk = |token_body: &str| {
            format!(
                "<rules lang=\"pl\"><unification feature=\"number\"><equivalence type=\"sg\"><token postag=\".*\\bsg\\b.*\" postag_regexp=\"yes\"/></equivalence><equivalence type=\"pl\"><token postag=\".*\\bpl(?:tant)?\\b.*\" postag_regexp=\"yes\"/></equivalence></unification><unification feature=\"gender\"><equivalence type=\"f\"><token postag=\".*\\bf\\b.*\" postag_regexp=\"yes\"/></equivalence></unification><unification feature=\"case\"><equivalence type=\"nom\"><token postag=\".*\\bnom\\b.*\" postag_regexp=\"yes\"/></equivalence></unification><rule name=\"t\" id=\"T1\"><pattern>{token_body}</pattern><disambig action=\"add\"><wd pos=\"TEST\"/></disambig></rule></rules>"
            )
        };
        let shapes = [
            ("<token>abc</token>", "plain"),
            (
                "<token>abc<exception postag=\"SENT_END\"/></token>",
                "text-then-exception",
            ),
            (
                "<token><exception postag=\"SENT_END\"/>abc</token>",
                "exception-then-text",
            ),
            (
                "<token regexp=\"yes\">[abc]+<exception postag=\"SENT_END\"/></token>",
                "regex-text-then-exception",
            ),
            (
                "<token regexp=\"yes\">[\\p{Punct}-\\u{2212}\\u{2013}\\u{2014}\\u{2015}\\u{201e}\\u{00ab}\\u{00bb}\\u{2026}\\u{201d}&&[^!.]]<exception postag=\"SENT_END\"/><exception postag=\"interp:nospace\"/></token>",
                "real-char-class-2-exc",
            ),
            (
                "<token regexp=\"yes\">[\\p{Punct}-\\u{2212}\\u{2013}\\u{2014}\\u{2015}\\u{201e}\\u{00ab}\\u{00bb}\\u{2026}\\u{201d}&&[^!.]]</token>",
                "real-char-class-no-exc",
            ),
        ];
        let real = "[\\p{Punct}-\\u{2212}\\u{2013}\\u{2014}\\u{2015}\\u{201e}\\u{00ab}\\u{00bb}\\u{2026}\\u{201d}&&[^!.]]";
        match regex::Regex::new(real) {
            Ok(re) => eprintln!(
                "regex-crate real class OK: w={} ,={} .={}",
                re.is_match("w"),
                re.is_match(","),
                re.is_match(".")
            ),
            Err(e) => eprintln!("regex-crate real class FAIL: {e}"),
        }
        let real2 = "[\\p{Punct}\\-\\u{2013}\\u{2212}\\u{2014}\\u{2015}\\u{201e}\\u{00ab}\\u{00bb}\\u{2026}\\u{201d}\\u{2019}&&[^\\)]]+";
        match regex::Regex::new(real2) {
            Ok(re) => eprintln!(
                "regex-crate grammar-entity class OK: w={} ,={}",
                re.is_match("w"),
                re.is_match(",")
            ),
            Err(e) => eprintln!("regex-crate grammar-entity class FAIL: {e}"),
        }
        for (body, label) in shapes {
            let xml = mk(body);
            match og_tagger::XmlDisambiguator::from_xml_with_english(&xml, None) {
                Ok(d) => {
                    let dbg = d.rule_pattern_debug(0);
                    eprintln!("{label}: {dbg}");
                }
                Err(e) => eprintln!("{label}: PARSE FAIL {e}"),
            }
        }
    }

    #[ignore]
    #[test]
    fn probe_double_hyphen_rule() {
        pattern_of("LACZNIK_MYSLNIK");
        probe("LACZNIK_MYSLNIK", "Rozumiesz -- przegrywasz.");
    }

    #[ignore]
    #[test]
    fn probe_zdania_zlozone_family() {
        pattern_of("ZDANIA_ZLOZONE");
        probe(
            "ZDANIA_ZLOZONE",
            "Kupi\u{142}em wino chcia\u{142}em te\u{17c} pi\u{107} wod\u{119}.",
        );
    }

    #[ignore]
    #[test]
    fn probe_adj_subst_adj_unify_residue() {
        pattern_of("ADJ_SUBST_ADJ_UNIFY");
        probe(
            "ADJ_SUBST_ADJ_UNIFY",
            "Beztlenowe bakterie magnetotaktyczna maj\u{105} funkcj\u{119} wykrywania tlenu.",
        );
        probe(
            "ADJ_SUBST_ADJ_UNIFY",
            "Sosna jest du\u{17c}y drzewem iglastym.",
        );
        probe(
            "ADJ_SUBST_ADJ_UNIFY",
            "Lubi\u{119} pi\u{142}k\u{119} no\u{17c}n\u{105}.",
        );
    }

    #[ignore]
    #[test]
    fn probe_weekday_and_name_rules() {
        pattern_of("DNI_TYGODNIA");
        probe(
            "DNI_TYGODNIA",
            "To si\u{119} sta\u{142}o w wielki pi\u{105}tek.",
        );
        pattern_of("BIALY_STOK");
        probe(
            "BIALY_STOK",
            "Jakie zabytki nadaja si\u{119} na poczt\u{f3}wki w Bia\u{142}ym stoku?",
        );
        pattern_of("HYPER");
        probe(
            "HYPER",
            "W tym tek\u{15b}cie s\u{105} liczne hyperodno\u{15b}niki.",
        );
    }
}
