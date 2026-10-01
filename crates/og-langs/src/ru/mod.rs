pub mod advanced_word_repeat;
pub mod dash;
pub mod filler_words;
pub mod paragraph_repeat_beginning;
pub mod specific_case;
pub mod verb_conjugation;
pub mod yo_speller;

pub use advanced_word_repeat::RussianWordRepeatRule;
pub use dash::RussianDashRule;
pub use filler_words::RussianFillerWordsRule;
pub use paragraph_repeat_beginning::RussianParagraphRepeatBeginningRule;
pub use specific_case::RussianSpecificCaseRule;
pub use verb_conjugation::RussianVerbConjugationRule;
pub use yo_speller::RussianYoSpellerRule;

use crate::en::{CompoundRule, SimpleReplaceRule, WordCoherencyRule};
use og_rules::native_rules::WordRepeatRule;

pub fn russian_unpaired_brackets_rule() -> og_rules::text_level_rules::GenericUnpairedBracketsRule {
    og_rules::text_level_rules::GenericUnpairedBracketsRule::new()
        .with_id("RU_UNPAIRED_BRACKETS")
        .with_symbols(
            vec!["(", "{", "„", "\"", "'", "“"],
            vec![")", "}", "“", "\"", "'", "”"],
        )
}

pub fn russian_word_coherency_rule() -> WordCoherencyRule {
    WordCoherencyRule::for_language(
        "RU_WORD_COHERENCY",
        "Единообразное написание слов с более чем одним допустимым написанием",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ru/src/main/resources/org/languagetool/rules/ru/coherency.txt"
        ),
    )
    .with_message_template("«{word1}» и «{word2}» не следует использовать одновременно")
}

pub fn russian_word_root_repeat_rule() -> WordCoherencyRule {
    WordCoherencyRule::for_language(
        "RU_WORD_ROOT_REPEAT",
        "Повтор однокоренных слов",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ru/src/main/resources/org/languagetool/rules/ru/wordrootrep.txt"
        ),
    )
    .with_message_template("«{word1}» и «{word2}» – однокоренные слова, их не стоит использовать одновременно")
    .with_default_off()
}

pub fn russian_simple_replace_rule() -> SimpleReplaceRule {
    let mut rule = SimpleReplaceRule::new(
        "RU_SIMPLE_REPLACE",
        "Поиск просторечий и ошибочных фраз",
        "MISC",
        "Общие правила",
    );
    rule = rule.with_message_template("«$match» — просторечие, исправление: $suggestions", ", ");
    rule.load_data(include_str!(
        "../../../../languagetool/languagetool-language-modules/ru/src/main/resources/org/languagetool/rules/ru/replace.txt"
    ));
    rule
}

pub fn russian_compound_rule() -> CompoundRule {
    CompoundRule::for_language(
        "RU_COMPOUNDS",
        "Правописание через дефис",
        "Эти слова должны быть написаны через дефис.",
        "Эти слова должны быть написаны слитно.",
        "Эти слова могут быть написаны через дефис или слитно.",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ru/src/main/resources/org/languagetool/resource/ru/compounds.txt"
        ),
    )
}

pub fn russian_simple_word_repeat_rule() -> WordRepeatRule {
    WordRepeatRule::new()
        .with_ignored_words(&["и", "по", "что"])
        .with_ignore_single_letter_repeats()
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::{Rule as _, TextLevelRule as _};

    #[test]
    fn russian_parameterized_natives_use_java_ids() {
        assert_eq!(
            russian_unpaired_brackets_rule().id(),
            "RU_UNPAIRED_BRACKETS"
        );
        assert_eq!(russian_word_coherency_rule().id(), "RU_WORD_COHERENCY");
        assert_eq!(russian_word_root_repeat_rule().id(), "RU_WORD_ROOT_REPEAT");
        assert_eq!(russian_simple_replace_rule().id(), "RU_SIMPLE_REPLACE");
        assert_eq!(russian_compound_rule().id(), "RU_COMPOUNDS");
        assert_eq!(russian_simple_word_repeat_rule().id(), "WORD_REPEAT_RULE");
        assert_eq!(RussianWordRepeatRule::new().id(), "RU_WORD_REPEAT");
        assert_eq!(
            RussianVerbConjugationRule::new().id(),
            "RU_VERB_CONJUGATION"
        );
        assert_eq!(RussianSpecificCaseRule::new().id(), "RU_SPECIFIC_CASE");
        assert_eq!(RussianDashRule::new().id(), "RU_DASH_RULE");
        assert_eq!(RussianFillerWordsRule::new().id(), "FILLER_WORDS_RU");
    }

    #[test]
    fn russian_word_coherency_flags_mixed_spelling() {
        let rule = russian_word_coherency_rule();
        let mk = |text: &str, start: usize| {
            let mut sentence = og_core::AnalyzedSentence::new(text, start, start + text.len());
            let mut tokens = Vec::new();
            let mut offset = 0;
            for part in text.split(' ') {
                if part.is_empty() {
                    continue;
                }
                let s = offset + text[offset..].find(part).unwrap_or(0);
                let e = s + part.len();
                offset = e;
                tokens.push(og_core::AnalyzedTokenReadings::new(
                    og_core::AnalyzedToken::new(part, start + s, start + e),
                ));
            }
            sentence.set_tokens(tokens);
            sentence
        };
        let data = std::fs::read_to_string(
            "languagetool/languagetool-language-modules/ru/src/main/resources/org/languagetool/rules/ru/coherency.txt",
        )
        .unwrap_or_default();
        let pair = data
            .lines()
            .find(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
            .and_then(|l| l.trim().split_once(';'));
        let Some((a, b)) = pair else {
            return;
        };
        let s1 = mk(a.trim(), 0);
        let s2 = mk(b, a.len() + 1);
        let text = format!("{} {}", a.trim(), b);
        let matches = rule.match_text(&text, &[s1, s2]);
        assert!(!matches.is_empty(), "second variant must be flagged");
    }
}
