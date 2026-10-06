use crate::pt::test_support::PortuguesePipeline;
use og_core::AnalyzedSentence;
use og_xml::compiler::{CompiledRule, CompiledRuleSet, XmlCompiler};
use std::path::PathBuf;
use std::sync::OnceLock;

fn rules_dir() -> PathBuf {
    crate::pt::filters::pt_rules_dir().expect("pt rules dir")
}

fn compiled_grammar() -> &'static CompiledRuleSet {
    static GRAMMAR: OnceLock<CompiledRuleSet> = OnceLock::new();
    GRAMMAR.get_or_init(|| {
        let path = rules_dir().join("grammar.xml");
        let xml = std::fs::read_to_string(&path).expect("grammar.xml");
        XmlCompiler::new()
            .compile_file_with_base(&xml, path.parent())
            .expect("compile rules xml")
    })
}

fn dump_readings(sentence: &AnalyzedSentence) {
    for t in sentence.non_whitespace_tokens() {
        let readings: Vec<String> = t
            .readings()
            .iter()
            .map(|r| format!("{}/{}", r.lemma().unwrap_or("-"), r.pos_tags().join("|")))
            .collect();
        eprintln!(
            "    [{: >2}] {:?}: {}",
            t.token().start(),
            t.token().token(),
            readings.join(", ")
        );
    }
}

fn rules_owning_example(text: &str) -> Vec<&'static CompiledRule> {
    compiled_grammar()
        .rules
        .iter()
        .filter(|r| r.examples.iter().any(|e| e.text.contains(text)))
        .collect()
}

pub fn probe(example_fragment: &str) {
    let engine = crate::pt::example_suite::suite_engine();
    let synthesizer = crate::pt::example_suite::suite_synthesizer();
    let pipeline = PortuguesePipeline::shared();
    og_rules::pattern_rule::set_test_date_hack(true);
    for rule in rules_owning_example(example_fragment) {
        let example = rule
            .examples
            .iter()
            .find(|e| e.text.contains(example_fragment))
            .expect("example");
        let clean = example
            .text
            .replace("<marker>", "")
            .replace("</marker>", "");
        eprintln!(
            "== {} / {:?} [type {:?}] filter={:?} '{}'",
            rule.id,
            rule.sub_id,
            example.example_type,
            rule.filter.as_ref().map(|f| f.class.as_str()),
            clean
        );
        for (sentence, sentence_start) in pipeline.analyze_offset_text(&clean) {
            let tokens: Vec<&og_core::AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
            eprintln!("  sentence @{sentence_start}: [{}]", sentence.text());
            dump_readings(&sentence);
            for m in engine.match_rule(rule, &sentence) {
                let span = m.offset()..m.offset() + m.length();
                eprintln!(
                    "  raw match {}..{} '{}'",
                    span.start,
                    span.end,
                    &sentence.text()[span.start.min(sentence.text().len())
                        ..span.end.min(sentence.text().len())]
                );
                match rule.filter.as_ref() {
                    Some(filter) => {
                        let (positions, texts) =
                            crate::pt::example_suite::matched_window(rule, &tokens, &m);
                        eprintln!("    window positions={positions:?} texts={texts:?}");
                        let filtered = crate::pt::example_suite::apply_suite_filter(
                            rule,
                            filter,
                            &sentence,
                            &tokens,
                            synthesizer.as_ref(),
                            m,
                        );
                        match filtered {
                            Some(m) => eprintln!(
                                "    filtered: kept @{}..{} reps={:?}",
                                m.offset(),
                                m.offset() + m.length(),
                                m.replacements()
                                    .iter()
                                    .map(|r| r.value().to_string())
                                    .collect::<Vec<_>>()
                            ),
                            None => eprintln!("    filtered: NULLED"),
                        }
                    }
                    None => eprintln!("    (no filter on this rule)"),
                }
            }
        }
    }
}

pub fn probe_dict(words: &[&str]) {
    let tagger = PortuguesePipeline::shared().tagger_ref();
    let tagged = og_tagger::Tagger::tag(tagger, words);
    for (word, atr) in words.iter().zip(tagged.iter()) {
        let readings: Vec<String> = atr
            .readings()
            .iter()
            .map(|r| format!("{}/{}", r.lemma().unwrap_or("-"), r.pos_tags().join("|")))
            .collect();
        eprintln!("  {word:?}: {}", readings.join(", "));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn trace_six_dispatch_residue_failures() {
        super::probe("das mares");
        super::probe("milênios atras");
        super::probe("traslado ao aeroporto");
        super::probe("discretamente");
        super::probe("impresso</marker>");
        super::probe("mil ienes");
        super::probe("teriam-na");
    }

    #[test]
    #[ignore]
    fn trace_dict_readings_of_suspect_words() {
        super::probe_dict(&[
            "das",
            "mares",
            "marés",
            "milênios",
            "milênio",
            "atras",
            "Quito",
            "quito",
            "limpo",
            "impresso",
            "imprimido",
            "mil",
            "gastado",
            "gasto",
            "aos",
            "ao",
            "no",
            "na",
        ]);
        let alt = std::path::PathBuf::from("/tmp/ptdict_probe");
        if alt.join("portuguese.dict").exists() {
            let tagger = super::super::tagger::PortugueseTagger::from_resource_dir(&alt);
            let words = [
                "das",
                "milênios",
                "milênio",
                "mil",
                "limpo",
                "impresso",
                "quito",
                "Quito",
                "mares",
            ];
            let tagged = og_tagger::Tagger::tag(&tagger, &words);
            eprintln!("  == alternate snapshot /tmp/ptdict_probe ==");
            for (word, atr) in words.iter().zip(tagged.iter()) {
                let readings: Vec<String> = atr
                    .readings()
                    .iter()
                    .map(|r| format!("{}/{}", r.lemma().unwrap_or("-"), r.pos_tags().join("|")))
                    .collect();
                eprintln!("  {word:?}: {}", readings.join(", "));
            }
        }
    }
}

#[cfg(test)]
mod round1_tests {
    use super::PortuguesePipeline;
    use super::XmlCompiler;

    #[test]
    #[ignore]
    fn trace_round1_global_disambiguation_and_style_residue() {
        super::probe("google.com.br");
        let engine = crate::pt::example_suite::suite_engine();
        let style = crate::pt::filters::pt_rules_dir()
            .expect("pt rules dir")
            .join("style.xml");
        let xml = std::fs::read_to_string(&style).expect("style.xml");
        let rule_set = XmlCompiler::new()
            .compile_file_with_base(&xml, style.parent())
            .expect("compile style.xml");
        let pipeline = PortuguesePipeline::shared();
        for (id, text) in [
            (
                "CIENTÍFICO_TERMOS_TÉCNICOS_TERMINOLOGIA",
                "Os termos técnicos são importantes.",
            ),
            (
                "SIMPLIFICAR_QUE_PRON_V_ANDAR_A_INFINITIVO",
                "Há décadas que me andava a arrastar sem objetivos.",
            ),
        ] {
            eprintln!("== {id} '{text}'");
            for rule in rule_set.rules.iter().filter(|r| r.id == id) {
                for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
                    for m in engine.match_rule(rule, &sentence) {
                        eprintln!(
                            "  sub {:?}: match {}..{} reps={:?}",
                            rule.sub_id,
                            m.offset() + sentence_start,
                            m.offset() + sentence_start + m.length(),
                            m.replacements()
                                .iter()
                                .map(|r| r.value().to_string())
                                .collect::<Vec<_>>()
                        );
                    }
                }
            }
        }
    }
}
