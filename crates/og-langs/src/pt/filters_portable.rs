use og_core::{AnalyzedTokenReadings, RuleMatch, SuggestedReplacement};
use og_rules::postponed_adjective::LangSynthesizer;
use og_xml::compiler::CompiledRule;
use og_xml::types::XmlFilter;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub fn resolve_args<'a>(args: &'a str, matched_texts: &[String]) -> HashMap<&'a str, String> {
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

pub fn raw_arg<'a>(args: &'a str, key: &str) -> Option<&'a str> {
    args.split_whitespace()
        .find_map(|part| part.strip_prefix(key)?.strip_prefix(':'))
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

pub fn is_all_uppercase(s: &str) -> bool {
    s.chars().all(|c| !c.is_alphabetic() || !c.is_lowercase())
}

pub fn is_capitalized_word(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {
            chars.all(|c| !c.is_alphabetic() || !c.is_lowercase())
        }
        _ => false,
    }
}

pub fn uppercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn has_diacritics(s: &str) -> bool {
    s.chars().any(|c| c.is_alphabetic() && !c.is_ascii())
}

pub fn full_match(pattern: &str, value: &str) -> bool {
    full_matcher(pattern).is_match(value)
}

pub fn full_matcher(pattern: &str) -> Regex {
    static CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    regex::Regex::clone(
        guard
            .entry(pattern.to_string())
            .or_insert_with(|| Regex::new(&format!("^(?:{pattern})$")).unwrap()),
    )
}

pub fn readings_match_pos_tag_regex(atr: &AnalyzedTokenReadings, pos_tag_regex: &str) -> bool {
    atr.readings()
        .iter()
        .flat_map(|r| r.pos_tags().iter())
        .any(|t| full_match(pos_tag_regex, t))
}

pub fn lt_modules_root() -> Option<PathBuf> {
    [
        PathBuf::from("languagetool/languagetool-language-modules"),
        PathBuf::from("../languagetool/languagetool-language-modules"),
        PathBuf::from("../languagetool-language-modules"),
        PathBuf::from("../../languagetool-language-modules"),
        PathBuf::from("/home/agent/opengrammar-rs/languagetool/languagetool-language-modules"),
    ]
    .into_iter()
    .find(|p| p.exists())
    .and_then(|p| p.canonicalize().ok())
}

pub fn pt_rules_dir() -> Option<PathBuf> {
    lt_modules_root().map(|root| root.join("pt/src/main/resources/org/languagetool/rules/pt"))
}

pub fn pt_resource_dir() -> Option<PathBuf> {
    lt_modules_root().map(|root| root.join("pt/src/main/resources/org/languagetool/resource/pt"))
}

pub struct ConfusionEntry {
    pub word: String,
    pub tags: Vec<String>,
}

pub fn confusion_pairs(brazilian: bool) -> &'static HashMap<String, ConfusionEntry> {
    static PT: OnceLock<HashMap<String, ConfusionEntry>> = OnceLock::new();
    static BR: OnceLock<HashMap<String, ConfusionEntry>> = OnceLock::new();
    let load = |variant_file: &str| {
        let mut map: HashMap<String, ConfusionEntry> = HashMap::new();
        let Some(dir) = pt_rules_dir() else {
            return map;
        };
        for name in ["confusion_pairs.txt", variant_file] {
            let Ok(text) = std::fs::read_to_string(dir.join(name)) else {
                continue;
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
                match map.get_mut(parts[0]) {
                    Some(entry) => entry.tags.push(parts[2].to_string()),
                    None => {
                        map.insert(
                            parts[0].to_string(),
                            ConfusionEntry {
                                word: parts[1].to_string(),
                                tags: vec![parts[2].to_string()],
                            },
                        );
                    }
                }
            }
        }
        map
    };
    if brazilian {
        BR.get_or_init(|| load("pt-BR/confusion_pairs.txt"))
    } else {
        PT.get_or_init(|| load("pt-PT/confusion_pairs.txt"))
    }
}

pub fn confusion_check_filter(
    variant: &str,
    args: &str,
    matched_texts: &[String],
    pattern_tokens: &[&AnalyzedTokenReadings],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let parsed = resolve_args(args, matched_texts);
    let postag = parsed.get("postag")?;
    let form = parsed.get("form")?.to_lowercase();
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

    let relevant_words = confusion_pairs(variant == "pt-BR");
    let mut replacement: Option<&str> = None;
    if let Some(entry) = relevant_words.get(&form) {
        if entry.tags.iter().any(|t| full_match(postag, t)) {
            if let Some(desired) = desired {
                if entry.tags.first().is_some_and(|t| full_match(desired, t)) {
                    replacement = Some(entry.word.as_str());
                }
            } else if gendernumber_from.is_none() {
                replacement = Some(entry.word.as_str());
            }
        }
    }
    let replacement = replacement?.to_string();
    let mut message = rm.message().to_string();
    if !(has_diacritics(&replacement) && !has_diacritics(&form)) {
        message = message.replace("se escribe con tilde", "se escribe de otra manera");
    }
    rm.set_message(message);
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

pub const BRAZILIAN_STATES: [&str; 27] = [
    "AC", "AL", "AP", "AM", "BA", "CE", "DF", "ES", "GO", "MA", "MT", "MS", "MG", "PA", "PB", "PR",
    "PE", "PI", "RJ", "RN", "RS", "RO", "RR", "SC", "SP", "SE", "TO",
];

pub fn toponyms() -> &'static HashSet<String> {
    static TOPONYMS: OnceLock<HashSet<String>> = OnceLock::new();
    TOPONYMS.get_or_init(|| {
        let mut set = HashSet::new();
        let Some(resource_dir) = pt_resource_dir() else {
            return set;
        };
        let dir = resource_dir.join("brazilian_municipalities");
        for state in BRAZILIAN_STATES {
            let Ok(text) = std::fs::read_to_string(dir.join(format!("{state}.tsv"))) else {
                continue;
            };
            for line in text.lines() {
                set.insert(line.replace('-', " ").to_lowercase());
            }
        }
        set
    })
}

pub fn is_valid_toponym(toponym: &str) -> bool {
    let normalised = toponym.replace('-', " ").to_lowercase();
    let parts: Vec<&str> = normalised.split(' ').collect();
    let all = toponyms();
    (0..parts.len()).any(|i| all.contains(&parts[i..].join(" ")))
}

pub fn brazilian_toponym_filter(groups: &[&str], mut rm: RuleMatch) -> Option<RuleMatch> {
    let toponym = groups.get(1).copied().unwrap_or_default();
    let underlined = groups.get(2).copied().unwrap_or_default();
    let state = groups.get(3).copied().unwrap_or_default();
    let suggestion = format!("\u{2013}{state}");
    if suggestion == underlined {
        return None;
    }
    if !is_valid_toponym(toponym) {
        return None;
    }
    rm.set_replacements(vec![SuggestedReplacement::new(suggestion)]);
    Some(rm)
}

pub fn is_regular_participle(p: &str) -> bool {
    let lp = p.to_lowercase();
    ["do", "dos", "da", "das"]
        .iter()
        .any(|end| lp.ends_with(end))
}

pub fn regular_irregular_participle_filter(
    synthesizer: Option<&LangSynthesizer>,
    args: &str,
    tokens: &[&AnalyzedTokenReadings],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let direction = raw_arg(args, "direction")?;
    let atr = tokens
        .iter()
        .copied()
        .find(|t| t.token().start() == rm.offset())?;
    if !atr
        .readings()
        .iter()
        .flat_map(|r| r.pos_tags().iter())
        .any(|t| t.starts_with("VMP"))
    {
        return None;
    }
    let mut selected: Option<&og_core::AnalyzedToken> = None;
    let mut desired_postag = String::new();
    for reading in atr.readings() {
        if let Some(tag) = reading.pos_tags().first()
            && tag.starts_with("VMP")
        {
            selected = Some(reading);
            desired_postag = tag.clone();
        }
    }
    let selected = selected?;
    let Some(last) = desired_postag.pop() else {
        return None;
    };
    if last == 'C' {
        desired_postag.push_str("[MC]");
    } else {
        desired_postag.push_str(&format!("[{last}C]"));
    }
    let synthesizer = synthesizer?;
    let lemma = selected.lemma().unwrap_or(selected.token());
    let participles = synthesizer.synthesize_full_for(lemma, None, &desired_postag);
    if participles.len() < 2 {
        return None;
    }
    let token_text = atr.token().token();
    let mut replacement: Option<String> = None;
    if direction.eq_ignore_ascii_case("RegularToIrregular") && is_regular_participle(token_text) {
        if !is_regular_participle(&participles[0]) {
            replacement = Some(participles[0].clone());
        } else if !is_regular_participle(&participles[1]) {
            replacement = Some(participles[1].clone());
        }
    } else if direction.eq_ignore_ascii_case("IrregularToRegular")
        && !is_regular_participle(token_text)
    {
        if is_regular_participle(&participles[0]) {
            replacement = Some(participles[0].clone());
        } else if is_regular_participle(&participles[1]) {
            replacement = Some(participles[1].clone());
        }
    }
    let replacement = replacement?;
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

pub fn soros_digit_shape(digit: u8, charset: [&str; 3]) -> String {
    let [c1, c2, c3] = charset;
    match digit {
        1 => c1.to_string(),
        2 => format!("{c1}{c1}"),
        3 => format!("{c1}{c1}{c1}"),
        4 => format!("{c1}{c2}"),
        5 => c2.to_string(),
        6 => format!("{c2}{c1}"),
        7 => format!("{c2}{c1}{c1}"),
        8 => format!("{c2}{c1}{c1}{c1}"),
        9 => format!("{c1}{c3}"),
        _ => String::new(),
    }
}

pub fn soros_overline(digits: &str) -> String {
    let mut out = String::new();
    for c in digits.chars() {
        out.push(c);
        out.push('\u{0305}');
    }
    out
}

pub fn soros_roman(input: &str) -> String {
    if input.is_empty() || !input.bytes().all(|b| b.is_ascii_digit()) {
        return String::new();
    }
    let digits = input.as_bytes();
    let Some(first) = digits.iter().position(|b| *b != b'0') else {
        return String::new();
    };
    run_digits(&digits[first..])
}

pub fn run_digits(digits: &[u8]) -> String {
    let n = digits.len();
    let d0 = digits[0] - b'0';
    match n {
        1 => soros_digit_shape(d0, ["I", "V", "X"]),
        2 => format!(
            "{}{}",
            soros_digit_shape(d0, ["X", "L", "C"]),
            run_digits(&digits[1..])
        ),
        3 => format!(
            "{}{}",
            soros_digit_shape(d0, ["C", "D", "M"]),
            run_digits(&digits[1..])
        ),
        4 if (1..=3).contains(&d0) => format!(
            "{}{}",
            soros_digit_shape(d0, ["M", "-", "-"]),
            run_digits(&digits[1..])
        ),
        7 if (1..=3).contains(&d0) => format!(
            "{}{}",
            soros_overline(&digits[..4].iter().map(|b| *b as char).collect::<String>()),
            run_digits(&digits[4..])
        ),
        _ => format!(
            "{}{}",
            soros_overline(
                &digits[..n - 3]
                    .iter()
                    .map(|b| *b as char)
                    .collect::<String>()
            ),
            run_digits(&digits[n - 3..])
        ),
    }
}

pub fn roman_numeral_filter(
    args: &str,
    matched_texts: &[String],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let parsed = resolve_args(args, matched_texts);
    let arabic_source = parsed.get("arabicSource").cloned().unwrap_or_default();
    rm.set_replacements(vec![SuggestedReplacement::new(soros_roman(&arabic_source))]);
    Some(rm)
}

pub fn convert_pronoun_to_accusative(pronoun_tag: &str) -> String {
    if pronoun_tag.ends_with("N00") {
        format!("{}A00", &pronoun_tag[..pronoun_tag.len() - 3])
    } else {
        pronoun_tag.to_string()
    }
}

pub fn pronoun_tags(
    pronoun_readings: &AnalyzedTokenReadings,
    verb_text: &str,
    convert_to_accusative: bool,
) -> Vec<String> {
    let mut tags = Vec::new();
    for reading in pronoun_readings.readings() {
        if reading.token() == "nos" {
            tags.push("PP1CPO00".to_string());
            if verb_text.ends_with('m') || verb_text.ends_with("ão") || verb_text.ends_with("õe")
            {
                tags.push("PP3MPA00".to_string());
            }
            break;
        }
        if let Some(tag) = reading.pos_tags().first()
            && tag.starts_with("PP")
        {
            let tag = if convert_to_accusative {
                convert_pronoun_to_accusative(tag)
            } else {
                tag.clone()
            };
            tags.push(tag);
        }
    }
    tags
}

pub fn portuguese_enclisis_filter(
    synthesizer: Option<&LangSynthesizer>,
    args: &str,
    tokens: &[&AnalyzedTokenReadings],
    matched_positions: &[usize],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let verb_pos = raw_arg(args, "verbPos").and_then(|v| v.parse::<usize>().ok())?;
    let pronoun_pos = raw_arg(args, "pronounPos").and_then(|v| v.parse::<usize>().ok())?;
    let convert_to_accusative =
        raw_arg(args, "convertToAccusative").is_some_and(|v| v.eq_ignore_ascii_case("true"));
    let window = pattern_token_range(tokens, matched_positions);
    let verb = window.get(verb_pos).copied()?;
    let pronoun = window.get(pronoun_pos).copied()?;
    let tags = pronoun_tags(pronoun, verb.token().token(), convert_to_accusative);
    if tags.is_empty() {
        return None;
    }
    let mut suggestions: Vec<String> = Vec::new();
    if let Some(synthesizer) = synthesizer {
        let verb_text = verb.token().token();
        let title_case = is_capitalized_word(verb_text);
        let all_caps = is_all_uppercase(verb_text);
        for reading in verb.readings() {
            let Some(pos_tag) = reading.pos_tags().first() else {
                continue;
            };
            if !pos_tag.starts_with('V') {
                continue;
            }
            for pronoun_tag in &tags {
                let enclisis_tag = format!("{pos_tag}:{pronoun_tag}");
                let lemma = reading.lemma().unwrap_or(reading.token());
                for form in synthesizer.synthesize_exact_for(lemma, None, &enclisis_tag) {
                    let form = if title_case {
                        uppercase_first_char(&form)
                    } else if all_caps {
                        form.to_uppercase()
                    } else {
                        form
                    };
                    if !suggestions.contains(&form) {
                        suggestions.push(form);
                    }
                }
            }
            break;
        }
    }
    rm.set_replacements(
        suggestions
            .into_iter()
            .map(SuggestedReplacement::new)
            .collect(),
    );
    Some(rm)
}

pub fn proclisis_pronoun_forms(old_pronoun: &str, old_verb: &str) -> Vec<String> {
    match old_pronoun {
        "lo" | "no" => vec!["o".to_string()],
        "la" | "na" => vec!["a".to_string()],
        "los" => vec!["os".to_string()],
        "las" | "nas" => vec!["as".to_string()],
        "nos" => {
            let mut forms = vec!["nos".to_string()];
            if old_verb.ends_with('m') || old_verb.ends_with("ão") || old_verb.ends_with("õe") {
                forms.push("os".to_string());
            }
            forms
        }
        other => vec![other.to_string()],
    }
}

pub fn portuguese_proclisis_filter(
    synthesizer: Option<&LangSynthesizer>,
    tokens: &[&AnalyzedTokenReadings],
    matched_positions: &[usize],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let window = pattern_token_range(tokens, matched_positions);
    let enclitic = window.last().copied()?;
    let mut suggestions: Vec<String> = Vec::new();
    if let Some(synthesizer) = synthesizer {
        for reading in enclitic.readings() {
            let Some(pos_tag) = reading.pos_tags().first() else {
                continue;
            };
            if !pos_tag.starts_with('V') || !pos_tag.contains(':') {
                continue;
            }
            let verb_tag = pos_tag.split(':').next().unwrap_or_default();
            let lemma = reading.lemma().unwrap_or(reading.token());
            let Some(new_verb) = synthesizer
                .synthesize_exact_for(lemma, None, verb_tag)
                .into_iter()
                .next()
            else {
                continue;
            };
            let mut parts = reading.token().split('-');
            let (Some(old_verb), Some(old_pronoun)) = (parts.next(), parts.next()) else {
                continue;
            };
            for new_pronoun in proclisis_pronoun_forms(old_pronoun, old_verb) {
                let suggestion = format!("{new_pronoun} {new_verb}");
                if !suggestions.contains(&suggestion) {
                    suggestions.push(suggestion);
                }
            }
        }
    }
    rm.set_replacements(
        suggestions
            .into_iter()
            .map(SuggestedReplacement::new)
            .collect(),
    );
    Some(rm)
}

pub fn marker_or_index(
    spec: &str,
    pattern_tokens: &[&AnalyzedTokenReadings],
    rm_from: usize,
) -> Option<usize> {
    if let Some(suffix) = spec.strip_prefix("marker") {
        let mut i = 0usize;
        while i < pattern_tokens.len() && pattern_tokens[i].token().start() < rm_from {
            i += 1;
        }
        i += 1;
        let extra: usize = suffix.parse().unwrap_or(0);
        let one_based = i + extra;
        if one_based < 1 || one_based > pattern_tokens.len() {
            return None;
        }
        Some(one_based - 1)
    } else {
        let n: usize = spec.parse().ok()?;
        if n < 1 || n > pattern_tokens.len() {
            return None;
        }
        Some(n - 1)
    }
}

pub fn select_reading<'a>(
    atr: &'a AnalyzedTokenReadings,
    select: &str,
) -> (Option<&'a str>, &'a str) {
    const UNKNOWN: &str = "UNKNOWN";
    for reading in atr.readings() {
        for tag in reading.pos_tags() {
            if full_match(select, tag) {
                return (reading.lemma(), tag);
            }
        }
    }
    match atr.readings().first() {
        Some(reading) => (
            reading.lemma(),
            reading
                .pos_tags()
                .first()
                .map(String::as_str)
                .unwrap_or(UNKNOWN),
        ),
        None => (None, UNKNOWN),
    }
}

pub fn composite_postag(
    lemma_select: &str,
    postag_select: &str,
    original_postag: &str,
    desired_postag: &str,
    postag_replace: &str,
) -> String {
    let a = full_matcher(lemma_select).captures(original_postag);
    let b = full_matcher(postag_select).captures(desired_postag);
    let mut result = postag_replace.to_string();
    if let (Some(a), Some(b)) = (a, b) {
        for i in 1..=a.len() {
            if let Some(g) = a.get(i) {
                result = result.replace(&format!(r"\a{i}"), g.as_str());
            }
        }
        for i in 1..=b.len() {
            if let Some(g) = b.get(i) {
                result = result.replace(&format!(r"\b{i}"), g.as_str());
            }
        }
    }
    result
}

pub fn move_pronoun_tag(source_tag: &str, destination_tag: &str) -> String {
    let mut parts: Vec<&str> = source_tag.split(':').collect();
    while parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    if parts.len() == 2 {
        format!(
            "{}:{}",
            destination_tag.split(':').next().unwrap_or_default(),
            parts[1]
        )
    } else {
        destination_tag.to_string()
    }
}

pub fn is_suggestion_exception_pt(token: &str, desired_postag: &str) -> bool {
    (desired_postag == "VMIP1P0" || desired_postag == "VMIP2P0") && !token.ends_with('s')
}

pub fn advanced_synthesizer_filter(
    synthesizer: Option<&LangSynthesizer>,
    args: &str,
    matched_texts: &[String],
    matched_positions: &[usize],
    tokens: &[&AnalyzedTokenReadings],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let pattern_tokens = pattern_token_range(tokens, matched_positions);
    if pattern_tokens.is_empty() {
        return Some(rm);
    }
    let parsed = resolve_args(args, matched_texts);
    let (Some(postag_select), Some(lemma_select), Some(postag_from_str), Some(lemma_from_str)) = (
        parsed.get("postagSelect"),
        parsed.get("lemmaSelect"),
        parsed.get("postagFrom"),
        parsed.get("lemmaFrom"),
    ) else {
        return Some(rm);
    };
    let new_lemma = parsed.get("newLemma").cloned().unwrap_or_default();
    let postag_replace = parsed.get("postagReplace").cloned();
    let lemma_from = marker_or_index(lemma_from_str, pattern_tokens, rm.offset())?;
    let postag_from = marker_or_index(postag_from_str, pattern_tokens, rm.offset())?;
    let (lemma, original_postag) = {
        let (l, t) = select_reading(pattern_tokens[lemma_from], lemma_select);
        (l.map(str::to_string), t.to_string())
    };
    let desired_postag = select_reading(pattern_tokens[postag_from], postag_select)
        .1
        .to_string();
    let mut desired_lemma = lemma;
    if !new_lemma.is_empty() {
        if new_lemma.starts_with('_') {
            desired_lemma = None;
        } else {
            desired_lemma = Some(new_lemma);
        }
    }
    let desired_lemma = desired_lemma?;
    let desired_postag = match postag_replace.as_deref() {
        Some("keepPronoun") => move_pronoun_tag(&original_postag, &desired_postag),
        Some(replace) => composite_postag(
            lemma_select,
            postag_select,
            &original_postag,
            &desired_postag,
            replace,
        ),
        None => desired_postag,
    };
    let surface = pattern_tokens[lemma_from].token().token();
    let is_word_capitalized = is_capitalized_word(surface);
    let is_word_all_upper = is_all_uppercase(surface);
    let Some(synthesizer) = synthesizer else {
        return Some(rm);
    };
    let tag_regex = match Regex::new(&desired_postag) {
        Ok(re) => re,
        Err(_) => return Some(rm),
    };
    let replacements = synthesizer.synthesize(&desired_lemma, &tag_regex);
    if replacements.is_empty() {
        return Some(rm);
    }
    let mut list: Vec<String> = Vec::new();
    let mut suggestion_used = false;
    for r in rm.replacements().iter().map(|r| r.value().to_string()) {
        for nr in &replacements {
            if is_suggestion_exception_pt(nr, &desired_postag) {
                continue;
            }
            if r.contains("{suggestion}")
                || r.contains("{Suggestion}")
                || r.contains("{SUGGESTION}")
            {
                suggestion_used = true;
            }
            let mut nr = nr.clone();
            if is_word_capitalized {
                nr = uppercase_first_char(&nr);
            }
            if is_word_all_upper {
                nr = nr.to_uppercase();
            }
            let complete = r
                .replace("{suggestion}", &nr)
                .replace("{Suggestion}", &uppercase_first_char(&nr))
                .replace("{SUGGESTION}", &nr.to_uppercase());
            if !list.contains(&complete) {
                list.push(complete);
            }
        }
    }
    if !suggestion_used {
        list.extend(replacements.iter().cloned());
    }
    rm.set_replacements(list.into_iter().map(SuggestedReplacement::new).collect());
    Some(rm)
}

pub fn is_pt_filter(class: &str) -> bool {
    matches!(
        class,
        "org.languagetool.rules.pt.ConfusionCheckFilter"
            | "org.languagetool.rules.pt.BrazilianToponymFilter"
            | "org.languagetool.rules.pt.RegularIrregularParticipleFilter"
            | "org.languagetool.rules.pt.RomanNumeralFilter"
            | "org.languagetool.rules.pt.PortugueseEnclisisFilter"
            | "org.languagetool.rules.pt.PortugueseProclisisFilter"
            | "org.languagetool.rules.pt.AdvancedSynthesizerFilter"
    )
}

pub struct PtFilterCall<'a> {
    pub variant: &'a str,
    pub args: &'a str,
    pub matched_texts: &'a [String],
    pub matched_positions: &'a [usize],
    pub tokens: &'a [&'a AnalyzedTokenReadings],
    pub regexp_groups: Option<&'a [&'a str]>,
    pub synthesizer: Option<&'a LangSynthesizer>,
}

pub fn apply_pt_filter(class: &str, call: PtFilterCall<'_>, rm: RuleMatch) -> Option<RuleMatch> {
    match class {
        "org.languagetool.rules.pt.ConfusionCheckFilter" => confusion_check_filter(
            call.variant,
            call.args,
            call.matched_texts,
            pattern_token_range(call.tokens, call.matched_positions),
            rm,
        ),
        "org.languagetool.rules.pt.BrazilianToponymFilter" => {
            brazilian_toponym_filter(call.regexp_groups?, rm)
        }
        "org.languagetool.rules.pt.RegularIrregularParticipleFilter" => {
            regular_irregular_participle_filter(call.synthesizer, call.args, call.tokens, rm)
        }
        "org.languagetool.rules.pt.RomanNumeralFilter" => {
            roman_numeral_filter(call.args, call.matched_texts, rm)
        }
        "org.languagetool.rules.pt.PortugueseEnclisisFilter" => portuguese_enclisis_filter(
            call.synthesizer,
            call.args,
            call.tokens,
            call.matched_positions,
            rm,
        ),
        "org.languagetool.rules.pt.PortugueseProclisisFilter" => {
            portuguese_proclisis_filter(call.synthesizer, call.tokens, call.matched_positions, rm)
        }
        "org.languagetool.rules.pt.AdvancedSynthesizerFilter" => advanced_synthesizer_filter(
            call.synthesizer,
            call.args,
            call.matched_texts,
            call.matched_positions,
            call.tokens,
            rm,
        ),
        _ => None,
    }
}

pub fn regexp_group_texts(
    regexp: &og_xml::compiler::CompiledRuleRegexp,
    text: &str,
    offset: usize,
    length: usize,
) -> Option<Vec<String>> {
    let end = offset + length;
    let captured: Vec<Vec<Option<(usize, usize)>>> = if let Some(ref re) = regexp.regex {
        re.captures_iter(text)
            .map(|caps| {
                (0..caps.len())
                    .map(|i| caps.get(i).map(|m| (m.start(), m.end())))
                    .collect()
            })
            .collect()
    } else if let Some(ref re) = regexp.fancy_regex {
        re.captures_iter(text)
            .flatten()
            .map(|caps| {
                (0..caps.len())
                    .map(|i| caps.get(i).map(|m| (m.start(), m.end())))
                    .collect()
            })
            .collect()
    } else {
        return None;
    };
    captured
        .into_iter()
        .find(|groups| {
            groups
                .get(regexp.mark)
                .and_then(|g| *g)
                .or_else(|| groups.first().and_then(|g| *g))
                .is_some_and(|(s, e)| s == offset && e == end)
        })
        .map(|groups| {
            groups
                .iter()
                .map(|g| g.map(|(s, e)| text[s..e].to_string()).unwrap_or_default())
                .collect()
        })
}

pub fn apply_engine_pt_filter(
    rule: &CompiledRule,
    filter: &XmlFilter,
    matched_texts: &[String],
    matched_positions: &[usize],
    tokens: &[&AnalyzedTokenReadings],
    sentence_text: &str,
    rm: RuleMatch,
    synthesizer: Option<&LangSynthesizer>,
    variant: &str,
) -> Option<RuleMatch> {
    let group_texts = rule
        .regexp
        .as_ref()
        .and_then(|re| regexp_group_texts(re, sentence_text, rm.offset(), rm.length()));
    let group_refs: Vec<&str> = group_texts
        .iter()
        .flat_map(|v| v.iter().map(String::as_str))
        .collect();
    apply_engine_pt_filter_with_groups(
        filter,
        &group_refs,
        matched_texts,
        matched_positions,
        tokens,
        rm,
        synthesizer,
        variant,
    )
}

pub fn apply_engine_pt_filter_with_groups(
    filter: &XmlFilter,
    groups: &[&str],
    matched_texts: &[String],
    matched_positions: &[usize],
    tokens: &[&AnalyzedTokenReadings],
    rm: RuleMatch,
    synthesizer: Option<&LangSynthesizer>,
    variant: &str,
) -> Option<RuleMatch> {
    let call = PtFilterCall {
        variant,
        args: &filter.args,
        matched_texts,
        matched_positions,
        tokens,
        regexp_groups: (!groups.is_empty()).then_some(groups),
        synthesizer,
    };
    apply_pt_filter(&filter.class, call, rm)
}
