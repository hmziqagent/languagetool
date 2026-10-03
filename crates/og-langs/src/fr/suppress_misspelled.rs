use og_core::{AnalyzedToken, AnalyzedTokenReadings, RuleMatch};
use og_rules::spelling_support::{SpellerVariant, prepare_line_for_speller};
use og_spell::Dictionary;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

pub const FRENCH_SUPPRESS_MISSPELLED_FILTER_CLASS: &str =
    "org.languagetool.rules.fr.FrenchSuppressMisspelledSuggestionsFilter";

pub fn shared_french_spelling_dictionary() -> Option<&'static Dictionary> {
    static CACHE: OnceLock<Option<Dictionary>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let resource_dir = crate::fr::fr_resource_dir()?;
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

fn shared_french_tokenizer() -> Option<&'static og_tokenizer::FrenchWordTokenizer> {
    static CACHE: OnceLock<Option<og_tokenizer::FrenchWordTokenizer>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let tagger = crate::fr::tagger::FrenchTagger::shared_arc()?;
            Some(og_tokenizer::FrenchWordTokenizer::new(Arc::new(
                move |w: &str| tagger.inner_is_word_tagged(w),
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

fn parse_filter_args(args: &str, matched_texts: &[String]) -> HashMap<String, String> {
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

fn replacement_matches_postag(word: &str, pattern: &str) -> bool {
    let Some(tagger) = crate::fr::tagger::FrenchTagger::shared_arc() else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::SuggestedReplacement;

    fn base_rm(replacement: &str) -> RuleMatch {
        RuleMatch::new(
            "Les deux parties de ce mot peuvent s'unir.",
            4,
            9,
            og_core::RuleMatchRule::new("FR_SPLIT_WORDS_HYPHEN", "d"),
            og_core::RuleMatchContext::new("", 0, 0),
        )
        .with_replacements(vec![SuggestedReplacement::new(replacement)])
    }

    #[test]
    fn french_dictionary_misspelling_pins() {
        assert!(is_misspelled("pianist"));
        assert!(!is_misspelled("pianiste"));
        assert!(is_misspelled("postprécédent"));
        assert!(is_misspelled("surfaceèdu"));
        assert!(is_misspelled("murêne"));
        assert!(!is_misspelled("là"));
        assert!(!is_misspelled("où"));
        assert!(!is_misspelled("guerre"));
        assert!(!is_misspelled(""));
        assert!(!is_misspelled("."));
    }

    #[test]
    fn replacements_are_misspelled_per_french_token() {
        assert!(!replacement_is_misspelled("j'arrive"));
        assert!(!replacement_is_misspelled("D'enfants"));
        assert!(!replacement_is_misspelled("J'ai"));
        assert!(!replacement_is_misspelled("l'opulence"));
        assert!(!replacement_is_misspelled("m'ennuyer"));
        assert!(replacement_is_misspelled("pianist"));
        assert!(replacement_is_misspelled("surfaceèdu"));
        assert!(!replacement_is_misspelled("le carré"));
    }

    #[test]
    fn accept_rule_match_drops_match_when_every_suggestion_is_misspelled() {
        assert!(accept_rule_match(base_rm("pianist"), "suppressMatch:true", &[]).is_none());
    }

    #[test]
    fn accept_rule_match_keeps_valid_suggestions_and_match() {
        let kept =
            accept_rule_match(base_rm("pianiste"), "suppressMatch:true", &[]).expect("match kept");
        let values: Vec<&str> = kept.replacements().iter().map(|r| r.value()).collect();
        assert_eq!(values, vec!["pianiste"]);
    }

    #[test]
    fn accept_rule_match_keeps_match_without_suggestions_when_suppress_match_false() {
        let kept =
            accept_rule_match(base_rm("pianist"), "suppressMatch:false", &[]).expect("match kept");
        assert!(kept.replacements().is_empty());
    }

    #[test]
    fn accept_rule_match_without_args_suppresses_by_default() {
        assert!(accept_rule_match(base_rm("pianist"), "", &[]).is_none());
    }

    #[test]
    fn accept_rule_match_drops_only_the_misspelled_suggestion() {
        let rm = base_rm("x").with_replacements(vec![
            SuggestedReplacement::new("pianist"),
            SuggestedReplacement::new("pianiste"),
        ]);
        let kept = accept_rule_match(rm, "suppressMatch:true", &[]).expect("match kept");
        let values: Vec<&str> = kept.replacements().iter().map(|r| r.value()).collect();
        assert_eq!(values, vec!["pianiste"]);
    }
}
