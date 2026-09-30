use og_test_runner::runner::TestRunner;
use og_xml::compiler::XmlCompiler;

fn main() {
    let cases: &[(&str, &str)] = &[
        (
            "IMP_AUX_SPELLING_RULE",
            "Stimmt es, dass du das Auto reparier hast?",
        ),
        (
            "HOCH_ADJ",
            "Er hat ein hoch effizientes Heilmittel entwickelt.",
        ),
        (
            "ZUSAMMENSCHREIBUNG_HER",
            "Kannst du ihn bitte rüber lassen?",
        ),
        (
            "DEF_ARTIKEL_INDEF_ADJ",
            "Für die italienische Mafia ist das heiliges Gebiet.",
        ),
        (
            "KOMMA_ZWISCHEN_HAUPT_UND_NEBENSATZ",
            "Dies ist das Bild welches er malte.",
        ),
        (
            "ZUSAMMENGESETZTE_VERBEN",
            "Wenn du nicht vor hast zu helfen, brauchst du hier nicht herumzustehen und zu meckern.",
        ),
        (
            "ZUSAMMENSCHREIBUNG_HINTER",
            "Er hat ihr hinterher spioniert.",
        ),
        (
            "GESTERN_MIT_PRAESENS",
            "Gestern willst du sechzehn Stunden am Stück gearbeitet haben?",
        ),
        (
            "PRAEPOSITION_NICHTDEKLINIERTES_ADJEKTIV",
            "Über Kurz oder lang wird er verlieren.",
        ),
        (
            "SUB_VER_KLEIN",
            "Wo soll ich Anfangen, wenn ich bauen möchte?",
        ),
        ("VERB_FEM_SUBST", "weil ich diese bestehe."),
        (
            "GROSSSCHREIBUNG_MAL",
            "Sie hatte es ihm einige mal gezeigt.",
        ),
        ("SCHOENE_WETTER", "Herzliche Dank für die schnelle Hilfe."),
        (
            "FRAGEZEICHEN_NACH_DIREKTER_REDE",
            "„Was willst du“, fragte er ungehalten.",
        ),
        ("TYPOGRAFISCHE_APOSTROPHE", "Ich geb' mir Mühe."),
        (
            "AUF_DEM_PUNKT",
            "Bringen wir die Sache endlich mal auf dem Punkt!",
        ),
        ("AUF_DEM_PLAN_RUFEN", "Das rief ihn auf dem Plan"),
        (
            "UNTER_DEM_TEPPICH_KEHREN",
            "Kehren wir die Sache nicht länger unter dem Teppich!",
        ),
    ];

    let grammar_path = "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/rules/de/grammar.xml";
    let xml = std::fs::read_to_string(grammar_path).unwrap_or_else(|e| panic!("{e}"));
    let base = std::path::Path::new(grammar_path)
        .parent()
        .map(std::path::Path::to_path_buf);
    let rule_set = XmlCompiler::new()
        .compile_file_with_base(&xml, base.as_deref())
        .unwrap_or_else(|e| panic!("compile: {e}"));
    let runner = TestRunner::for_language("de");
    og_rules::pattern_rule::set_test_date_hack(true);

    let inline: &[(&str, &str, &str)] = &[
        (
            "SCHOENE_WETTER_PROBE",
            r#"<pattern><token postag="SENT_START|VER.*" postag_regexp="yes"/><token postag="ADV.*" postag_regexp="yes" min="0"/><token postag="(ADJ|PA[12]).*" postag_regexp="yes" regexp="yes">[a-zäöü].+e</token><token postag="SUB.*SIN.*MAS" postag_regexp="yes"/></pattern>"#,
            "Herzliche Dank für die schnelle Hilfe.",
        ),
        (
            "SCHOENE_WETTER_NOOPT",
            r#"<pattern><token postag="SENT_START|VER.*" postag_regexp="yes"/><token postag="(ADJ|PA[12]).*" postag_regexp="yes" regexp="yes">[a-zäöü].+e</token><token postag="SUB.*SIN.*MAS" postag_regexp="yes"/></pattern>"#,
            "Herzliche Dank für die schnelle Hilfe.",
        ),
        (
            "SCHOENE_WETTER_ADJ_ONLY",
            r#"<pattern><token postag="(ADJ|PA[12]).*" postag_regexp="yes" regexp="yes">[a-zäöü].+e</token></pattern>"#,
            "Herzliche Dank für die schnelle Hilfe.",
        ),
        (
            "KOMMA_SKIP",
            r#"<pattern><token postag="(VER:[1-3]:.*|VER.*:[1-3]:.*|VER:IMP:.*)" postag_regexp="yes" skip="-1"><exception postag="VER:(INF|MOD):.*|VER:AUX:.*:KJ[12]|PA[12]:.*|VER:PA2.*|SUB:.*|EIG:.*|ADJ:.*|ART:.*|PRO:.*|ADV.*|PRP:.*|NEG:.*|ABK:.*|ZUS.*|ZAL.*" postag_regexp="yes"/><exception inflected="yes" regexp="yes">bleiben|lassen|versuchen|bemühen|sieben|achten|wetter|feuer|web|hure</exception><exception>bitte</exception><exception regexp="yes" scope="next">,|\.|:|\?|!|;|-|–|—|─|~|\)|\(|„|“|»|«|"|\*|und|oder|&amp;|…|→|•|=|\/|'|‚|‘|\u2063</exception><exception regexp="yes" scope="next">weil|ob(wohl|gleich)?|da(mit|ss)?|indem|wenn|falls|während|bevor|nachdem|seitdem|so(bald|dass|fern|lange)|was|als|wie|bis|wer|wo|wohin|sondern|aber</exception></token><token postag="VER:[1-3]:.*|VER.*:[1-3]:.*|VER:IMP:.*" postag_regexp="yes" regexp="yes" case_sensitive="yes">[a-zäöü]+<exception postag="VER:(INF:|PA2).*|VER:MOD:INF.*|PA[12]:.*|SUB:.*|EIG:.*|ADJ:.*|ART:.*|PRO:.*|ADV.*|PRP:.*|NEG:.*|ABK:.*|ZUS.*|ZAL.*" postag_regexp="yes"/><exception inflected="yes" regexp="yes">bleiben|lassen|sieben|achten|wetter|feuer|web|hure</exception><exception regexp="yes">bitte|halt|paff|mindere|lichte|sog|rauf|vermittelst</exception><exception postag="VER:IMP:SIN:NON" /><exception scope="next">,</exception></token></pattern>"#,
            "Dies ist das Bild welches er malte.",
        ),
        (
            "KOMMA_SKIP_MIN",
            r#"<pattern><token postag="VER.*" postag_regexp="yes" skip="-1"/><token postag="VER.*" postag_regexp="yes"/></pattern>"#,
            "Dies ist das Bild welches er malte.",
        ),
        (
            "KOMMA_BISECT",
            r#"<pattern><token>nomatch</token></pattern>"#,
            "Dies ist das Bild welches er malte.",
        ),
        (
            "SCHOENE_WETTER_BISECT",
            r#"<pattern><token>nomatch</token></pattern>"#,
            "Herzliche Dank für die schnelle Hilfe.",
        ),
        (
            "GAP_LITERAL",
            r#"<pattern><token>auf</token><token min="0" max="3">dem</token><token>Punkt</token></pattern>"#,
            "auf dem Punkt",
        ),
        (
            "PLAIN_BRINGEN",
            r#"<pattern><token>bringen</token><token>wir</token></pattern>"#,
            "Bringen wir die Sache endlich mal auf dem Punkt!",
        ),
        (
            "INFLECTED_BRINGEN",
            r#"<pattern><token inflected="yes">bringen</token><token>wir</token></pattern>"#,
            "Bringen wir die Sache endlich mal auf dem Punkt!",
        ),
        (
            "INFLECTED_BRINGEN2",
            r#"<pattern><token inflected="yes">bringen</token><token>wir</token></pattern>"#,
            "Wir bringen die Sache auf den Punkt.",
        ),
        (
            "INFLECTED_RIEF",
            r#"<pattern><token inflected="yes">rufen</token><token>ihn</token></pattern>"#,
            "Das rief ihn auf dem Plan",
        ),
        (
            "INFLECTED_NOGAP",
            r#"<pattern><token inflected="yes">bringen</token><token min="0">nicht</token><token>auf</token><token>dem</token><token>Punkt</token></pattern>"#,
            "Bringen wir die Sache endlich mal auf dem Punkt!",
        ),
        (
            "GAP_NOMARKER",
            r#"<pattern><token inflected="yes">bringen</token><token min="0" max="6"/><token min="0">nicht</token><token>auf</token><token>dem</token><token>Punkt</token></pattern>"#,
            "Bringen wir die Sache endlich mal auf dem Punkt!",
        ),
        (
            "GAP_MARKER",
            r#"<pattern><token inflected="yes">bringen</token><token min="0" max="6"/><token min="0">nicht</token><token>auf</token><marker><token>dem</token></marker><token>Punkt</token></pattern>"#,
            "Bringen wir die Sache endlich mal auf dem Punkt!",
        ),
        (
            "GAP_MARKER_PLAIN",
            r#"<pattern><token>Bringen</token><token min="0" max="6"/><token min="0">nicht</token><token>auf</token><marker><token>dem</token></marker><token>Punkt</token></pattern>"#,
            "Bringen wir die Sache endlich mal auf dem Punkt!",
        ),
    ];
    for (id, pattern, text) in inline {
        let xml = format!(
            r#"<rules lang="de"><rule id="{id}" name="x">{pattern}<message>m</message></rule></rules>"#
        );
        let rs = XmlCompiler::new().compile_file(&xml).expect("compile");
        let r = &rs.rules[0];
        if *id == "KOMMA_BISECT" {
            let path = "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/rules/de/grammar.xml";
            let xml_full = std::fs::read_to_string(path).unwrap();
            let base = std::path::Path::new(path)
                .parent()
                .map(std::path::Path::to_path_buf);
            let full = XmlCompiler::new()
                .compile_file_with_base(&xml_full, base.as_deref())
                .unwrap();
            let text = "Dies ist das Bild welches er malte.";
            for rule in &full.rules {
                if rule.id != "KOMMA_ZWISCHEN_HAUPT_UND_NEBENSATZ" {
                    continue;
                }
                let base_n = runner.match_rule_on_text(rule, text).2.len();
                let mut no_ap = rule.clone();
                no_ap.antipatterns.clear();
                let no_ap_n = runner.match_rule_on_text(&no_ap, text).2.len();
                let mut no_exc = rule.clone();
                for t in no_exc.pattern.tokens.iter_mut() {
                    t.exceptions.clear();
                }
                for t in no_exc.antipatterns.iter_mut() {
                    for tt in t.tokens.iter_mut() {
                        tt.exceptions.clear();
                    }
                }
                let no_exc_n = runner.match_rule_on_text(&no_exc, text).2.len();
                println!(
                    "KBISECT: full={base_n} no_antipatterns={no_ap_n} no_exceptions={no_exc_n}"
                );
                for (ai, _ap) in rule.antipatterns.iter().enumerate() {
                    let mut only = rule.clone();
                    only.antipatterns = vec![rule.antipatterns[ai].clone()];
                    if runner.match_rule_on_text(&only, text).2.len() == 0 {
                        println!("  KBLOCKING antipattern #{ai}");
                        for (ti, t) in _ap.tokens.iter().enumerate() {
                            println!(
                                "    ap-token[{ti}] text={:?} postag={:?} re={:?} cs={} min={:?} skip={}",
                                t.text,
                                t.postag,
                                t.compiled_regexp.as_ref().map(|r| r.as_str()),
                                t.case_sensitive,
                                t.min,
                                t.skip
                            );
                        }
                    }
                }
            }
        }
        if *id == "SCHOENE_WETTER_BISECT" {
            let path = "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/rules/de/grammar.xml";
            let xml_full = std::fs::read_to_string(path).unwrap();
            let base = std::path::Path::new(path)
                .parent()
                .map(std::path::Path::to_path_buf);
            let full = XmlCompiler::new()
                .compile_file_with_base(&xml_full, base.as_deref())
                .unwrap();
            let text = "Herzliche Dank für die schnelle Hilfe.";
            for rule in &full.rules {
                if rule.id != "SCHOENE_WETTER" {
                    continue;
                }
                let base_n = runner.match_rule_on_text(rule, text).2.len();
                let mut no_ap = rule.clone();
                no_ap.antipatterns.clear();
                let no_ap_n = runner.match_rule_on_text(&no_ap, text).2.len();
                let mut no_exc = rule.clone();
                for t in no_exc.pattern.tokens.iter_mut() {
                    t.exceptions.clear();
                }
                let no_exc_n = runner.match_rule_on_text(&no_exc, text).2.len();
                println!(
                    "BISECT subrule {:?}: full={base_n} no_antipatterns={no_ap_n} no_exceptions={no_exc_n}",
                    rule.sub_id
                );
                for (ai, _ap) in rule.antipatterns.iter().enumerate() {
                    let mut only = rule.clone();
                    only.antipatterns = vec![rule.antipatterns[ai].clone()];
                    if runner.match_rule_on_text(&only, text).2.len() == 0 {
                        println!(
                            "  BLOCKING antipattern #{ai} with {} tokens",
                            _ap.tokens.len()
                        );
                        for (ti, t) in _ap.tokens.iter().enumerate() {
                            println!(
                                "    ap-token[{ti}] text={:?} postag={:?} regexp={} cs={} min={:?} max={:?} skip={}",
                                t.text,
                                t.postag,
                                t.compiled_regexp.is_some(),
                                t.case_sensitive,
                                t.min,
                                t.max,
                                t.skip
                            );
                        }
                    }
                }
            }
        }
        println!(
            "COMPILED {id}: {} tokens, structured={}",
            r.pattern.tokens.len(),
            r.pattern.has_structured_elements
        );
        for (i, t) in r.pattern.tokens.iter().enumerate() {
            println!(
                "  tok[{i}] text={:?} inflected={} min={:?} max={:?} skip={}",
                t.text, t.inflected, t.min, t.max, t.skip
            );
        }
        for (i, e) in r.pattern.elements.iter().enumerate() {
            match e {
                og_xml::compiler::CompiledPatternElement::Token(t) => println!(
                    "  elem[{i}] Token text={:?} min={:?} max={:?}",
                    t.text, t.min, t.max
                ),
                og_xml::compiler::CompiledPatternElement::OrGroup(a) => {
                    println!("  elem[{i}] OrGroup({} alts)", a.len())
                }
                og_xml::compiler::CompiledPatternElement::AndGroup(c) => {
                    println!("  elem[{i}] AndGroup({})", c.len())
                }
            }
        }
        let n = runner.match_rule_on_text(&rs.rules[0], text).2.len();
        println!("INLINE {id}: {n} matches on '{text}'");
    }

    for (rule_id, text) in cases {
        println!("\n=== {rule_id}: {text}");
        let sentence = runner.debug_sentence(text);
        for (i, token) in sentence.non_whitespace_tokens().iter().enumerate() {
            let chunk = token.chunk().unwrap_or("-");
            println!(
                "  [{i:2}] {:18} chunk={} readings:",
                format!("'{}'", token.token().token()),
                chunk
            );
            for r in token.readings() {
                println!(
                    "         {:18} {}",
                    format!("'{}'", r.lemma().unwrap_or("-")),
                    r.pos_tags().join(",")
                );
            }
        }
        for rule in &rule_set.rules {
            if rule.id == *rule_id {
                let matches = runner.match_rule_on_text(rule, text).2;
                println!("  -> subrule matches: {}", matches.len());
                for m in &matches {
                    let covered: String = text[m.offset()..m.offset() + m.length()].to_string();
                    println!("     match at {}..{} '{}'", m.offset(), m.length(), covered);
                }
            }
        }
    }
}
