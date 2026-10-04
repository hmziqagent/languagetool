use og_core::{AnalyzedTokenReadings, RuleMatch, SuggestedReplacement};
use og_rules::postponed_adjective::LangSynthesizer;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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

fn raw_arg<'a>(args: &'a str, key: &str) -> Option<&'a str> {
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

fn is_all_uppercase(s: &str) -> bool {
    s.chars().all(|c| !c.is_alphabetic() || !c.is_lowercase())
}

fn is_capitalized_word(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => {
            chars.all(|c| !c.is_alphabetic() || !c.is_lowercase())
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
    full_matcher(pattern).is_match(value)
}

fn full_matcher(pattern: &str) -> Regex {
    static CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap();
    regex::Regex::clone(
        guard
            .entry(pattern.to_string())
            .or_insert_with(|| Regex::new(&format!("^(?:{pattern})$")).unwrap()),
    )
}

fn readings_match_pos_tag_regex(atr: &AnalyzedTokenReadings, pos_tag_regex: &str) -> bool {
    atr.readings()
        .iter()
        .flat_map(|r| r.pos_tags().iter())
        .any(|t| full_match(pos_tag_regex, t))
}

pub(crate) fn pt_rules_dir() -> Option<PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("pt/src/main/resources/org/languagetool/rules/pt"))
}

struct ConfusionEntry {
    word: String,
    tags: Vec<String>,
}

fn confusion_pairs(brazilian: bool) -> &'static HashMap<String, ConfusionEntry> {
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

const BRAZILIAN_STATES: [&str; 27] = [
    "AC", "AL", "AP", "AM", "BA", "CE", "DF", "ES", "GO", "MA", "MT", "MS", "MG", "PA", "PB", "PR",
    "PE", "PI", "RJ", "RN", "RS", "RO", "RR", "SC", "SP", "SE", "TO",
];

fn toponyms() -> &'static HashSet<String> {
    static TOPONYMS: OnceLock<HashSet<String>> = OnceLock::new();
    TOPONYMS.get_or_init(|| {
        let mut set = HashSet::new();
        let Some(resource_dir) = super::tagger::pt_resource_dir() else {
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

fn is_valid_toponym(toponym: &str) -> bool {
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

fn is_regular_participle(p: &str) -> bool {
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

fn soros_digit_shape(digit: u8, charset: [&str; 3]) -> String {
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

fn soros_overline(digits: &str) -> String {
    let mut out = String::new();
    for c in digits.chars() {
        out.push(c);
        out.push('\u{0305}');
    }
    out
}

fn soros_roman(input: &str) -> String {
    if input.is_empty() || !input.bytes().all(|b| b.is_ascii_digit()) {
        return String::new();
    }
    let digits = input.as_bytes();
    let Some(first) = digits.iter().position(|b| *b != b'0') else {
        return String::new();
    };
    run_digits(&digits[first..])
}

fn run_digits(digits: &[u8]) -> String {
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

fn convert_pronoun_to_accusative(pronoun_tag: &str) -> String {
    if pronoun_tag.ends_with("N00") {
        format!("{}A00", &pronoun_tag[..pronoun_tag.len() - 3])
    } else {
        pronoun_tag.to_string()
    }
}

fn pronoun_tags(
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

fn proclisis_pronoun_forms(old_pronoun: &str, old_verb: &str) -> Vec<String> {
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

fn marker_or_index(
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

fn select_reading<'a>(atr: &'a AnalyzedTokenReadings, select: &str) -> (Option<&'a str>, &'a str) {
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

fn composite_postag(
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

fn move_pronoun_tag(source_tag: &str, destination_tag: &str) -> String {
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

fn is_suggestion_exception_pt(token: &str, desired_postag: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pt::test_support::PortuguesePipeline;
    use og_core::AnalyzedToken;
    use og_rules::pattern_rule::PatternRuleEngine;
    use og_xml::compiler::{CompiledRule, CompiledRuleSet, XmlCompiler};
    use std::path::PathBuf;
    use std::sync::Arc;

    fn grammar_path() -> PathBuf {
        crate::engine::find_lt_resource_dir()
            .expect("lt resource dir")
            .join("pt/src/main/resources/org/languagetool/rules/pt/grammar.xml")
    }

    fn compiled_rules() -> &'static CompiledRuleSet {
        static RULES: OnceLock<CompiledRuleSet> = OnceLock::new();
        RULES.get_or_init(|| {
            let path = grammar_path();
            let xml = std::fs::read_to_string(&path).unwrap();
            XmlCompiler::new()
                .compile_file_with_base(&xml, path.parent())
                .expect("compile grammar.xml")
        })
    }

    fn synthesizer() -> Option<LangSynthesizer> {
        LangSynthesizer::for_language(
            &super::super::tagger::pt_resource_dir().expect("pt resource dir"),
            "pt",
        )
    }

    fn atr(word: &str, tags: &[(&str, &str)]) -> AnalyzedTokenReadings {
        let readings: Vec<AnalyzedToken> = tags
            .iter()
            .map(|(lemma, tag)| {
                AnalyzedToken::new(word, 0, word.len())
                    .with_lemma(*lemma)
                    .with_pos_tags(vec![tag.to_string()])
            })
            .collect();
        AnalyzedTokenReadings::new(AnalyzedToken::new(word, 0, word.len())).with_readings(readings)
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

    fn first_rule(id: &str) -> &'static CompiledRule {
        compiled_rules()
            .rules
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("rule {id} not compiled"))
    }

    #[test]
    fn confusion_pairs_data_parses_clean_and_accumulates() {
        for (brazilian, variant_file) in [(false, "pt-PT"), (true, "pt-BR")] {
            let map = confusion_pairs(brazilian);
            assert!(
                map.len() > 1000,
                "{variant_file} map must load the base file plus its variant overlay"
            );
            let medias = map.get("medias").expect("medias entry");
            assert_eq!(medias.word, "médias");
            assert_eq!(
                medias.tags,
                vec![
                    "NCFP000".to_string(),
                    "AQ0FP0".to_string(),
                    "NCFP000".to_string()
                ]
            );
            let tramites = map.get("tramites").expect("tramites entry");
            assert_eq!(tramites.word, "trâmites");
            assert_eq!(tramites.tags, vec!["NCMP000".to_string()]);
        }
        assert_eq!(
            confusion_pairs(true).get("anonimo").unwrap().word,
            "anônimo"
        );
        assert_eq!(
            confusion_pairs(false).get("anonimo").unwrap().word,
            "anónimo"
        );
        assert_eq!(confusion_pairs(false).get("bebe").unwrap().word, "bebé");
        assert_eq!(confusion_pairs(true).get("bebe").unwrap().word, "bebê");
    }

    #[test]
    fn confusion_check_resolves_form_and_postag_from_matched_texts() {
        let token = atr("tramites", &[("trâmite", "NCMP000")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&token];
        let matched = vec!["nos".to_string(), "tramites".to_string()];
        let rm = base_rm("m", 8, 8, "{suggestion}");
        let out = confusion_check_filter(
            "pt-PT",
            "form:\\2 postag:N.[MC]P.*|A..[MC]P.",
            &matched,
            &tokens,
            rm,
        )
        .expect("tramites -> trâmites");
        assert_eq!(out.replacements()[0].value(), "trâmites");
    }

    #[test]
    fn confusion_check_nulls_forms_missing_from_the_pairs() {
        let token = atr("servia", &[("servir", "VMII3S0")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&token];
        let matched = vec!["nos".to_string(), "servia".to_string()];
        let rm = base_rm("m", 9, 6, "{suggestion}");
        assert!(
            confusion_check_filter(
                "pt-PT",
                "form:\\2 postag:N.[MC]P.*|A..[MC]P.",
                &matched,
                &tokens,
                rm,
            )
            .is_none(),
            "'servia' is not a confusion pair, Java returns null"
        );
    }

    #[test]
    fn confusion_check_templates_carry_all_three_case_placeholders() {
        let token = atr("Anonimo", &[("anónimo", "AQ0MS0")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&token];
        let matched = vec!["Anonimo".to_string()];
        let rm = base_rm("m", 0, 7, "{Suggestion} / {suggestion} / {SUGGESTION}");
        let out = confusion_check_filter("pt-PT", "form:\\1 postag:[AN].*", &matched, &tokens, rm)
            .expect("anonimo entry exists in the pt-PT overlay");
        assert_eq!(out.replacements()[0].value(), "Anónimo / anónimo / ANÓNIMO");
        let rm = base_rm("m", 0, 7, "{Suggestion}");
        let out = confusion_check_filter("pt-BR", "form:\\1 postag:[AN].*", &matched, &tokens, rm)
            .expect("anonimo entry exists in the pt-BR overlay");
        assert_eq!(out.replacements()[0].value(), "Anônimo");
    }

    #[test]
    fn confusion_check_gendernumber_uses_only_the_first_reading() {
        let matched = vec!["os".to_string(), "tramites".to_string()];
        let source = atr("os", &[("o", "DA0MP0")]);
        let entry = atr("tramites", &[("trâmite", "NCMP000")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&source, &entry];
        let rm = base_rm("m", 3, 8, "{suggestion}");
        let out = confusion_check_filter(
            "pt-PT",
            "form:\\2 postag:N.[MC]P.*|A..[MC]P. gendernumberFrom:1",
            &matched,
            &tokens,
            rm,
        )
        .expect("MP source selects the MP entry");
        assert_eq!(out.replacements()[0].value(), "trâmites");

        let source = atr("o", &[("o", "DA0MS0")]);
        let entry = atr("tramites", &[("trâmite", "NCMP000")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&source, &entry];
        let rm = base_rm("m", 2, 8, "{suggestion}");
        assert!(
            confusion_check_filter(
                "pt-PT",
                "form:\\2 postag:N.[MC]P.*|A..[MC]P. gendernumberFrom:1",
                &matched,
                &tokens,
                rm,
            )
            .is_none(),
            "MS source demands a singular first reading, NCMP000 must be rejected"
        );

        let source = atr("x", &[("x", "ZP0")]);
        let entry = atr("tramites", &[("trâmite", "NCMP000")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&source, &entry];
        let rm = base_rm("m", 2, 8, "{suggestion}");
        assert!(
            confusion_check_filter(
                "pt-PT",
                "form:\\2 postag:N.[MC]P.*|A..[MC]P. gendernumberFrom:1",
                &matched,
                &tokens,
                rm,
            )
            .is_none(),
            "a source matching no gender pattern leaves the replacement unset"
        );
    }

    #[test]
    fn confusion_check_rewrites_message_without_diacritic_gain() {
        let mut message = "se escribe con tilde".to_string();
        let replacement = "casa";
        let form = "kasa";
        if !(has_diacritics(replacement) && !has_diacritics(form)) {
            message = message.replace("se escribe con tilde", "se escribe de otra manera");
        }
        assert_eq!(message, "se escribe de otra manera");
        assert!(has_diacritics("anónimo"));
        assert!(!has_diacritics("anonimo"));
    }

    #[test]
    fn toponym_map_validates_municipalities_with_suffix_relaxation() {
        assert!(is_valid_toponym("Niterói"));
        assert!(is_valid_toponym("Angra dos Reis"));
        assert!(is_valid_toponym("Varre-Sai"));
        assert!(is_valid_toponym("Paty Do Alferes"));
        assert!(is_valid_toponym("São José do Vale do Rio Preto"));
        assert!(is_valid_toponym("do Rio de Janeiro"));
        assert!(!is_valid_toponym("Fernando Gabeira"));
        assert!(!is_valid_toponym("qualquer coisa"));
    }

    #[test]
    fn toponym_filter_accepts_and_rejects_like_java() {
        let rm = base_rm("m", 17, 5, "{suggestion}");
        let out = brazilian_toponym_filter(&["Niterói (RJ)", "Niterói", " (RJ)", "RJ"], rm)
            .expect("Niterói is an RJ municipality");
        assert_eq!(out.replacements()[0].value(), "\u{2013}RJ");
        let rm = base_rm("m", 17, 3, "{suggestion}");
        assert!(
            brazilian_toponym_filter(&["Niterói–RJ", "Niterói", "–RJ", "RJ"], rm).is_none(),
            "already-correct en dash suggestion is dropped"
        );
        let rm = base_rm("m", 33, 4, "{suggestion}");
        assert!(
            brazilian_toponym_filter(
                &["Fernando Gabeira (RJ)", "Fernando Gabeira", " (RJ)", "RJ"],
                rm,
            )
            .is_none(),
            "non-toponym is dropped"
        );
    }

    #[test]
    fn toponym_rule_examples_pass_end_to_end_through_the_compiled_regexp() {
        let pipeline = PortuguesePipeline::shared();
        let engine = PatternRuleEngine::new();
        let cases: &[(&str, &str, Option<&str>)] = &[
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PARENTESES",
                "Nasceu em Niterói (RJ).",
                Some("\u{2013}RJ"),
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PARENTESES",
                "Nasceu em Niterói–RJ.",
                None,
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PARENTESES",
                "O candidato a prefeito Fernando Gabeira (RJ).",
                None,
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Nasceu em Niterói – RJ.",
                Some("\u{2013}RJ"),
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Nasceu em Niterói/RJ.",
                Some("\u{2013}RJ"),
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Nativo de Angra dos Reis/RJ.",
                Some("\u{2013}RJ"),
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Nativo de Varre-Sai/RJ.",
                Some("\u{2013}RJ"),
            ),
            (
                "SEPARADORES_DE_ESTADOS_BRASILEIROS_MEIA_RISCA_PONTUACAO_SIMPLES",
                "Nativo de Paty Do Alferes/RJ.",
                Some("\u{2013}RJ"),
            ),
        ];
        for (id, text, expected) in cases {
            let rule = first_rule(id);
            let sentence = &pipeline.analyze_text(text)[0];
            let raw = engine.match_rule(rule, sentence);
            let regexp = rule.regexp.as_ref().expect("regexp rule");
            let capture_sets: Vec<Vec<String>> = if let Some(re) = regexp.regex.as_ref() {
                re.captures_iter(sentence.text())
                    .map(|caps| {
                        (0..caps.len())
                            .map(|i| {
                                caps.get(i)
                                    .map(|m| m.as_str().to_string())
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .collect()
            } else if let Some(re) = regexp.fancy_regex.as_ref() {
                re.captures_iter(sentence.text())
                    .flatten()
                    .map(|caps| {
                        (0..caps.len())
                            .map(|i| {
                                caps.get(i)
                                    .map(|m| m.as_str().to_string())
                                    .unwrap_or_default()
                            })
                            .collect()
                    })
                    .collect()
            } else {
                Vec::new()
            };
            assert_eq!(
                raw.len(),
                capture_sets.len(),
                "{id} on {text}: engine matches and capture sets must align"
            );
            let mut kept: Vec<String> = Vec::new();
            for (rm, groups) in raw.into_iter().zip(capture_sets) {
                let refs: Vec<&str> = groups.iter().map(String::as_str).collect();
                if let Some(out) = brazilian_toponym_filter(&refs, rm) {
                    kept.push(out.replacements()[0].value().to_string());
                }
            }
            match expected {
                Some(value) => {
                    assert_eq!(kept, vec![value.to_string()], "{id} on {text}");
                }
                None => {
                    assert!(kept.is_empty(), "{id} on {text} kept {kept:?}");
                }
            }
        }
    }

    #[test]
    fn participle_filter_selects_irregular_form_for_regular_token() {
        let synth = synthesizer().expect("pt synthesizer from the main dict");
        let gastado = atr("gastado", &[("gastar", "VMP00SM")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&gastado];
        let rm = base_rm("m", 0, 7, "{suggestion}");
        let out = regular_irregular_participle_filter(
            Some(&synth),
            "direction:RegularToIrregular",
            &tokens,
            rm,
        )
        .expect("gastado -> gasto");
        assert_eq!(out.replacements()[0].value(), "gasto");
    }

    #[test]
    fn participle_filter_nulls_tokens_without_a_vmp_reading() {
        let synth = synthesizer().expect("pt synthesizer from the main dict");
        let limpo = atr("limpo", &[("limpar", "VMIP1S0"), ("limpo", "AQ0MS0")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&limpo];
        let rm = base_rm("m", 0, 5, "{suggestion}");
        assert!(
            regular_irregular_participle_filter(
                Some(&synth),
                "direction:IrregularToRegular",
                &tokens,
                rm,
            )
            .is_none(),
            "the dict carries no VMP reading for 'limpo', Java's hasPosTagStartingWith fails"
        );
    }

    #[test]
    fn participle_rule_example_passes_end_to_end() {
        let pipeline = PortuguesePipeline::shared();
        let engine = PatternRuleEngine::new();
        let synth = Arc::new(synthesizer().expect("pt synthesizer"));
        let rule = compiled_rules()
            .rules
            .iter()
            .find(|r| {
                r.filter.as_ref().is_some_and(|f| {
                    f.class.ends_with("RegularIrregularParticipleFilter")
                        && f.args.contains("RegularToIrregular")
                })
            })
            .expect("RegularToIrregular participle rule");
        let text = "O dinheiro foi gastado o mês passado.";
        let sentence = &pipeline.analyze_text(text)[0];
        let tokens: Vec<&og_core::AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
        let raw = engine
            .match_rule(rule, sentence)
            .into_iter()
            .next()
            .expect("raw pattern match on 'foi gastado'");
        let out = regular_irregular_participle_filter(
            Some(synth.as_ref()),
            "direction:RegularToIrregular",
            &tokens,
            raw,
        )
        .expect("filtered match");
        assert_eq!(out.replacements()[0].value(), "gasto");
        assert_eq!(&text[out.offset()..out.offset() + out.length()], "gastado");
    }

    #[test]
    fn soros_roman_matches_the_active_roman_sor_rules() {
        assert_eq!(soros_roman("1"), "I");
        assert_eq!(soros_roman("2"), "II");
        assert_eq!(soros_roman("4"), "IV");
        assert_eq!(soros_roman("9"), "IX");
        assert_eq!(soros_roman("10"), "X");
        assert_eq!(soros_roman("20"), "XX");
        assert_eq!(soros_roman("40"), "XL");
        assert_eq!(soros_roman("44"), "XLIV");
        assert_eq!(soros_roman("90"), "XC");
        assert_eq!(soros_roman("400"), "CD");
        assert_eq!(soros_roman("900"), "CM");
        assert_eq!(soros_roman("1987"), "MCMLXXXVII");
        assert_eq!(soros_roman("3999"), "MMMCMXCIX");
        assert_eq!(soros_roman("0"), "");
        assert_eq!(soros_roman(""), "");
        assert_eq!(soros_roman("12a"), "");
        assert_eq!(soros_roman("007"), "VII");
        assert_eq!(soros_roman("4000"), "4\u{0305}");
        assert_eq!(soros_roman("12000"), "1\u{0305}2\u{0305}");
    }

    #[test]
    fn roman_numeral_rule_examples_pass_end_to_end() {
        let pipeline = PortuguesePipeline::shared();
        let engine = PatternRuleEngine::new();
        let rule = first_rule("SECULO_EM_NUMERAIS_ROMANOS");
        for (text, source, expected) in [
            ("No começo do século 20.", "20", "XX"),
            ("Nasceu no século 1.", "1", "I"),
            ("A história se passa no século 44.", "44", "XLIV"),
        ] {
            let sentence = &pipeline.analyze_text(text)[0];
            let raw = engine
                .match_rule(rule, sentence)
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("raw match on {text}"));
            assert_eq!(raw.replacements()[0].value(), "{suggestion}", "{text}");
            let matched = vec!["século".to_string(), source.to_string()];
            let out = roman_numeral_filter("arabicSource:\\2", &matched, raw).expect("always set");
            assert_eq!(out.replacements()[0].value(), expected, "{text}");
        }
    }

    #[test]
    fn enclisis_pronoun_tag_extraction_ports_java_cases() {
        let nos = atr("nos", &[("nos", "PP1CP000")]);
        assert_eq!(
            pronoun_tags(&nos, "comeu", false),
            vec!["PP1CPO00".to_string()]
        );
        assert_eq!(
            pronoun_tags(&nos, "comeram", false),
            vec!["PP1CPO00".to_string(), "PP3MPA00".to_string()]
        );
        assert_eq!(
            pronoun_tags(&nos, "põe", false),
            vec!["PP1CPO00".to_string(), "PP3MPA00".to_string()]
        );
        let os = atr("os", &[("o", "PP3MPA00")]);
        assert_eq!(
            pronoun_tags(&os, "comeu", true),
            vec!["PP3MPA00".to_string()],
            "already-accusative tags pass through the N00 conversion unchanged"
        );
        let ele = atr("ele", &[("ele", "PP3CN00")]);
        assert_eq!(pronoun_tags(&ele, "viu", true), vec!["PP3CA00".to_string()]);
        let noun = atr("casa", &[("casa", "NCFS000")]);
        assert!(pronoun_tags(&noun, "viu", false).is_empty());
    }

    #[test]
    fn enclisis_filter_ports_the_all_caps_example_shape() {
        let verb = atr("TEM", &[("ter", "VMIP3P0")]);
        let hyphen = atr("-", &[]);
        let pronoun = atr("O", &[("o", "PP3MSA00")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&verb, &hyphen, &pronoun];
        let rm = base_rm("m", 4, 5, "{suggestion}");
        let out = portuguese_enclisis_filter(
            synthesizer().as_ref(),
            "verbPos:0 pronounPos:2 convertToAccusative:False",
            &tokens,
            &[0, 1, 2],
            rm,
        )
        .expect("pronoun tags are non-empty");
        assert!(
            out.replacements().is_empty(),
            "the pt synth dict is absent, so enclisisTag lookups return no forms"
        );
    }

    #[test]
    fn proclisis_pronoun_forms_port_the_java_switch() {
        assert_eq!(proclisis_pronoun_forms("lo", "distribuí"), vec!["o"]);
        assert_eq!(proclisis_pronoun_forms("no", "pôs"), vec!["o"]);
        assert_eq!(proclisis_pronoun_forms("la", "comeu"), vec!["a"]);
        assert_eq!(proclisis_pronoun_forms("na", "pôs"), vec!["a"]);
        assert_eq!(proclisis_pronoun_forms("los", "comeu"), vec!["os"]);
        assert_eq!(proclisis_pronoun_forms("las", "comeu"), vec!["as"]);
        assert_eq!(proclisis_pronoun_forms("nas", "pôs"), vec!["as"]);
        assert_eq!(proclisis_pronoun_forms("nos", "comeu"), vec!["nos"]);
        assert_eq!(proclisis_pronoun_forms("nos", "puseram"), vec!["nos", "os"]);
        assert_eq!(proclisis_pronoun_forms("nos", "põe"), vec!["nos", "os"]);
        assert_eq!(proclisis_pronoun_forms("lhe", "deu"), vec!["lhe"]);
    }

    #[test]
    fn proclisis_filter_builds_pronoun_verb_suggestions() {
        let enclitic = atr("quis-lo", &[("querer", "VMIS3S0:PP3MSA00")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&enclitic];
        let synth = synthesizer().expect("pt synthesizer from the main dict");
        let rm = base_rm("m", 0, 7, "{suggestion}");
        let out = portuguese_proclisis_filter(Some(&synth), &tokens, &[0], rm).expect("match");
        assert_eq!(
            out.replacements()
                .iter()
                .map(|r| r.value().to_string())
                .collect::<Vec<String>>(),
            vec!["o quis".to_string()],
            "VMIS3S0 synthesizes to 'quis' and the clitic 'lo' maps to 'o'"
        );
    }

    #[test]
    fn pattern_token_range_windows_the_matched_span() {
        let a = atr("a", &[]);
        let b = atr("b", &[]);
        let c = atr("c", &[]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&a, &b, &c];
        assert_eq!(pattern_token_range(&tokens, &[1, 2]).len(), 2);
        assert!(pattern_token_range(&tokens, &[]).is_empty());
    }

    #[test]
    fn move_pronoun_tag_ports_the_java_split_semantics() {
        assert_eq!(
            move_pronoun_tag("VMN0000:PP3MSN00", "VMIP3P0"),
            "VMIP3P0:PP3MSN00"
        );
        assert_eq!(move_pronoun_tag("VMN0000", "VMIP3P0"), "VMIP3P0");
        assert_eq!(
            move_pronoun_tag("VMN0000:PP3MSN00:X", "VMIP3P0"),
            "VMIP3P0",
            "a three-part source tag is not a verb:pronoun pair"
        );
        assert_eq!(move_pronoun_tag("VMN0000:", "VMIP3P0"), "VMIP3P0");
    }

    #[test]
    fn suggestion_exception_rejects_plural_without_s() {
        assert!(is_suggestion_exception_pt("estudo", "VMIP1P0"));
        assert!(is_suggestion_exception_pt("estudai", "VMIP2P0"));
        assert!(!is_suggestion_exception_pt("estudamos", "VMIP1P0"));
        assert!(!is_suggestion_exception_pt("estuda", "VMIP3S0"));
    }

    #[test]
    fn advanced_synthesizer_composes_postag_replace_groups() {
        assert_eq!(
            composite_postag(
                "V(.)N.*",
                "V...(..).*",
                "VMN0000",
                "VMIP1S0",
                r"V\a1(IP|IF)\b1."
            ),
            "VM(IP|IF)1S."
        );
        assert_eq!(
            composite_postag("V.*", "NC(.)(.)000", "VMN0000", "NCFS000", "VMP00\\b2\\b1"),
            "VMP00SF"
        );
        assert_eq!(
            composite_postag("V.*", "NC.*", "AQ0MS0", "NCFS000", r"AQ0\b1\b20"),
            "AQ0\\b1\\b20",
            "without matching groups the replace text stays verbatim"
        );
    }

    #[test]
    fn advanced_synthesizer_synthesizes_from_the_lemma_and_postag_tokens() {
        let tenho = atr("tenho", &[("ter", "VMIP1S0")]);
        let pesquisado = atr("pesquisado", &[("pesquisar", "VMP00SM")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&tenho, &pesquisado];
        let synth = synthesizer().expect("pt synthesizer from the main dict");
        let rm = base_rm("m", 3, 17, "{suggestion}");
        let out = advanced_synthesizer_filter(
            Some(&synth),
            "lemmaFrom:2 lemmaSelect:V.* postagFrom:1 postagSelect:V.*",
            &[],
            &[0, 1],
            &tokens,
            rm,
        )
        .expect("filtered match");
        assert_eq!(
            out.replacements()
                .iter()
                .map(|r| r.value().to_string())
                .collect::<Vec<String>>(),
            vec!["pesquiso".to_string()],
            "pesquisar synthesized with tenho's VMIP1S0"
        );
    }

    #[test]
    fn advanced_synthesizer_rejects_new_lemma_placeholders() {
        let tenho = atr("tenho", &[("ter", "VMIP1S0")]);
        let pesquisado = atr("pesquisado", &[("pesquisar", "VMP00SM")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&tenho, &pesquisado];
        let synth = synthesizer().expect("pt synthesizer from the main dict");
        let rm = base_rm("m", 3, 17, "{suggestion}");
        assert!(
            advanced_synthesizer_filter(
                Some(&synth),
                "lemmaFrom:2 lemmaSelect:V.* postagFrom:1 postagSelect:V.* newLemma:_x",
                &[],
                &[0, 1],
                &tokens,
                rm,
            )
            .is_none(),
            "pt does not override getNewLemma, so a '_' placeholder nulls the lemma"
        );
    }

    #[test]
    fn advanced_synthesizer_routes_through_the_dispatch() {
        let tenho = atr("tenho", &[("ter", "VMIP1S0")]);
        let pesquisado = atr("pesquisado", &[("pesquisar", "VMP00SM")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&tenho, &pesquisado];
        let synth = synthesizer().expect("pt synthesizer from the main dict");
        let call = PtFilterCall {
            variant: "pt-PT",
            args: "lemmaFrom:2 lemmaSelect:V.* postagFrom:1 postagSelect:V.*",
            matched_texts: &[],
            matched_positions: &[0, 1],
            tokens: &tokens,
            regexp_groups: None,
            synthesizer: Some(&synth),
        };
        let rm = base_rm("m", 3, 17, "{suggestion}");
        let out = apply_pt_filter(
            "org.languagetool.rules.pt.AdvancedSynthesizerFilter",
            call,
            rm,
        )
        .expect("synthesizer match");
        assert_eq!(out.replacements()[0].value(), "pesquiso");
    }

    #[test]
    fn dispatch_recognizes_exactly_the_seven_java_filter_classes() {
        for class in [
            "org.languagetool.rules.pt.ConfusionCheckFilter",
            "org.languagetool.rules.pt.BrazilianToponymFilter",
            "org.languagetool.rules.pt.RegularIrregularParticipleFilter",
            "org.languagetool.rules.pt.RomanNumeralFilter",
            "org.languagetool.rules.pt.PortugueseEnclisisFilter",
            "org.languagetool.rules.pt.PortugueseProclisisFilter",
            "org.languagetool.rules.pt.AdvancedSynthesizerFilter",
        ] {
            assert!(is_pt_filter(class), "{class} must route");
        }
        assert!(!is_pt_filter(
            "org.languagetool.rules.es.ConfusionCheckFilter"
        ));
        assert!(!is_pt_filter(
            "org.languagetool.rules.es.AdvancedSynthesizerFilter"
        ));
        assert!(!is_pt_filter("org.languagetool.rules.pt.FutureDateFilter"));
        assert!(!is_pt_filter(""));
    }

    #[test]
    fn dispatch_windows_the_confusion_pattern_tokens_for_gendernumber_from() {
        let unmatched = atr("x", &[("x", "ZP0")]);
        let source = atr("os", &[("o", "DA0MP0")]);
        let entry = atr("tramites", &[("trâmite", "NCMP000")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&unmatched, &source, &entry];
        let matched = vec!["os".to_string(), "tramites".to_string()];
        let call = PtFilterCall {
            variant: "pt-PT",
            args: "form:\\2 postag:N.[MC]P.*|A..[MC]P. gendernumberFrom:1",
            matched_texts: &matched,
            matched_positions: &[1, 2],
            tokens: &tokens,
            regexp_groups: None,
            synthesizer: None,
        };
        let rm = base_rm("m", 3, 8, "{suggestion}");
        let out = apply_pt_filter("org.languagetool.rules.pt.ConfusionCheckFilter", call, rm)
            .expect("gendernumberFrom must index the match window, not the sentence tokens");
        assert_eq!(out.replacements()[0].value(), "trâmites");
    }

    #[test]
    fn dispatch_routes_the_toponym_filter_through_the_regexp_groups() {
        let tokens: Vec<&AnalyzedTokenReadings> = Vec::new();
        let call = PtFilterCall {
            variant: "pt-BR",
            args: "foo:bar",
            matched_texts: &[],
            matched_positions: &[],
            tokens: &tokens,
            regexp_groups: None,
            synthesizer: None,
        };
        let rm = base_rm("m", 17, 5, "{suggestion}");
        assert!(
            apply_pt_filter("org.languagetool.rules.pt.BrazilianToponymFilter", call, rm).is_none(),
            "the regexp path must pass capture group texts"
        );
        let groups = ["Niterói (RJ)", "Niterói", " (RJ)", "RJ"];
        let call = PtFilterCall {
            variant: "pt-BR",
            args: "foo:bar",
            matched_texts: &[],
            matched_positions: &[],
            tokens: &tokens,
            regexp_groups: Some(&groups),
            synthesizer: None,
        };
        let rm = base_rm("m", 17, 5, "{suggestion}");
        let out = apply_pt_filter("org.languagetool.rules.pt.BrazilianToponymFilter", call, rm)
            .expect("Niterói is an RJ municipality");
        assert_eq!(out.replacements()[0].value(), "\u{2013}RJ");
    }

    #[test]
    fn dispatch_drives_the_compiled_confusion_rule_end_to_end() {
        let pipeline = PortuguesePipeline::shared();
        let engine = PatternRuleEngine::new();
        let rule = compiled_rules()
            .rules
            .iter()
            .find(|r| {
                r.filter.as_ref().is_some_and(|f| {
                    f.class.ends_with("pt.ConfusionCheckFilter") && f.args.contains("N.[MC]P")
                })
            })
            .expect("nos|são confusion subrule");
        let text = "Um erro nos tramites.";
        let sentence = &pipeline.analyze_text(text)[0];
        let raw = engine
            .match_rule(rule, sentence)
            .into_iter()
            .next()
            .expect("raw pattern match on 'nos tramites'");
        assert_eq!(raw.replacements()[0].value(), "{suggestion}");
        let tokens: Vec<&AnalyzedTokenReadings> = sentence.non_whitespace_tokens();
        let marker_idx = tokens
            .iter()
            .position(|t| t.token().start() == raw.offset())
            .expect("marker token");
        let matched_texts = vec!["nos".to_string(), "tramites".to_string()];
        let start = marker_idx + 1 - matched_texts.len();
        let matched_positions: Vec<usize> = (start..=marker_idx).collect();
        let call = PtFilterCall {
            variant: "pt-PT",
            args: rule.filter.as_ref().unwrap().args.as_str(),
            matched_texts: &matched_texts,
            matched_positions: &matched_positions,
            tokens: &tokens,
            regexp_groups: None,
            synthesizer: None,
        };
        let out = apply_pt_filter("org.languagetool.rules.pt.ConfusionCheckFilter", call, raw)
            .expect("tramites -> trâmites");
        assert_eq!(out.replacements()[0].value(), "trâmites");
    }

    #[test]
    fn dispatch_routes_the_four_pattern_path_filters() {
        let synth = synthesizer().expect("pt synthesizer from the main dict");
        let gastado = atr("gastado", &[("gastar", "VMP00SM")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&gastado];
        let call = PtFilterCall {
            variant: "pt-PT",
            args: "direction:RegularToIrregular",
            matched_texts: &[],
            matched_positions: &[0],
            tokens: &tokens,
            regexp_groups: None,
            synthesizer: Some(&synth),
        };
        let rm = base_rm("m", 0, 7, "{suggestion}");
        let out = apply_pt_filter(
            "org.languagetool.rules.pt.RegularIrregularParticipleFilter",
            call,
            rm,
        )
        .expect("gastado -> gasto");
        assert_eq!(out.replacements()[0].value(), "gasto");

        let matched = vec!["século".to_string(), "20".to_string()];
        let call = PtFilterCall {
            variant: "pt-PT",
            args: "arabicSource:\\2",
            matched_texts: &matched,
            matched_positions: &[0, 1],
            tokens: &tokens,
            regexp_groups: None,
            synthesizer: None,
        };
        let rm = base_rm("m", 0, 2, "{suggestion}");
        let out = apply_pt_filter("org.languagetool.rules.pt.RomanNumeralFilter", call, rm)
            .expect("roman replacement is always set");
        assert_eq!(out.replacements()[0].value(), "XX");

        let verb = atr("TEM", &[("ter", "VMIP3P0")]);
        let hyphen = atr("-", &[]);
        let pronoun = atr("O", &[("o", "PP3MSA00")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&verb, &hyphen, &pronoun];
        let call = PtFilterCall {
            variant: "pt-BR",
            args: "verbPos:0 pronounPos:2 convertToAccusative:False",
            matched_texts: &[],
            matched_positions: &[0, 1, 2],
            tokens: &tokens,
            regexp_groups: None,
            synthesizer: Some(&synth),
        };
        let rm = base_rm("m", 4, 5, "{suggestion}");
        let out = apply_pt_filter(
            "org.languagetool.rules.pt.PortugueseEnclisisFilter",
            call,
            rm,
        )
        .expect("pronoun tags are non-empty");
        assert!(out.replacements().is_empty());

        let enclitic = atr("quis-lo", &[("querer", "VMIS3S0:PP3MSA00")]);
        let tokens: Vec<&AnalyzedTokenReadings> = vec![&enclitic];
        let call = PtFilterCall {
            variant: "pt-PT",
            args: "",
            matched_texts: &[],
            matched_positions: &[0],
            tokens: &tokens,
            regexp_groups: None,
            synthesizer: Some(&synth),
        };
        let rm = base_rm("m", 0, 7, "{suggestion}");
        let out = apply_pt_filter(
            "org.languagetool.rules.pt.PortugueseProclisisFilter",
            call,
            rm,
        )
        .expect("proclisis match");
        assert_eq!(out.replacements()[0].value(), "o quis");
    }
}
