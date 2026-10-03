use og_core::{AnalyzedTokenReadings, RuleMatch, SuggestedReplacement};
use regex::Regex;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

fn resolve_args<'a>(args: &'a str, matched_texts: &[String]) -> HashMap<&'a str, String> {
    let mut result = HashMap::new();
    for part in args.split_whitespace() {
        let Some((key, val)) = part.split_once(':') else {
            continue;
        };
        let resolved = if let Some(rest) = val.strip_prefix('\\')
            && let Ok(idx) = rest.parse::<usize>()
            && idx > 0
            && idx <= matched_texts.len()
        {
            matched_texts[idx - 1].clone()
        } else {
            val.to_string()
        };
        result.insert(key, resolved);
    }
    result
}

pub fn pattern_token_range<'a>(
    tokens: &'a [&'a AnalyzedTokenReadings],
    matched_positions: &[usize],
) -> &'a [&'a AnalyzedTokenReadings] {
    match (matched_positions.first(), matched_positions.last()) {
        (Some(&first), Some(&last)) if last < tokens.len() => &tokens[first..=last],
        _ => &[],
    }
}

fn is_all_uppercase(s: &str) -> bool {
    s.chars().all(|c| !c.is_alphabetic() || !c.is_lowercase())
}

fn is_capitalized_word(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {
            chars.all(|c| !c.is_alphabetic() || c.is_lowercase())
        }
        _ => false,
    }
}

fn uppercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn has_diacritics(s: &str) -> bool {
    s.chars().any(|c| c.is_alphabetic() && !c.is_ascii())
}

fn full_match(pattern: &str, value: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    let re = guard
        .entry(pattern.to_string())
        .or_insert_with(|| Regex::new(&format!("^(?:{pattern})$")).unwrap());
    re.is_match(value)
}

fn readings_match_pos_tag_regex(atr: &AnalyzedTokenReadings, pos_tag_regex: &str) -> bool {
    atr.readings()
        .iter()
        .flat_map(|r| r.pos_tags().iter())
        .any(|t| full_match(pos_tag_regex, t))
}

fn confusion_pairs() -> &'static HashMap<String, Vec<(String, String)>> {
    static PAIRS: OnceLock<HashMap<String, Vec<(String, String)>>> = OnceLock::new();
    PAIRS.get_or_init(|| {
        let mut map: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let Some(dir) = crate::es::es_rules_dir() else {
            return map;
        };
        let Ok(text) = std::fs::read_to_string(dir.join("confusion_pairs.txt")) else {
            return map;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split(';').collect();
            if parts.len() != 3 {
                continue;
            }
            map.entry(parts[0].to_string())
                .or_default()
                .push((parts[1].to_string(), parts[2].to_string()));
        }
        map
    })
}

pub fn confusion_check_filter(
    args: &str,
    matched_texts: &[String],
    pattern_tokens: &[&AnalyzedTokenReadings],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let parsed = resolve_args(args, matched_texts);
    let postag = parsed.get("postag")?;
    let original_form = parsed.get("form")?;
    let is_all_upper = is_all_uppercase(original_form);
    let is_capitalized = is_capitalized_word(original_form);
    let form = original_form.to_lowercase();
    let gendernumber_from = parsed
        .get("gendernumberFrom")
        .and_then(|v| v.parse::<usize>().ok());
    let desired: Option<&str> = gendernumber_from.and_then(|i| {
        let atr = pattern_tokens.get(i.checked_sub(1)?)?;
        if readings_match_pos_tag_regex(atr, "[NAPD].+MS.*|V.P..SM") {
            Some("NC[MC][SN]000|A..[MC][SN].|V.P..SM")
        } else if readings_match_pos_tag_regex(atr, "[NAPD].+MP.*|V.P..PM") {
            Some("NC[MC][PN]000|A..[MC][PN].|V.P..PM")
        } else if readings_match_pos_tag_regex(atr, "[NAPD].+FS.*|V.P..SF") {
            Some("NC[FC][SN]000|A..[FC][SN].|V.P..SF")
        } else if readings_match_pos_tag_regex(atr, "[NAPD].+FP.*|V.P..PF") {
            Some("NC[FC][PN]000|A..[FC][PN].|V.P..PF")
        } else if readings_match_pos_tag_regex(atr, "[NAPD].+CP.*|V.P..P.") {
            Some("NC[MFC][PN]000|A..[MFC][PN].|V.P..P.")
        } else if readings_match_pos_tag_regex(atr, "[NAPD].+CS.*|V.P..S.") {
            Some("NC[MFC][SN]000|A..[MFC][SN].|V.P..S.")
        } else {
            None
        }
    });

    let pairs = confusion_pairs();
    let mut replacement: Option<String> = None;
    if let Some(entries) = pairs.get(&form) {
        if entries.iter().any(|(_, tag)| full_match(postag, tag)) {
            if let Some(desired) = desired {
                if entries
                    .first()
                    .is_some_and(|(_, tag)| full_match(desired, tag))
                {
                    replacement = entries.first().map(|(word, _)| word.clone());
                }
            } else if gendernumber_from.is_none() {
                replacement = entries.first().map(|(word, _)| word.clone());
            }
        }
    }
    let mut replacement = replacement?;
    let mut message = rm.message().to_string();
    if !(has_diacritics(&replacement) && !has_diacritics(&form)) {
        message = message.replace("se escribe con tilde", "se escribe de otra manera");
    }
    rm.set_message(message);
    if is_all_upper {
        replacement = replacement.to_uppercase();
    }
    if is_capitalized {
        replacement = uppercase_first_char(&replacement);
    }
    let template = rm
        .replacements()
        .first()
        .map(|r| r.value().to_string())
        .unwrap_or_else(|| "{suggestion}".to_string());
    let suggestion = template
        .replace("{suggestion}", &replacement)
        .replace("{Suggestion}", &uppercase_first_char(&replacement))
        .replace("{SUGGESTION}", &replacement.to_uppercase());
    rm.set_replacements(vec![SuggestedReplacement::new(suggestion)]);
    Some(rm)
}

const OPENING_QUOTES: [char; 7] = ['«', '“', '"', '‘', '\'', '„', '¿'];

fn is_punctuation_or_symbol(token: &str) -> bool {
    token
        .chars()
        .all(|c| !c.is_alphanumeric() && !c.is_whitespace())
}

fn is_opening_quote(token: &str) -> bool {
    token.chars().count() == 1 && OPENING_QUOTES.contains(&token.chars().next().unwrap())
}

fn has_space_before(tokens: &[&AnalyzedTokenReadings], i: usize) -> bool {
    match i.checked_sub(1) {
        Some(prev) => tokens[prev].token().end() < tokens[i].token().start(),
        None => tokens.first().is_some_and(|t| t.token().start() > 0),
    }
}

pub fn add_commas_filter(
    tokens: &[&AnalyzedTokenReadings],
    sentence_base: usize,
    sentence_text: &str,
    mut rm: RuleMatch,
    args: &str,
) -> Option<RuleMatch> {
    let suggest_semicolon = args
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("suggestSemicolon:true"));
    let from = rm.offset();
    let to = rm.offset() + rm.length();
    let rel = |abs: usize| abs - sentence_base;
    let mut postag_from = 1usize;
    while postag_from < tokens.len() && tokens[postag_from].token().start() < from {
        postag_from += 1;
    }
    let mut postag_to = postag_from;
    while postag_to < tokens.len() && tokens[postag_to].token().end() < to {
        postag_to += 1;
    }
    let before_ok = postag_from == 1
        || is_punctuation_or_symbol(tokens[postag_from - 1].token().token())
        || is_capitalized_word(tokens[postag_from].token().token());
    let after_ok = tokens.get(postag_to + 1).is_some_and(|n| {
        is_punctuation_or_symbol(n.token().token())
            && !(has_space_before(tokens, postag_to + 1) && is_opening_quote(n.token().token()))
    });
    if before_ok && after_ok {
        return None;
    }
    let matched = &sentence_text[rel(from)..rel(to)];
    if suggest_semicolon
        && postag_from >= 1
        && postag_from - 1 < tokens.len()
        && tokens[postag_from - 1].token().token() == ","
        && !after_ok
    {
        let start = tokens[postag_from - 1].token().start();
        let end = tokens[postag_to].token().end();
        rm.set_offset_length(start, end - start);
        rm.set_replacements(vec![
            SuggestedReplacement::new(format!("; {matched},")),
            SuggestedReplacement::new(format!(", {matched},")),
        ]);
        return Some(rm);
    }
    if before_ok && !after_ok {
        let start = tokens[postag_to].token().start();
        rm.set_offset_length(start, to - start);
        rm.set_replacements(vec![SuggestedReplacement::new(format!(
            "{},",
            tokens[postag_to].token().token()
        ))]);
        return Some(rm);
    }
    if !before_ok && after_ok {
        let mut start = tokens[postag_from].token().start();
        if has_space_before(tokens, postag_from) {
            start = start.saturating_sub(1);
        }
        let end = tokens[postag_from].token().end();
        rm.set_offset_length(start, end - start);
        rm.set_replacements(vec![SuggestedReplacement::new(format!(
            ", {}",
            tokens[postag_from].token().token()
        ))]);
        return Some(rm);
    }
    let mut start = tokens[postag_from].token().start();
    if has_space_before(tokens, postag_from) {
        start = start.saturating_sub(1);
    }
    let end = tokens[postag_to].token().end();
    rm.set_offset_length(start, end - start);
    rm.set_replacements(vec![SuggestedReplacement::new(format!(", {matched},"))]);
    Some(rm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::es::test_support::SpanishPipeline;
    use og_rules::pattern_rule::PatternRuleEngine;
    use og_xml::compiler::XmlCompiler;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    fn compiled_rules() -> &'static og_xml::compiler::CompiledRuleSet {
        static RULES: OnceLock<og_xml::compiler::CompiledRuleSet> = OnceLock::new();
        RULES.get_or_init(|| {
            let path = PathBuf::from(
                "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/es/src/main/resources/org/languagetool/rules/es/grammar.xml",
            );
            let xml = std::fs::read_to_string(&path).unwrap();
            XmlCompiler::new()
                .compile_file_with_base(&xml, path.parent())
                .expect("compile grammar.xml")
        })
    }

    fn atr(word: &str, tags: &[(&str, &str)]) -> AnalyzedTokenReadings {
        let mut out = AnalyzedTokenReadings::new(og_core::AnalyzedToken::new(word, 0, word.len()));
        let readings: Vec<og_core::AnalyzedToken> = tags
            .iter()
            .map(|(lemma, tag)| {
                og_core::AnalyzedToken::new(word, 0, word.len())
                    .with_lemma(*lemma)
                    .with_pos_tags(vec![tag.to_string()])
            })
            .collect();
        *out.readings_mut() = readings;
        out
    }

    fn base_rm(message: &str, offset: usize, length: usize, template: &str) -> RuleMatch {
        RuleMatch::new(
            message,
            offset,
            length,
            og_core::RuleMatchRule::new("X", "d"),
            og_core::RuleMatchContext::new("", 0, 0),
        )
        .with_replacements(vec![SuggestedReplacement::new(template)])
    }

    #[test]
    fn confusion_check_ports_mydia02_shape() {
        let base = atr("Maquina", &[("maquinar", "VMIP3S0"), ("maca", "NCFS00D")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&base];
        let matched = vec!["Maquina".to_string()];
        let rm = base_rm(
            "Si es adjetivo o nombre, se escribe con tilde.",
            0,
            7,
            "{suggestion}",
        );
        let out = confusion_check_filter("form:\\1 postag:[AN].*", &matched, &tokens, rm)
            .expect("maquina -> máquina");
        assert_eq!(out.replacements()[0].value(), "Máquina");
        assert_eq!(
            out.message(),
            "Si es adjetivo o nombre, se escribe con tilde."
        );
    }

    #[test]
    fn confusion_check_rejects_wrong_gender() {
        let base = atr("practicas", &[("x", "AQ0MP0")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&base];
        let matched = vec!["practicas".to_string()];
        let rm = base_rm("m", 0, 9, "{suggestion}");
        assert!(pairs_contain("practicas", "prácticas", "NCFP000"));
        let out = confusion_check_filter(
            "form:\\1 postag:[AN].* gendernumberFrom:1",
            &matched,
            &tokens,
            rm,
        );
        assert!(
            out.is_none(),
            "first reading NCFP000 must not match MS pattern"
        );
    }

    fn pairs_contain(form: &str, word: &str, tag: &str) -> bool {
        confusion_pairs()
            .get(form)
            .is_some_and(|e| e.contains(&(word.to_string(), tag.to_string())))
    }

    #[test]
    fn confusion_check_rewrites_message_without_diacritic_gain() {
        let base = atr("ralla", &[("rallar", "VMIP3S0")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&base];
        let matched = vec!["ralla".to_string()];
        let rm = base_rm("m se escribe con tilde x", 0, 5, "{suggestion}");
        let out = confusion_check_filter("form:\\1 postag:[AN].*", &matched, &tokens, rm)
            .expect("ralla -> raya");
        assert_eq!(out.message(), "m se escribe de otra manera x");
        assert_eq!(out.replacements()[0].value(), "raya");
    }

    #[test]
    fn add_commas_ports_marker_examples() {
        let pipeline = SpanishPipeline::shared();
        let engine = PatternRuleEngine::new();
        let rule_set = compiled_rules();
        for (text, expect_start, expect_len, expect_first) in [
            (
                "Es por lo tanto lo que hay que hacer.",
                2usize,
                13usize,
                ", por lo tanto,",
            ),
            ("Y por otro lado no logró ganar.", 1, 14, ", por otro lado,"),
        ] {
            let sentence = &pipeline.analyze_text(text)[0];
            let tokens: Vec<&og_core::AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
            let mut best: Option<(usize, usize, String)> = None;
            for rule in rule_set
                .rules
                .iter()
                .filter(|r| r.id == "COMMA_MARCADOR_DISCURSIVO")
            {
                for raw in engine.match_rule(rule, sentence) {
                    if let Some(out) = add_commas_filter(
                        &tokens,
                        sentence.start(),
                        sentence.text(),
                        raw,
                        "none:none",
                    ) {
                        let candidate = (
                            out.offset(),
                            out.length(),
                            out.replacements()[0].value().to_string(),
                        );
                        if candidate.2 == expect_first {
                            best = Some(candidate);
                        }
                    }
                }
            }
            let (start, len, first) =
                best.unwrap_or_else(|| panic!("no repositioned match for {text}"));
            assert_eq!((start, len), (expect_start, expect_len), "{text}");
            assert_eq!(first, expect_first, "{text}");
        }
    }

    #[test]
    fn add_commas_ports_semicolon_variant() {
        let pipeline = SpanishPipeline::shared();
        let engine = PatternRuleEngine::new();
        let rule_set = compiled_rules();
        let text = "Me gusta mucho este, sin embargo prefiero aquel.";
        let rule = rule_set
            .rules
            .iter()
            .filter(|r| r.id == "COMMA_MARCADOR_DISCURSIVO")
            .find(|r| {
                r.filter
                    .as_ref()
                    .is_some_and(|f| f.args.contains("suggestSemicolon"))
            })
            .expect("semicolon subrule");
        let sentence = &pipeline.analyze_text(text)[0];
        let tokens: Vec<&og_core::AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
        let raw = engine
            .match_rule(rule, sentence)
            .into_iter()
            .next()
            .expect("raw match");
        let out = add_commas_filter(
            &tokens,
            sentence.start(),
            sentence.text(),
            raw,
            "suggestSemicolon:true",
        )
        .expect("repositioned match");
        assert_eq!((out.offset(), out.length()), (19, 13));
        assert_eq!(out.replacements()[0].value(), "; sin embargo,");
        assert_eq!(out.replacements()[1].value(), ", sin embargo,");
    }
}
