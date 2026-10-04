use crate::en::CompoundRule;
use og_core::rule::{Rule, TextLevelRule};
use std::path::PathBuf;
use std::sync::Arc;

pub mod dash_rule;
pub mod simple_replace;
pub mod speller;
pub mod tagger;
pub mod unpaired_brackets;
pub mod word_repeat;
pub mod word_tokenizer;

#[cfg(test)]
pub(crate) mod example_suite;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
pub(crate) mod trace;

pub fn pl_rules_dir() -> Option<PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("pl/src/main/resources/org/languagetool/rules/pl"))
}

pub fn normalize_pl_rules_xml(xml: &str) -> std::borrow::Cow<'_, str> {
    if !xml.contains("(?-iu)") && !xml.contains("\\0") {
        return std::borrow::Cow::Borrowed(xml);
    }
    let bytes = xml.as_bytes();
    let mut out = String::with_capacity(xml.len() + 96);
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'(' && xml[i..].starts_with("(?-iu)") {
            out.push_str("(?-i)");
            i += 6;
        } else if b == b'\\'
            && bytes.get(i + 1) == Some(&b'0')
            && is_java_octal_digit(bytes.get(i + 2))
        {
            let mut value = (bytes[i + 2] - b'0') as u32;
            let mut digits = 1usize;
            while digits < 3 && is_java_octal_digit(bytes.get(i + 2 + digits)) {
                let candidate = value * 8 + (bytes[i + 2 + digits] - b'0') as u32;
                if candidate > 0o377 {
                    break;
                }
                value = candidate;
                digits += 1;
            }
            out.push_str(&format!("\\x{{{value:x}}}"));
            i += 2 + digits;
        } else {
            let mut step = xml[i..].chars().next().map_or(1, char::len_utf8);
            if b == b'\\' && bytes.get(i + 1).is_some() {
                step = 1 + xml[i + 1..].chars().next().map_or(1, char::len_utf8);
            }
            out.push_str(&xml[i..i + step]);
            i += step;
        }
    }
    std::borrow::Cow::Owned(out)
}

fn is_java_octal_digit(d: Option<&u8>) -> bool {
    matches!(d, Some(d) if d.is_ascii_digit() && *d <= b'7')
}

fn normalize_disambiguation_xml(xml: &str) -> String {
    // Java quirks the shared crates cannot express: the bare dash is a literal class member,
    // and AnalyzedToken.matches treats a remove <wd> null pos as an any-pos wildcard.
    let escaped = xml.replace(
        "[\\p{Punct}-\u{2212}\u{2013}\u{2014}\u{2015}\u{201e}\u{00ab}\u{00bb}\u{2026}\u{201d}&amp;&amp;[^!\\.]",
        "[\\p{Punct}\\-\u{2212}\u{2013}\u{2014}\u{2015}\u{201e}\u{00ab}\u{00bb}\u{2026}\u{201d}&amp;&amp;[^!\\.]",
    );
    wildcard_lemma_only_remove_wds(&escaped)
}

fn wildcard_lemma_only_remove_wds(xml: &str) -> String {
    const OPEN: &str = "action=\"remove\"><wd ";
    let mut out = String::with_capacity(xml.len() + 460);
    let mut rest = xml;
    while let Some(found) = rest.find(OPEN) {
        let split = found + OPEN.len();
        out.push_str(&rest[..split]);
        rest = &rest[split..];
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        let attrs = &rest[..tag_end];
        let self_closing = attrs.ends_with('/');
        let attr_body = if self_closing {
            &attrs[..attrs.len() - 1]
        } else {
            attrs
        };
        if attr_body.contains("lemma=\"") && !attr_body.contains("pos=\"") {
            out.push_str(attr_body);
            out.push_str(" pos=\".*\"");
            if self_closing {
                out.push('/');
            }
            out.push('>');
        } else {
            out.push_str(attrs);
            out.push('>');
        }
        rest = &rest[tag_end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn build_pl_disambiguator(
    resource_dir: &std::path::Path,
) -> Option<Arc<dyn og_core::checker::Disambiguator>> {
    let xml = std::fs::read_to_string(resource_dir.join("disambiguation.xml")).ok()?;
    let normalized = normalize_disambiguation_xml(&xml);
    let disambiguator =
        og_tagger::XmlDisambiguator::from_xml_with_english(&normalized, None).ok()?;
    let core = crate::engine::find_lt_core_resource_dir()?;
    Some(og_tagger::hybrid_disambiguator(
        "pl",
        resource_dir,
        &core,
        disambiguator,
    ))
}

pub fn polish_compound_rule() -> CompoundRule {
    CompoundRule::for_language(
        "PL_COMPOUNDS",
        "Sprawdza wyrazy z łącznikiem, np. „łapu capu” zamiast „łapu-capu”",
        "Ten wyraz pisze się z łącznikiem.",
        "Ten wyraz pisze się razem (bez spacji ani łącznika).",
        "Ten wyraz pisze się z łącznikiem lub bez niego.",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/pl/src/main/resources/org/languagetool/resource/pl/compounds.txt"
        ),
    )
}

pub fn polish_word_coherency_rule() -> crate::en::WordCoherencyRule {
    crate::en::WordCoherencyRule::for_language(
        "PL_WORD_COHERENCY",
        "Jednolita pisownia wyrazów o obocznej dopuszczalnej pisowni",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/pl/src/main/resources/org/languagetool/rules/pl/coherency.txt"
        ),
    )
    .with_message_template(
        "Formy „{word1}” i „{word2}” zwykle nie powinny być używane jednocześnie.",
    )
}

pub fn pl_unpaired_brackets_rule() -> unpaired_brackets::PolishUnpairedBracketsRule {
    unpaired_brackets::PolishUnpairedBracketsRule::new()
}

pub struct PolishNativeRules {
    pub rules: Vec<std::sync::Arc<dyn Rule>>,
    pub text_level_rules: Vec<std::sync::Arc<dyn TextLevelRule>>,
}

pub fn polish_native_rules() -> PolishNativeRules {
    PolishNativeRules {
        rules: vec![
            std::sync::Arc::new(og_rules::native_rules::CommaWhitespaceRule::new()),
            std::sync::Arc::new(og_rules::native_rules::UppercaseSentenceStartRule::new()),
            std::sync::Arc::new(og_rules::native_rules::WordRepeatRule::new()),
            std::sync::Arc::new(speller::PolishSpellerRule::new()),
            std::sync::Arc::new(word_repeat::PolishWordRepeatRule::new()),
            std::sync::Arc::new(polish_compound_rule()),
            std::sync::Arc::new(simple_replace::PolishSimpleReplaceRule::new()),
            std::sync::Arc::new(dash_rule::PolishDashRule::new()),
        ],
        text_level_rules: vec![
            std::sync::Arc::new(og_rules::text_level_rules::MultipleWhitespaceRule::new()),
            std::sync::Arc::new(og_rules::text_level_rules::SentenceWhitespaceRule::new()),
            std::sync::Arc::new(pl_unpaired_brackets_rule()),
            std::sync::Arc::new(polish_word_coherency_rule()),
        ],
    }
}

pub fn priority_for_id(id: &str) -> i32 {
    if id == "ZDANIA_ZLOZONE" {
        return -1;
    }
    if id.eq_ignore_ascii_case("TOO_LONG_SENTENCE") {
        return -101;
    }
    if id == "REPETITIONS_STYLE" {
        return -55;
    }
    if id.contains("STYLE") {
        return -50;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_rules_carry_java_ids() {
        let native = polish_native_rules();
        let ids: Vec<&str> = native.rules.iter().map(|r| r.id()).collect();
        for expected in [
            "COMMA_PARENTHESIS_WHITESPACE",
            "UPPERCASE_SENTENCE_START",
            "WORD_REPEAT_RULE",
            "MORFOLOGIK_RULE_PL_PL",
            "PL_WORD_REPEAT",
            "PL_COMPOUNDS",
            "PL_SIMPLE_REPLACE",
            "PL_DASH_RULE",
        ] {
            assert!(ids.contains(&expected), "missing {expected} in {ids:?}");
        }
        let text_ids: Vec<&str> = native.text_level_rules.iter().map(|r| r.id()).collect();
        for expected in [
            "WHITESPACE_RULE",
            "SENTENCE_WHITESPACE",
            "PL_UNPAIRED_BRACKETS",
            "PL_WORD_COHERENCY",
        ] {
            assert!(
                text_ids.contains(&expected),
                "missing {expected} in {text_ids:?}"
            );
        }
    }

    #[test]
    fn normalize_disambiguation_xml_gives_lemma_only_remove_wds_the_java_wildcard_pos() {
        let xml = std::fs::read_to_string(
            crate::pl::tagger::pl_resource_dir()
                .expect("pl resource dir")
                .join("disambiguation.xml"),
        )
        .expect("disambiguation.xml");
        let normalized = normalize_disambiguation_xml(&xml);
        let self_closing = regex::Regex::new("action=\"remove\"><wd lemma=\"[^\"]*\"/>").unwrap();
        let empty_text = regex::Regex::new("action=\"remove\"><wd lemma=\"[^\"]*\"></wd>").unwrap();
        assert_eq!(self_closing.find_iter(&xml).count(), 88);
        assert_eq!(empty_text.find_iter(&xml).count(), 4);
        assert_eq!(self_closing.find_iter(&normalized).count(), 0);
        assert_eq!(empty_text.find_iter(&normalized).count(), 0);
        assert_eq!(xml.matches(" pos=\".*\"").count(), 0);
        assert_eq!(normalized.matches(" pos=\".*\"").count(), 92);
        assert_eq!(
            normalized.matches("<wd ").count(),
            xml.matches("<wd ").count(),
            "the rewrite must not create or drop wd elements"
        );
        assert!(normalized.contains("<wd lemma=\"ko\u{142}o\" pos=\".*\"/>"));
        assert!(normalized.contains("<wd lemma=\"mi\" pos=\".*\"></wd>"));
        assert_eq!(
            normalized
                .matches("<wd lemma=\"zdarzy\u{107}\" pos=\"refl\">")
                .count(),
            2,
            "wds that already carry pos stay untouched"
        );
    }

    #[test]
    fn java_pins_rule_k_removes_every_kolo_reading_of_the_k_token() {
        let resource_dir = crate::pl::tagger::pl_resource_dir().expect("pl resource dir");
        let xml = std::fs::read_to_string(resource_dir.join("disambiguation.xml")).expect("xml");
        let parsed = og_tagger::XmlDisambiguator::from_xml_with_english(
            &normalize_disambiguation_xml(&xml),
            None,
        )
        .expect("parse disambiguation.xml");
        let idx = parsed
            .rule_ids()
            .iter()
            .position(|id| *id == "K")
            .expect("K rule");
        let text = "Stopie\u{144} dysocjacji w temperaturze 4000 K wynosi nieca\u{142}e 3%.";
        let mut sentence = crate::pl::test_support::analyze_undisambiguated(text, 0, text.len());
        let k_lemmas = |sentence: &og_core::AnalyzedSentence| -> Vec<Option<String>> {
            sentence
                .non_whitespace_tokens()
                .iter()
                .find(|t| t.token().token() == "K")
                .expect("K token")
                .readings()
                .iter()
                .map(|r| r.lemma().map(str::to_string))
                .collect()
        };
        let before = k_lemmas(&sentence);
        assert!(
            before.iter().any(|l| l.as_deref() == Some("ko\u{142}o")),
            "the tagger offers ko\u{142}o readings for K: {before:?}"
        );
        parsed.apply_rule(&mut sentence, idx);
        let after = k_lemmas(&sentence);
        assert!(
            after.iter().all(|l| l.as_deref() != Some("ko\u{142}o")),
            "AnalyzedToken.matches (AnalyzedToken.java:91-111) treats a null pos as a \
             wildcard, so the lemma-only remove must drop every ko\u{142}o reading: {after:?}"
        );
        assert!(
            !after.is_empty(),
            "Java falls back to a bare reading when all readings are removed"
        );
    }

    #[test]
    fn java_pins_priority_map() {
        assert_eq!(priority_for_id("ZDANIA_ZLOZONE"), -1);
        assert_eq!(priority_for_id("TOO_LONG_SENTENCE"), -101);
        assert_eq!(priority_for_id("too_long_sentence"), -101);
        assert_eq!(priority_for_id("REPETITIONS_STYLE"), -55);
        assert_eq!(priority_for_id("SOMETHING_STYLE_CAT"), -50);
        assert_eq!(priority_for_id("UNKNOWN_RULE"), 0);
    }

    #[test]
    fn normalize_pl_rules_xml_rewrites_the_java_only_regex_constructs() {
        let xml =
            std::fs::read_to_string(pl_rules_dir().expect("pl rules dir").join("grammar.xml"))
                .expect("grammar.xml");
        let normalized = normalize_pl_rules_xml(&xml);
        assert_eq!(
            xml.matches("(?-iu)").count(),
            19,
            "the round-10 inventory of inline unicode-flag negations"
        );
        assert_eq!(xml.matches("\\02").count(), 1);
        assert!(normalized.contains("(?-i)w.*"));
        assert!(normalized.contains("\\x{2}"));
        assert!(!normalized.contains("(?-iu)"));
        assert!(!normalized.contains("\\02"));
        assert_eq!(normalized.matches("(?-i)").count(), 19);
        assert_eq!(
            normalized.matches("<example").count(),
            xml.matches("<example").count()
        );
        assert_eq!(
            normalized.matches("<rule").count(),
            xml.matches("<rule").count()
        );
    }

    #[test]
    fn normalize_pl_rules_xml_escapes_backslash_pairs_and_borrows_clean_input() {
        assert!(matches!(
            normalize_pl_rules_xml("<rules lang='pl'/>"),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(normalize_pl_rules_xml("a(?-iu)w.*b").as_ref(), "a(?-i)w.*b");
        assert_eq!(
            normalize_pl_rules_xml("[\\\\02]|(?-iu)Don").as_ref(),
            "[\\\\02]|(?-i)Don"
        );
        assert_eq!(normalize_pl_rules_xml("\\021x").as_ref(), "\\x{11}x");
        assert_eq!(normalize_pl_rules_xml("\\0").as_ref(), "\\0");
    }

    #[test]
    fn normalized_regexes_carry_java_semantics() {
        let punct = regex::Regex::new("^(?:[?\\.!,:;\u{2026}\u{201d}\"\\x{2}])$").unwrap();
        for hit in [".", "!", "?", ",", ":", ";", "\u{2026}", "\u{201d}", "\""] {
            assert!(
                punct.is_match(hit),
                "{hit:?} must hit the BRAK_KROPKI class"
            );
        }
        assert!(!punct.is_match("z\u{142}otych."));
        let day = regex::Regex::new("^(?:(?-i)w.*)$").unwrap();
        assert!(day.is_match("wielki"));
        assert!(!day.is_match("Wielki"));
        let exceptions = regex::RegexBuilder::new("^(?:minus|plus|lada|(?-i)Don|Los)$")
            .case_insensitive(true)
            .build()
            .unwrap();
        assert!(exceptions.is_match("MINUS"));
        assert!(exceptions.is_match("Don"));
        assert!(!exceptions.is_match("don"));
        assert!(!exceptions.is_match("los"));
    }

    #[test]
    fn java_pins_compound_rule_examples() {
        let rule = polish_compound_rule();
        let pipeline = crate::pl::test_support::PolishPipeline::shared();
        let text = "Witamy w Rabka Zdr\u{f3}j.";
        let sentence = pipeline.analyze_range(text, 0, text.len());
        let matches = og_core::rule::Rule::match_sentence(&rule, &sentence);
        assert!(
            matches
                .iter()
                .any(|m| &text[m.offset()..m.offset() + m.length()] == "Rabka Zdr\u{f3}j"),
            "the compounds.txt entry Rabka-Zdr\u{f3}j* fires on the spaced form: {matches:?}"
        );
        let text = "Witamy w \u{141}apu capu.";
        let sentence = pipeline.analyze_range(text, 0, text.len());
        let matches = og_core::rule::Rule::match_sentence(&rule, &sentence);
        assert!(
            matches
                .iter()
                .any(|m| &text[m.offset()..m.offset() + m.length()] == "\u{141}apu capu"),
            "the description's \u{141}apu-capu example fires: {matches:?}"
        );
        let text = "Witamy w Rabka-Zdr\u{f3}j.";
        let sentence = pipeline.analyze_range(text, 0, text.len());
        assert!(
            og_core::rule::Rule::match_sentence(&rule, &sentence).is_empty(),
            "the hyphenated form is accepted"
        );
    }

    #[test]
    fn java_pins_word_coherency_example() {
        let rule = polish_word_coherency_rule();
        let pipeline = crate::pl::test_support::PolishPipeline::shared();
        let text = "Kupi\u{142}em grapefruity, a grejpfrut\u{f3}w nie lubi\u{119}.";
        let sentences = pipeline.analyze_text(text);
        let matches = og_core::rule::TextLevelRule::match_text(&rule, text, &sentences);
        assert_eq!(
            matches.len(),
            1,
            "the Java example pair pins one coherency match"
        );
    }

    #[test]
    fn java_pins_unpaired_brackets_example() {
        let rule = pl_unpaired_brackets_rule();
        let pipeline = crate::pl::test_support::PolishPipeline::shared();
        let text = "To jest zdanie z \u{201e}cudzys\u{142}owem.";
        let sentences = pipeline.analyze_text(text);
        let matches = og_core::rule::TextLevelRule::match_text(&rule, text, &sentences);
        assert!(
            matches.len() >= 1,
            "the Polish quote symbols are tracked: {matches:?}"
        );
        let text = "To jest zdanie z \u{201e}cudzys\u{142}owem\u{201d}.";
        let sentences = pipeline.analyze_text(text);
        assert!(rule.match_text(text, &sentences).is_empty());
    }
}

#[cfg(test)]
mod dict_probe {
    use og_tagger::morfologik::MorfologikDictionary;

    fn polish_dict() -> MorfologikDictionary {
        let dir = super::tagger::pl_resource_dir().expect("pl resource dir");
        let dict = MorfologikDictionary::load(&dir.join("polish.dict"), &dir.join("polish.info"))
            .expect("polish.dict loads");
        dict
    }

    #[ignore]
    #[test]
    fn probe_oracle_divergence_sample() {
        let dir = super::tagger::pl_resource_dir().expect("pl resource dir");
        let hunspell = og_tagger::morfologik::load_fsa_word_list(
            &dir.join("hunspell/pl_PL.dict"),
            &dir.join("hunspell/pl_PL.info"),
        )
        .expect("pl_PL words");
        let tagger =
            og_tagger::generic_tagger::shared_dictionary(&dir, "polish").expect("polish dict");
        let mut missing: Vec<&String> = Vec::new();
        let mut checked = 0usize;
        for (i, w) in hunspell.iter().enumerate() {
            if i % 2267 != 0 {
                continue;
            }
            checked += 1;
            if tagger.lookup(w).is_empty() && !missing.contains(&w) {
                missing.push(w);
            }
        }
        eprintln!(
            "checked {checked} hunspell words; not in polish.dict: {}",
            missing.len()
        );
        for w in missing.iter().take(10) {
            eprintln!("  hunspell-only: {w}");
        }
    }

    #[test]
    fn probe_hunspell_words() {
        let dir = super::tagger::pl_resource_dir().expect("pl resource dir");
        let words = og_tagger::morfologik::load_fsa_word_list(
            &dir.join("hunspell/pl_PL.dict"),
            &dir.join("hunspell/pl_PL.info"),
        )
        .expect("pl_PL hunspell FSA");
        eprintln!("pl_PL hunspell word count: {}", words.len());
        let lookup = |w: &str| words.iter().any(|x| x == w);
        for w in [
            "bledem",
            "b\u{142}\u{119}dem",
            "kobieta",
            "warszawa",
            "nauk\u{119}",
            "ekstradycj\u{119}",
        ] {
            eprintln!("hunspell contains {w}: {}", lookup(w));
        }
        let with_prefix: Vec<&String> = words
            .iter()
            .filter(|w| w.starts_with("quasi") || w.starts_with("niby"))
            .take(10)
            .collect();
        eprintln!("quasi/niby words: {with_prefix:?}");
    }

    #[ignore]
    #[test]
    fn probe_speller_words() {
        let rule = crate::pl::speller::PolishSpellerRule::shared();
        for text in [
            "To jest zdanie z b\u{142}\u{119}dem.",
            "To jest bia\u{142}ozielony dach.",
            "To jest supernowoczesny sprz\u{119}t.",
            "On uprawia quasi-nauk\u{119}.",
        ] {
            let sentence = crate::pl::test_support::PolishPipeline::shared().analyze_range(
                text,
                0,
                text.len(),
            );
            let matches = og_core::rule::Rule::match_sentence(rule.as_ref(), &sentence);
            eprintln!("{text} -> {}", matches.len());
            for m in &matches {
                let rel = m.offset().saturating_sub(sentence.start());
                eprintln!(
                    "   word {:?} known={} offset {} len {}",
                    &sentence.text()[rel..rel + m.length()],
                    rule.is_known_word(&sentence.text()[rel..rel + m.length()]),
                    m.offset(),
                    m.length()
                );
            }
        }
    }

    #[ignore]
    #[test]
    fn probe_word_repeat_tokens() {
        let text =
            "M\u{f3}wi\u{142}a d\u{142}ugo, bo lubi\u{142}a robi\u{107} wszystko d\u{142}ugo.";
        let sentence =
            crate::pl::test_support::PolishPipeline::shared().analyze_range(text, 0, text.len());
        let mut inflected: std::collections::HashSet<String> = Default::default();
        for (i, t) in sentence.non_whitespace_tokens().iter().enumerate() {
            let _ = &mut inflected;
            let readings: Vec<String> = t
                .readings()
                .iter()
                .map(|r| format!("{}[{}]", r.lemma().unwrap_or("-"), r.pos_tags().join("+")))
                .collect();
            eprintln!("{i} {:?} => {}", t.token().token(), readings.join(", "));
        }
    }

    #[ignore]
    #[test]
    fn probe_coherency_matches() {
        let rule = crate::pl::polish_word_coherency_rule();
        let text = "Kupi\u{142}em grapefruity, a grejpfrut\u{f3}w nie lubi\u{119}.";
        let sentences = crate::pl::test_support::PolishPipeline::shared().analyze_text(text);
        let matches = og_core::rule::TextLevelRule::match_text(&rule, text, &sentences);
        eprintln!("coherency matches: {}", matches.len());
        for m in &matches {
            eprintln!(
                "  match off {} len {} msg {}",
                m.offset(),
                m.length(),
                m.message()
            );
        }
        let inline = crate::en::WordCoherencyRule::for_language("X", "Y", "grapefruit;grejpfrut\n");
        let matches_inline = og_core::rule::TextLevelRule::match_text(&inline, text, &sentences);
        eprintln!("inline data matches: {}", matches_inline.len());
        let rule2 = crate::pl::polish_compound_rule();
        for w in [
            "Rabka Zdr\u{f3}j",
            "Rabka-Zdr\u{f3}j",
            "\u{141}apu capu",
            "\u{141}apu-capu",
        ] {
            let sentence =
                crate::pl::test_support::PolishPipeline::shared().analyze_range(w, 0, w.len());
            eprintln!(
                "compound {w:?} -> {}",
                og_core::rule::Rule::match_sentence(&rule2, &sentence).len()
            );
        }
    }

    #[ignore]
    #[test]
    fn probe_coherency_tokens() {
        let text = "Kupi\u{142}em grapefruity, a grejpfrut\u{f3}w nie lubi\u{119}.";
        for (si, sentence) in crate::pl::test_support::PolishPipeline::shared()
            .analyze_text(text)
            .into_iter()
            .enumerate()
        {
            for t in sentence.non_whitespace_tokens() {
                let readings: Vec<String> = t
                    .readings()
                    .iter()
                    .map(|r| format!("{}[{}]", r.lemma().unwrap_or("-"), r.pos_tags().join("+")))
                    .collect();
                eprintln!("{si} {:?} => {}", t.token().token(), readings.join(", "));
            }
        }
    }

    #[test]
    fn probe_polish_readings() {
        let dict = polish_dict();
        for w in [
            "kobieta",
            "kobietą",
            "K",
            "k",
            "W",
            "w",
            "Były",
            "były",
            "rozum",
            "działanie",
            "najlepiej",
            "lepiej",
            "nielepiej",
            "białozielony",
            "polsko",
            "warszawa",
            "Warszawa",
            "niemiecko",
            "indonezyjski",
            "kobieta-wojownik",
            "Rabke",
            "Zdroju",
            "Rabke-Zdroju",
            "32-u",
            "64-bitowe",
            "XIX-go",
            "bledem",
            "ble",
            "dem",
            "b\u{142}\u{119}d",
            "b\u{142}\u{119}dem",
            "b\u{142}\u{119}du",
            "b\u{142}\u{119}dy",
            "edem",
            "bled",
            "quasi",
            "quasi-nauk\u{119}",
            "nauk\u{119}",
            "bia\u{142}ozielony",
            "bia\u{142}o",
            "zielony",
            "trzynastobitowy",
            "supernowoczesny",
            "ekstradycj\u{119}",
        ] {
            let readings: Vec<String> = dict
                .lookup(w)
                .iter()
                .map(|(l, t)| format!("{l}/{t}"))
                .collect();
            eprintln!("{w} => [{}]", readings.join(", "));
        }
    }
}
