use crate::spelling_support::{SpellerVariant, prepare_line_for_speller};
use og_core::{AnalyzedToken, AnalyzedTokenReadings, RuleMatch};
use og_spell::Dictionary;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

pub const FRENCH_SUPPRESS_MISSPELLED_FILTER_CLASS: &str =
    "org.languagetool.rules.fr.FrenchSuppressMisspelledSuggestionsFilter";

pub fn fr_resource_dir() -> Option<PathBuf> {
    [
        PathBuf::from("languagetool/languagetool-language-modules/fr/src/main/resources/org/languagetool/resource/fr"),
        PathBuf::from("../languagetool/languagetool-language-modules/fr/src/main/resources/org/languagetool/resource/fr"),
        PathBuf::from("../languagetool-language-modules/fr/src/main/resources/org/languagetool/resource/fr"),
        PathBuf::from("../../languagetool/languagetool-language-modules/fr/src/main/resources/org/languagetool/resource/fr"),
        PathBuf::from("../../languagetool-language-modules/fr/src/main/resources/org/languagetool/resource/fr"),
        PathBuf::from("../../../languagetool-language-modules/fr/src/main/resources/org/languagetool/resource/fr"),
        PathBuf::from("/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/fr/src/main/resources/org/languagetool/resource/fr"),
    ]
    .into_iter()
    .find(|p| p.join("french.dict").exists())
    .and_then(|p| p.canonicalize().ok())
}

pub fn shared_french_spelling_dictionary() -> Option<&'static Dictionary> {
    static CACHE: OnceLock<Option<Dictionary>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let resource_dir = fr_resource_dir()?;
            let dictionary = og_tagger::generic_tagger::shared_dictionary(&resource_dir, "french")?;
            let mut dict = Dictionary::from_shared_words(Arc::new(
                og_spell::morfologik_view::MorfologikView::new(dictionary),
            ));
            if let Ok(text) = std::fs::read_to_string(resource_dir.join("hunspell/spelling.txt")) {
                for line in text.lines() {
                    for word in prepare_line_for_speller(SpellerVariant::French, line) {
                        if !word.is_empty() {
                            dict.add_word(&word);
                        }
                    }
                }
            }
            Some(dict)
        })
        .as_ref()
}

pub fn is_misspelled(word: &str) -> bool {
    if word.is_empty() || !word.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    match shared_french_spelling_dictionary() {
        Some(dictionary) => !dictionary.contains(word),
        None => false,
    }
}

fn shared_french_tagger() -> Option<&'static Arc<og_tagger::GenericTagger>> {
    static CACHE: OnceLock<Option<Arc<og_tagger::GenericTagger>>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let tagger = og_tagger::GenericTagger::french(&fr_resource_dir()?);
            Some(Arc::new(tagger))
        })
        .as_ref()
}

fn shared_french_tokenizer() -> Option<&'static og_tokenizer::FrenchWordTokenizer> {
    static CACHE: OnceLock<Option<og_tokenizer::FrenchWordTokenizer>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let tagger = shared_french_tagger()?.clone();
            Some(og_tokenizer::FrenchWordTokenizer::new(Arc::new(
                move |w: &str| tagger.is_word_tagged(w),
            )))
        })
        .as_ref()
}

pub fn replacement_is_misspelled(value: &str) -> bool {
    match shared_french_tokenizer() {
        Some(tokenizer) => {
            use og_tokenizer::WordTokenizer as _;
            tokenizer
                .tokenize(value)
                .iter()
                .any(|t| is_misspelled(t.text()))
        }
        None => value.split_whitespace().any(is_misspelled),
    }
}

pub fn parse_filter_args(args: &str, matched_texts: &[String]) -> HashMap<String, String> {
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
        } else if let Ok(idx) = val.parse::<usize>()
            && idx > 0
            && idx <= matched_texts.len()
        {
            matched_texts[idx - 1].clone()
        } else {
            val.to_string()
        };
        result.insert(key.to_string(), resolved);
    }
    result
}

pub fn replacement_matches_postag(word: &str, pattern: &str) -> bool {
    let Some(tagger) = shared_french_tagger() else {
        return false;
    };
    let readings = og_tagger::Tagger::tag(tagger.as_ref(), &[word])
        .into_iter()
        .next()
        .map(|atr| atr.into_readings())
        .unwrap_or_default();
    AnalyzedTokenReadings::new(AnalyzedToken::new(word, 0, 0))
        .with_readings(readings)
        .has_pos_tag_matching(pattern)
}

pub fn accept_rule_match(
    mut rm: RuleMatch,
    args: &str,
    matched_texts: &[String],
) -> Option<RuleMatch> {
    let parsed = parse_filter_args(args, matched_texts);
    let suppress_match = parsed
        .get("suppressMatch")
        .is_none_or(|v| !v.eq_ignore_ascii_case("false"));
    let suppress_postag = parsed.get("SuppressPostag").cloned();
    let filter_postag = parsed.get("FilterPostag").cloned();
    let mut kept = Vec::new();
    for repl in rm.replacements() {
        let value = repl.value();
        if replacement_is_misspelled(value) {
            continue;
        }
        if let Some(sp) = suppress_postag.as_deref()
            && replacement_matches_postag(value, sp)
        {
            continue;
        }
        if let Some(fp) = filter_postag.as_deref()
            && !replacement_matches_postag(value, fp)
        {
            continue;
        }
        kept.push(repl.clone());
    }
    if kept.is_empty() && suppress_match {
        None
    } else {
        rm.set_replacements(kept);
        Some(rm)
    }
}
