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
        let xml = crate::pl::normalize_pl_rules_xml(&xml);
        XmlCompiler::new()
            .compile_file_with_base(xml.as_ref(), path.parent())
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
    fn probe_nietypowa_antipattern_trace() {
        probe_with_engine_trace(
            "NIETYPOWA_KOMBINACJA_DUZYCH_I_MALYCH_LITER",
            "To jest dziwnie napisane s\u{141}oWo.",
        );
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

    #[ignore]
    #[test]
    fn probe_round10_single_residues() {
        probe(
            "JEDNAK_PRZECINEK",
            "Jednak\u{17c}e, warto spr\u{f3}bowa\u{107}.",
        );
        probe("WIECEJ_JAK", "Wi\u{119}cej jak Kosmetyk!");
        probe(
            "NIETYPOWA_KOMBINACJA_DUZYCH_I_MALYCH_LITER",
            "To jest dziwnie napisane s\u{141}oWo.",
        );
        probe("O_WYM", "Rozmawiali o tym i o wym.");
        probe("WLADCE", "Surowce zawieraj\u{105}ce te alkaloidy");
        probe(
            "SPACJA_ZA_PRZECINKIEM_DZIESITNYM",
            "Wzr\u{f3}s\u{142} zaledwie o 1, 9%.",
        );
        probe("NIEZGODNOSC_PRZYPADKU_PO_LICZEBNIKU", "Mam 1,5% chleb.");
        probe(
            "MNIE_MNIEJ",
            "P\u{142}ac\u{105} nawet 40% mnie ni\u{17c} w sklepie.",
        );
        probe(
            "SKROTY_Z_KROPKA",
            "Stopie\u{144} dysocjacji w temperaturze 4000 K wynosi nieca\u{142}e 3%.",
        );
        probe("PO_SRODKU", "Prawda po \u{15b}rodku");
        probe("PO_WOLI", "\u{15a}pieszcie si\u{119} po woli");
        probe("BRAK_KROPKI", "Po\u{17c}ycz mi sto z\u{142}otych.");
        probe("BRAK_KROPKI", "Po\u{17c}ycz mi sto z\u{142}otych\u{2026}");
        probe(
            "BRAK_KROPKI",
            "To jest zdanie z \u{201e}cytatem na ko\u{144}cu i przypisem\u{201d}.",
        );
        probe("RYMY", "Tu wyl\u{105}dowa\u{142}y oddzia\u{142}y.");
        probe("PREP_CASUS", "By\u{142}em przy grobie Mickiewicza.");
        probe(
            "UZYWAC_CO",
            "Chcia\u{142}by\u{15b} u\u{17c}y\u{107} troch\u{119} \u{17c}ycia?",
        );
        probe(
            "ZA_ZWYCZAJ",
            "W Egipcie wszyscy dygnitarze uwa\u{17c}aj\u{105} \u{142}ap\u{f3}wki za zwyczaj tak powszechny...",
        );
        probe("IMIONA_Z_APOSTROFAMI", "Porozmawiajmy o IPN'ie.");
    }

    #[ignore]
    #[test]
    fn probe_disambig_lemma_remove_actions() {
        let resource_dir = crate::pl::tagger::pl_resource_dir().expect("pl resource dir");
        let xml = std::fs::read_to_string(resource_dir.join("disambiguation.xml")).unwrap();
        let normalized = crate::pl::normalize_disambiguation_xml(&xml);
        let parsed = og_tagger::XmlDisambiguator::from_xml_with_english(&normalized, None)
            .expect("parse disambiguation.xml");
        for wanted in ["K", "k_bez_kropki"] {
            let Some(idx) = parsed.rule_ids().iter().position(|id| *id == wanted) else {
                eprintln!("rule {wanted}: NOT FOUND");
                continue;
            };
            eprintln!("rule {wanted} idx {idx}: {}", parsed.rule_debug(idx));
            let text = "Stopie\u{144} dysocjacji w temperaturze 4000 K wynosi nieca\u{142}e 3%.";
            let mut sentence =
                crate::pl::test_support::analyze_undisambiguated(text, 0, text.len());
            parsed.apply_rule(&mut sentence, idx);
            for t in sentence.non_whitespace_tokens() {
                let readings: Vec<String> = t
                    .readings()
                    .iter()
                    .map(|r| format!("{}[{}]", r.lemma().unwrap_or("-"), r.pos_tags().join("+")))
                    .collect();
                eprintln!(
                    "   after {wanted}: {:?} => {}",
                    t.token().token(),
                    readings.join(", ")
                );
            }
        }
    }

    #[ignore]
    #[test]
    fn probe_round15_zdania_zlozone_residues() {
        for text in [
            "Romuald odpowiedzia\u{142} z wa\u{17c}n\u{105} i tajemnicz\u{105} min\u{105} zwielokrotniaj\u{105}c g\u{142}osk\u{119}.",
            "Je\u{15b}li w w\u{119}glowodorze wyst\u{119}puje wi\u{105}zanie podw\u{f3}jne i potr\u{f3}jne to wa\u{17c}niejsze przy nazewnictwie jest podw\u{f3}jne.",
            "Fatma uca\u{142}owawszy r\u{119}ce obu in\u{17c}ynier\u{f3}w i dzieci wysz\u{142}a, b\u{142}ogos\u{142}awi\u{105}c szczeg\u{f3}lniej Nel.",
            "Kupi\u{142}em wino chcia\u{142}em te\u{17c} pi\u{107} wod\u{119}.",
        ] {
            probe("ZDANIA_ZLOZONE", text);
        }
    }

    #[ignore]
    #[test]
    fn probe_round15_zdania_zlozone_engine_trace() {
        probe_with_engine_trace(
            "ZDANIA_ZLOZONE",
            "Romuald odpowiedzia\u{142} z wa\u{17c}n\u{105} i tajemnicz\u{105} min\u{105} zwielokrotniaj\u{105}c g\u{142}osk\u{119}.",
        );
    }

    #[ignore]
    #[test]
    fn probe_round15_remaining_singles() {
        probe(
            "ZAGNIEZDZONE_CUDZYSLOWY",
            "\u{201e}Ka\u{17c}dy noemat ma \u{2018}tre\u{15b}\u{107}\u{2019}\u{201d}.",
        );
        probe(
            "ADJ_SUBST_ADJ_UNIFY",
            "Beztlenowe bakterie magnetotaktyczna maj\u{105} funkcj\u{119} wykrywania tlenu.",
        );
        probe(
            "ZDANIE_PODRZEDNE_Z_KTORY_LUB_JAKI",
            "I skoczy\u{142} ku domowi, w kt\u{f3}rego sieni s\u{142}ycha\u{107} by\u{142}o \u{15b}iej\u{105}ce si\u{119} g\u{142}osy i huk obracanych \u{17c}aren.",
        );
        probe(
            "NIETYPOWA_KOMBINACJA_DUZYCH_I_MALYCH_LITER",
            "To jest dziwnie napisane s\u{141}oWo.",
        );
        probe("O_WYM", "Rozmawiali o tym i o wym.");
        probe("WLADCE", "Surowce zawieraj\u{105}ce te alkaloidy");
        probe(
            "BRAK_PRZECINKA_ZE",
            "I c\u{f3}\u{17c}, stary Balu \u{2014} doda\u{142}a \u{2014} jak \u{17c}e\u{15b} wyszed\u{142} z tej przygody?",
        );
        probe(
            "BRAK_PRZECINKA_GDY",
            "Co znajdujemy gdy \u{201e}spogl\u{105}damy\u{201d} wewn\u{105}trz?",
        );
        probe(
            "BRAK_PRZECINKA_ANI",
            "Dojad\u{119} tam b\u{105}d\u{17a} rowerem b\u{105}d\u{17a} hulajnog\u{105}.",
        );
        probe("PREP_CASUS", "By\u{142}em przy grobie Mickiewicza.");
        probe("RYMY", "Tu wyl\u{105}dowa\u{142}y oddzia\u{142}y.");
        probe("IMIONA_Z_APOSTROFAMI", "Porozmawiajmy o IPN'ie.");
        probe("PO_SRODKU", "Prawda po \u{15b}rodku");
        probe("PO_WOLI", "\u{15a}pieszcie si\u{119} po woli");
        probe(
            "ZA_ZWYCZAJ",
            "W Egipcie wszyscy dygnitarze uwa\u{17c}aj\u{105} \u{142}ap\u{f3}wki za zwyczaj tak powszechny...",
        );
    }

    #[ignore]
    #[test]
    fn probe_round15_disambig_rule_effects() {
        let resource_dir = crate::pl::tagger::pl_resource_dir().expect("pl resource dir");
        let xml = std::fs::read_to_string(resource_dir.join("disambiguation.xml")).unwrap();
        let normalized = crate::pl::normalize_disambiguation_xml(&xml);
        let parsed = og_tagger::XmlDisambiguator::from_xml_with_english(&normalized, None)
            .expect("parse disambiguation.xml");
        let snapshot = |s: &og_core::AnalyzedSentence, watched: &str| -> Vec<String> {
            s.non_whitespace_tokens()
                .iter()
                .filter(|t| t.token().token() == watched)
                .map(|t| {
                    t.readings()
                        .iter()
                        .map(|r| r.pos_tags().join("|"))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .collect()
        };
        let cases: &[(&str, &str)] = &[
            (
                "I c\u{f3}\u{17c}, stary Balu \u{2014} doda\u{142}a \u{2014} jak \u{17c}e\u{15b} wyszed\u{142} z tej przygody?",
                "jak",
            ),
            ("By\u{142}em przy grobie Mickiewicza.", "grobie"),
            (
                "Tu wyl\u{105}dowa\u{142}y oddzia\u{142}y.",
                "wyl\u{105}dowa\u{142}y",
            ),
            ("Rozmawiali o tym i o wym.", "wym"),
            ("O tym i o wym", "wym"),
            ("\u{15a}pieszcie si\u{119} po woli", "woli"),
            ("Prawda po \u{15b}rodku", "\u{15b}rodku"),
            (
                "Dojad\u{119} tam b\u{105}d\u{17a} rowerem b\u{105}d\u{17a} hulajnog\u{105}.",
                "b\u{105}d\u{17a}",
            ),
        ];
        for (text, watched) in cases {
            eprintln!("=== '{text}' watching {watched:?} ===");
            let mut sentence =
                crate::pl::test_support::analyze_undisambiguated(text, 0, text.len());
            eprintln!("   pre  {watched:?}: {:?}", snapshot(&sentence, watched));
            for (idx, id) in parsed.rule_ids().iter().enumerate() {
                let before = snapshot(&sentence, watched);
                parsed.apply_rule(&mut sentence, idx);
                let after = snapshot(&sentence, watched);
                if before != after {
                    eprintln!("   rule {id} (idx {idx}): {before:?} -> {after:?}");
                }
            }
            eprintln!("   post {watched:?}: {:?}", snapshot(&sentence, watched));
        }
    }

    #[ignore]
    #[test]
    fn probe_round15_o_wym_disambig_rule() {
        let resource_dir = crate::pl::tagger::pl_resource_dir().expect("pl resource dir");
        let xml = std::fs::read_to_string(resource_dir.join("disambiguation.xml")).unwrap();
        let normalized = crate::pl::normalize_disambiguation_xml(&xml);
        let parsed = og_tagger::XmlDisambiguator::from_xml_with_english(&normalized, None)
            .expect("parse disambiguation.xml");
        let snapshot = |s: &og_core::AnalyzedSentence| -> Vec<String> {
            s.non_whitespace_tokens()
                .iter()
                .map(|t| {
                    format!(
                        "{:?}:{}",
                        t.token().token(),
                        t.readings()
                            .iter()
                            .map(|r| r.pos_tags().join("|"))
                            .collect::<Vec<_>>()
                            .join(",")
                    )
                })
                .collect()
        };
        for wanted in ["O_WYM_", "o_wym_2"] {
            let Some(idx) = parsed.rule_ids().iter().position(|id| *id == wanted) else {
                eprintln!("rule {wanted}: NOT FOUND");
                continue;
            };
            eprintln!("== rule {wanted} (idx {idx}) ==");
            eprintln!("{}", parsed.rule_debug(idx));
            for text in ["Rozmawiali o tym i o wym.", "O tym i o wym"] {
                let mut sentence =
                    crate::pl::test_support::analyze_undisambiguated(text, 0, text.len());
                let before = snapshot(&sentence);
                parsed.apply_rule(&mut sentence, idx);
                let after = snapshot(&sentence);
                eprintln!("  '{text}'");
                eprintln!("    before {before:?}");
                eprintln!("    after  {after:?}");
            }
        }
    }

    #[ignore]
    #[test]
    fn probe_round15_zagniezdzone_wladce_traces() {
        pattern_of("ZAGNIEZDZONE_CUDZYSLOWY");
        probe_with_engine_trace(
            "ZAGNIEZDZONE_CUDZYSLOWY",
            "\u{201e}Ka\u{17c}dy noemat ma \u{2018}tre\u{15b}\u{107}\u{2019}\u{201d}.",
        );
        pattern_of("WLADCE");
        probe_with_engine_trace("WLADCE", "Surowce zawieraj\u{105}ce te alkaloidy");
    }

    #[test]
    fn quote_class_survives_java_ci_compilation() {
        let plain = regex::RegexBuilder::new("^(?:[\u{2018}\u{201e}])$")
            .case_insensitive(true)
            .build()
            .unwrap();
        assert!(
            plain.is_match("\u{2018}"),
            "plain ci compile matches the class member"
        );
        let folded = og_xml::java_case_fold::compile_java_ci("^(?:[\u{2018}\u{201e}])$");
        assert!(
            folded.as_ref().is_some_and(|re| re.is_match("\u{2018}")),
            "java_case_fold must keep the quote class intact"
        );
    }

    #[ignore]
    #[test]
    fn probe_round15_prep_casus_trace() {
        probe_with_engine_trace("PREP_CASUS", "By\u{142}em przy grobie Mickiewicza.");
    }

    #[ignore]
    #[test]
    fn probe_round15_rymy_trace() {
        pattern_of("RYMY");
        probe_with_engine_trace("RYMY", "Tu wyl\u{105}dowa\u{142}y oddzia\u{142}y.");
    }

    #[test]
    fn regex_crate_rejects_java_octal_and_inline_unicode_flag_negation() {
        let brak_kropki_exception_class = "[?\\.!,:;…\u{201d}\"\\02]";
        assert!(
            regex::Regex::new(brak_kropki_exception_class).is_err(),
            "the BRAK_KROPKI marker exception class must fail the regex crate (octal \\02)"
        );
        let dni_tygodnia_exception = "(?-iu)w.*";
        assert!(
            regex::Regex::new(dni_tygodnia_exception).is_err(),
            "the DNI_TYGODNIA antipattern exceptions must fail the regex crate (u cannot be \\
             disabled on &str regexes)"
        );
    }

    #[test]
    fn pl_disambiguation_carries_95_lemma_only_remove_actions() {
        let resource_dir = crate::pl::tagger::pl_resource_dir().expect("pl resource dir");
        let xml = std::fs::read_to_string(resource_dir.join("disambiguation.xml")).unwrap();
        let normalized = crate::pl::normalize_disambiguation_xml(&xml);
        let parsed = og_tagger::XmlDisambiguator::from_xml_with_english(&normalized, None)
            .expect("parse disambiguation.xml");
        assert!(
            parsed.rule_ids().iter().any(|id| *id == "K"),
            "the K rule must parse"
        );
        assert_eq!(
            xml.matches("action=\"remove\"><wd lemma").count(),
            95,
            "the lemma-only remove action count the og-tagger wildcard-pos fix must unlock"
        );
    }

    #[test]
    fn sentence_final_word_carries_sent_end_before_disambiguation() {
        let text = "\u{15a}pieszcie si\u{119} po woli";
        let sentence = crate::pl::test_support::analyze_undisambiguated(text, 0, text.len());
        let tokens = sentence.non_whitespace_tokens();
        let woli = tokens.last().expect("final token");
        assert_eq!(woli.token().token(), "woli");
        assert!(
            woli.is_sentence_end(),
            "the pipeline adds the SENT_END reading before disambiguation, so the PO_WOLI \\
             residue isolates the loss to a disambiguator path that Java guards with \\
             AnalyzedTokenReadings.java:377 / DisambiguationPatternRuleReplacer.java:176"
        );
    }
}
