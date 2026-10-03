pub mod accentuation;
pub mod compound;
pub mod dash;
pub mod filler_words;
pub mod readability;
pub mod simple_replace;
pub mod simple_replace2;
pub mod word_repeat;
pub mod word_tokenizer;

use crate::en::{WordCoherencyRule, WrongWordInContextRule};
use og_core::rule::{Rule, TextLevelRule};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtVariant {
    Brazil,
    Portugal,
}

pub fn variant_for_code(code: &str) -> Option<PtVariant> {
    match code {
        "pt-BR" => Some(PtVariant::Brazil),
        "pt-PT" => Some(PtVariant::Portugal),
        _ => None,
    }
}

pub fn variant_rules_dir(variant: PtVariant) -> &'static str {
    match variant {
        PtVariant::Brazil => "pt-BR",
        PtVariant::Portugal => "pt-PT",
    }
}

pub fn portuguese_word_coherency_rule() -> WordCoherencyRule {
    WordCoherencyRule::for_language(
        "PT_WORD_COHERENCY",
        "Consistência de palavras com grafias múltiplas",
        include_str!("../../../../languagetool/languagetool-language-modules/pt/src/main/resources/org/languagetool/rules/pt/coherency.txt"),
    )
    .with_message_template(
        "Não deve utilizar formas distintas de palavras com dupla grafia no mesmo texto. Escolha entre '{word1}' e '{word2}'.",
    )
}

pub fn portuguese_wrong_word_in_context_rule() -> WrongWordInContextRule {
    WrongWordInContextRule::for_language(
        "PORTUGUESE_WRONG_WORD_IN_CONTEXT",
        "Confusão de palavra dentro do contexto (p.ex. infligir/infringir, etc.)",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/pt/src/main/resources/org/languagetool/rules/pt/wrongWordInContext.txt"
        ),
        "Pretende dizer <suggestion>$SUGGESTION</suggestion> em vez de $WRONGWORD?",
        "Considere <suggestion>$SUGGESTION</suggestion>, i.e. $EXPLANATION_SUGGESTION, em vez de '$WRONGWORD', i.e. $EXPLANATION_WRONGWORD?",
    )
}

pub struct PortugueseNativeRules {
    pub rules: Vec<Arc<dyn Rule>>,
    pub text_level_rules: Vec<Arc<dyn TextLevelRule>>,
}

pub fn shared_native_rules() -> PortugueseNativeRules {
    PortugueseNativeRules {
        rules: vec![
            Arc::new(compound::PortugueseCompoundRule::post_reform()),
            Arc::new(compound::PortugueseCompoundRule::colour_hyphenation()),
            Arc::new(simple_replace::orthography_replace_rule()),
            Arc::new(simple_replace::portuguese_replace_rule()),
            Arc::new(simple_replace2::barbarisms_base()),
            Arc::new(simple_replace2::cliche_base()),
            Arc::new(simple_replace2::redundancy_base()),
            Arc::new(simple_replace2::wordiness_base()),
            Arc::new(simple_replace2::wikipedia_base()),
            Arc::new(word_repeat::PortugueseWordRepeatRule::new()),
            Arc::new(accentuation::PortugueseAccentuationCheckRule::new()),
            Arc::new(simple_replace2::diacritics_rule()),
            Arc::new(portuguese_wrong_word_in_context_rule()),
            Arc::new(simple_replace::english_contraction_rule()),
        ],
        text_level_rules: vec![
            Arc::new(filler_words::PortugueseFillerWordsRule::new()),
            Arc::new(word_repeat::PortugueseWordRepeatBeginningRule::new()),
            Arc::new(portuguese_word_coherency_rule()),
            Arc::new(readability::PortugueseReadabilityRule::new(true)),
            Arc::new(readability::PortugueseReadabilityRule::new(false)),
        ],
    }
}

pub fn variant_native_rules(variant: PtVariant) -> PortugueseNativeRules {
    let (replace, barbarisms, archaisms, cliche, redundancy, wordiness, wikipedia) = match variant {
        PtVariant::Portugal => (
            simple_replace2::portugal_replace_rule(),
            simple_replace2::barbarisms_portugal(),
            simple_replace2::archaisms_portugal(),
            simple_replace2::cliche_portugal(),
            simple_replace2::redundancy_portugal(),
            simple_replace2::wordiness_portugal(),
            simple_replace2::wikipedia_portugal(),
        ),
        PtVariant::Brazil => (
            simple_replace2::brazilian_replace_rule(),
            simple_replace2::barbarisms_brazil(),
            simple_replace2::archaisms_brazil(),
            simple_replace2::cliche_brazil(),
            simple_replace2::redundancy_brazil(),
            simple_replace2::wordiness_brazil(),
            simple_replace2::wikipedia_brazil(),
        ),
    };
    let agreement =
        (variant == PtVariant::Portugal).then(|| simple_replace::agreement_replace_rule());
    PortugueseNativeRules {
        rules: std::iter::once(
            Arc::new(compound::PortugueseCompoundRule::post_reform()) as Arc<dyn Rule>
        )
        .chain(std::iter::once(
            Arc::new(dash::PostReformPortugueseDashRule::new()) as Arc<dyn Rule>,
        ))
        .chain(agreement.map(|r| Arc::new(r) as Arc<dyn Rule>))
        .chain([
            Arc::new(replace) as Arc<dyn Rule>,
            Arc::new(barbarisms),
            Arc::new(archaisms),
            Arc::new(cliche),
            Arc::new(redundancy),
            Arc::new(wordiness),
            Arc::new(wikipedia),
        ])
        .collect(),
        text_level_rules: Vec::new(),
    }
}

pub fn priority_overrides() -> Vec<(&'static str, i32)> {
    vec![
        ("FRAGMENT_TWO_ARTICLES", 50),
        ("DEGREE_MINUTES_SECONDS", 30),
        ("INTERJECTIONS_PUNTUATION", 20),
        ("CONFUSION_POR_PÔR_V2", 10),
        ("PARONYM_POLITICA_523", 10),
        ("PARONYM_PRONUNCIA_262", 10),
        ("PARONYM_CRITICA_397", 10),
        ("PARONYM_INICIO_169", 10),
        ("LP_PARONYMS", 10),
        ("PARONYM_MUSICO_499_bis", 10),
        ("NA_NÃO", 10),
        ("VERB_COMMA_CONJUNCTION", 10),
        ("HOMOPHONE_AS_CARD", 5),
        ("TODOS_FOLLOWED_BY_NOUN_PLURAL", 3),
        ("TODOS_FOLLOWED_BY_NOUN_SINGULAR", 2),
        ("AUSENCIA_VIRGULA", 1),
        ("EMAIL", 1),
        ("UNPAIRED_BRACKETS", -5),
        ("PROFANITY", -6),
        ("PT_BARBARISMS_REPLACE", -10),
        ("BARBARISMS_PT_PT_V4", -10),
        ("PT_PT_SIMPLE_REPLACE", -11),
        ("PT_REDUNDANCY_REPLACE", -12),
        ("PT_WORDINESS_REPLACE", -13),
        ("PT_CLICHE_REPLACE", -17),
        ("INTERNET_ABBREVIATIONS", -24),
        ("CHILDISH_LANGUAGE", -25),
        ("ARCHAISMS", -26),
        ("INFORMALITIES", -27),
        ("BIASED_OPINION_WORDS", -31),
        ("PT_AGREEMENT_REPLACE", -35),
        ("CONTA_TO", -44),
        ("PT_DIACRITICS_REPLACE", -45),
        ("DIACRITICS", -45),
        ("PT_COMPOUNDS_POST_REFORM", -45),
        ("AUX_VERBO", -45),
        ("ENSINO_A_DISTANCIA", -45),
        ("OQ_O_QUE_ORTHOGRAPHY", -45),
        ("PT_ENGLISH_CONTRACTION_ORTHOGRAPHY", -45),
        ("EMAIL_SEM_HIFEN", -45),
        ("PRETERITO_PERFEITO", -51),
        ("PT_BR_SIMPLE_REPLACE", -51),
        ("CRASE_CONFUSION", -54),
        ("NAO_MILITARES_CIVIS", -54),
        ("NA_QUELE", -54),
        ("NOTAS_FICAIS", -54),
        ("GENERAL_VERB_AGREEMENT_ERRORS", -55),
        ("GENERAL_NUMBER_AGREEMENT_ERRORS", -56),
        ("GENERAL_GENDER_NUMBER_AGREEMENT_ERRORS", -56),
        ("FINAL_STOPS", -75),
        ("EU_NÓS_REMOVAL", -90),
        ("COLOCAÇÃO_ADVÉRBIO", -90),
        ("FAZER_USO_DE-USAR-RECORRER", -90),
        ("FORMAL_T-V_DISTINCTION", -100),
        ("FORMAL_T-V_DISTINCTION_ALL", -101),
        ("REPEATED_WORDS", -210),
        ("PT_WIKIPEDIA_COMMON_ERRORS", -500),
        ("UPPERCASE_SENTENCE_START", -600),
        ("FILLER_WORDS_PT", -990),
        ("LONG_SENTENCE", -997),
        ("LONG_PARAGRAPH", -998),
        ("READABILITY_RULE_SIMPLE_PT", -1100),
        ("READABILITY_RULE_DIFFICULT_PT", -1101),
        ("UNKNOWN_WORD", -2000),
        ("PT_COMPOUNDS_POST_REFORM_VARIANT", 1),
        ("PORTUGUESE_OLD_SPELLING_INTERNAL", -9),
    ]
}

#[cfg(test)]
pub(crate) mod test_util {
    use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};

    pub(crate) fn make_sentence(text: &str) -> AnalyzedSentence {
        let mut sentence = AnalyzedSentence::new(text, 0, text.len());
        let mut tokens = vec![AnalyzedTokenReadings::new(
            AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
        )];
        let mut offset = 0usize;
        while offset < text.len() {
            let next = text[offset..]
                .chars()
                .next()
                .expect("non-empty slice has a char");
            let end = if next.is_whitespace() {
                text[offset..]
                    .char_indices()
                    .find(|(_, c)| !c.is_whitespace())
                    .map(|(i, _)| offset + i)
                    .unwrap_or(text.len())
            } else if next.is_alphanumeric() || next == '-' || next == '\u{00E9}' {
                text[offset..]
                    .char_indices()
                    .find(|(_, c)| !(c.is_alphanumeric() || *c == '-'))
                    .map(|(i, _)| offset + i)
                    .unwrap_or(text.len())
            } else {
                offset + next.len_utf8()
            };
            tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                &text[offset..end],
                offset,
                end,
            )));
            offset = end;
        }
        sentence.set_tokens(tokens);
        sentence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_for_code_maps_codes() {
        assert_eq!(variant_for_code("pt-BR"), Some(PtVariant::Brazil));
        assert_eq!(variant_for_code("pt-PT"), Some(PtVariant::Portugal));
        assert_eq!(variant_for_code("pt"), None);
        assert_eq!(variant_rules_dir(PtVariant::Portugal), "pt-PT");
    }

    #[test]
    fn shared_native_rules_carry_java_ids() {
        let rules = shared_native_rules();
        let ids: Vec<&str> = rules.rules.iter().map(|r| r.id()).collect();
        for expected in [
            "PT_COMPOUNDS_POST_REFORM",
            "PT_COLOUR_HYPHENATION",
            "PT_SIMPLE_REPLACE_ORTHOGRAPHY",
            "PT_SIMPLE_REPLACE",
            "PT_BARBARISMS_REPLACE",
            "PT_CLICHE_REPLACE",
            "PT_REDUNDANCY_REPLACE",
            "PT_WORDINESS_REPLACE",
            "PT_WIKIPEDIA_COMMON_ERRORS",
            "PORTUGUESE_WORD_REPEAT_RULE",
            "ACCENTUATION_CHECK_PT",
            "PT_DIACRITICS_REPLACE",
            "PORTUGUESE_WRONG_WORD_IN_CONTEXT",
            "PT_ENGLISH_CONTRACTION_ORTHOGRAPHY",
        ] {
            assert!(ids.contains(&expected), "missing {expected} in {ids:?}");
        }
        let text_ids: Vec<&str> = rules.text_level_rules.iter().map(|r| r.id()).collect();
        for expected in [
            "FILLER_WORDS_PT",
            "PORTUGUESE_WORD_REPEAT_BEGINNING_RULE",
            "PT_WORD_COHERENCY",
            "READABILITY_RULE_SIMPLE_PT",
            "READABILITY_RULE_DIFFICULT_PT",
        ] {
            assert!(text_ids.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn portugal_variant_rules_carry_java_ids() {
        let rules = variant_native_rules(PtVariant::Portugal);
        let ids: Vec<&str> = rules.rules.iter().map(|r| r.id()).collect();
        for expected in [
            "PT_POSAO_DASH_RULE",
            "PT_AGREEMENT_REPLACE",
            "PT_PT_SIMPLE_REPLACE",
            "PT_ARCHAISMS_REPLACE",
        ] {
            assert!(ids.contains(&expected), "missing {expected} in {ids:?}");
        }
    }

    #[test]
    fn brazil_variant_rules_carry_java_ids() {
        let rules = variant_native_rules(PtVariant::Brazil);
        let ids: Vec<&str> = rules.rules.iter().map(|r| r.id()).collect();
        for expected in ["PT_BR_SIMPLE_REPLACE", "PT_ARCHAISMS_REPLACE"] {
            assert!(ids.contains(&expected), "missing {expected} in {ids:?}");
        }
        assert!(!ids.contains(&"PT_AGREEMENT_REPLACE"));
    }

    #[test]
    fn coherency_rule_matches_java_example() {
        let rule = portuguese_word_coherency_rule();
        let s1 = test_util::make_sentence("Foi um período duradouro.");
        let s2 = test_util::make_sentence("Tão marcante e duradoiro dificilmente será esquecido.");
        let text = format!("{} {}", s1.text(), s2.text());
        let matches = rule.match_text(&text, &[s1, s2]);
        assert!(!matches.is_empty(), "duradouro/duradoiro must be flagged");
    }

    #[test]
    fn wrong_word_context_matches_java_example() {
        let rule = portuguese_wrong_word_in_context_rule();
        let matches = rule.match_sentence(&test_util::make_sentence(
            "O acidente infringiu grandes danos.",
        ));
        assert!(
            matches
                .iter()
                .any(|m| m.replacements().iter().any(|r| r.value() == "infligiu")),
            "expected infligiu suggestion, got {matches:?}"
        );
    }
}
