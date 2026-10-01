pub mod inflected_one_word;
pub mod trans_verb;

pub use inflected_one_word::ArabicInflectedOneWordReplaceRule;
pub use trans_verb::ArabicTransVerbRule;

use crate::en::{SimpleReplaceRule, WordCoherencyRule};
use og_rules::native_rules::{CommaWhitespaceRule, DoublePunctuationRule, WordRepeatRule};

pub fn arabic_double_punctuation_rule() -> DoublePunctuationRule {
    DoublePunctuationRule::new()
        .with_id("ARABIC_DOUBLE_PUNCTUATION")
        .with_comma_character('،')
}

pub fn arabic_comma_whitespace_rule() -> CommaWhitespaceRule {
    CommaWhitespaceRule::new()
        .with_id("ARABIC_COMMA_PARENTHESIS_WHITESPACE")
        .with_comma_character('،')
}

pub fn arabic_question_mark_whitespace_rule() -> CommaWhitespaceRule {
    CommaWhitespaceRule::new()
        .with_id("ARABIC_QM_WHITESPACE")
        .with_comma_character('؟')
}

pub fn arabic_semicolon_whitespace_rule() -> CommaWhitespaceRule {
    CommaWhitespaceRule::new()
        .with_id("ARABIC_SC_WHITESPACE")
        .with_comma_character('؛')
}

pub fn arabic_word_repeat_rule() -> WordRepeatRule {
    WordRepeatRule::new()
        .with_id("ARABIC_WORD_REPEAT_RULE")
        .with_ignored_words(&["خطوة", "رويدا"])
}

fn replace_rule(
    id: &str,
    name: &str,
    message: &str,
    separator: &str,
    data: &str,
    category: (&str, &str),
) -> SimpleReplaceRule {
    let mut rule = SimpleReplaceRule::new(id, name, category.0, category.1);
    rule = rule.with_message_template(message, separator);
    rule.load_data(data);
    rule
}

pub fn arabic_darja_rule() -> SimpleReplaceRule {
    replace_rule(
        "AR_DARJA_REPLACE",
        "كلمات بديلة للكلمات العامية أو الأجنبية",
        "الكلمة عامية  أو أجنبية يفضل أن يقال $suggestions",
        " أو  ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/darja.txt"
        ),
        ("STYLE", "Style"),
    )
}

pub fn arabic_diacritics_rule() -> SimpleReplaceRule {
    replace_rule(
        "AR_DIACRITICS_REPLACE",
        "كلمات مشكولة للتوضيح",
        "'$match' كلمة يشيع نطقها نطقا خاطئا لذا نقترح تشكيلها كالآتي: $suggestions",
        " أو ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/diacritics.txt"
        ),
        ("STYLE", "Style"),
    )
}

pub fn arabic_simple_replace_rule() -> SimpleReplaceRule {
    replace_rule(
        "AR_SIMPLE_REPLACE",
        "قاعدة تطابق الكلمات التي يجب تجنبها وتقترح تصويبا لها",
        "قل $suggestions",
        " أو  ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/replaces.txt"
        ),
        ("CONFUSED_WORDS", "Confused Words"),
    )
}

pub fn arabic_homophones_rule() -> SimpleReplaceRule {
    replace_rule(
        "AR_HOMOPHONES_REPLACE",
        "كلمات متشابهة لفظا للتوضيح، يرجى التحقق منها مثل تشابه الظاء والضاد.",
        "قل $suggestions",
        " أو  ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/homophones.txt"
        ),
        ("CONFUSED_WORDS", "Confused Words"),
    )
}

pub fn arabic_redundancy_rule() -> SimpleReplaceRule {
    replace_rule(
        "AR_REDUNDANCY_REPLACE",
        "1. تكرار (عام)",
        "'$match' تعبير فيه تكرار.في بعض الحالات، يستحسن استعمال $suggestions",
        " أو ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/redundancies.txt"
        ),
        ("REDUNDANCY", "Redundancy"),
    )
}

pub fn arabic_wordiness_rule() -> SimpleReplaceRule {
    replace_rule(
        "AR_WORDINESS_REPLACE",
        "2. حشو(تعبير فيه تكرار)",
        "'$match' تعبير فيه حشو يفضل أن يقال $suggestions",
        " أو ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/wordiness.txt"
        ),
        ("REDUNDANCY", "Redundancy"),
    )
}

pub fn arabic_word_coherency_rule() -> WordCoherencyRule {
    WordCoherencyRule::for_language(
        "AR_WORD_COHERENCY",
        "ضبط انسجام التهجئة للكلمات التي تكتب بطرق مختلفة مقبولة.",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/coherency.txt"
        ),
    )
    .with_message_template(
        "تجنب استعمال شكلين للكلمة نفسها ('{word1}' و '{word2}') في  النص نفسه.",
    )
}

pub fn arabic_wrong_word_in_context_rule() -> crate::en::WrongWordInContextRule {
    crate::en::WrongWordInContextRule::for_language(
        "ARABIC_WRONG_WORD_IN_CONTEXT",
        "كلمات شائعةمتشابهة (ظل/ضلَ, رؤيا/رؤية الخ.)",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/wrongWordInContext.txt"
        ),
        "احتمال كلمة متشابهة: هل تقصد <suggestion>$SUGGESTION</suggestion> بدلا من '$WRONGWORD'?",
        "احتمال كلمة متشابهة: هل تقصد <suggestion>$SUGGESTION</suggestion> (= $EXPLANATION_SUGGESTION) بدلا من '$WRONGWORD' (= $EXPLANATION_WRONGWORD)?",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::{Rule as _, TextLevelRule as _};

    #[test]
    fn arabic_natives_use_java_ids() {
        assert_eq!(
            arabic_double_punctuation_rule().id(),
            "ARABIC_DOUBLE_PUNCTUATION"
        );
        assert_eq!(
            arabic_comma_whitespace_rule().id(),
            "ARABIC_COMMA_PARENTHESIS_WHITESPACE"
        );
        assert_eq!(
            arabic_question_mark_whitespace_rule().id(),
            "ARABIC_QM_WHITESPACE"
        );
        assert_eq!(
            arabic_semicolon_whitespace_rule().id(),
            "ARABIC_SC_WHITESPACE"
        );
        assert_eq!(arabic_word_repeat_rule().id(), "ARABIC_WORD_REPEAT_RULE");
        assert_eq!(arabic_darja_rule().id(), "AR_DARJA_REPLACE");
        assert_eq!(arabic_diacritics_rule().id(), "AR_DIACRITICS_REPLACE");
        assert_eq!(arabic_simple_replace_rule().id(), "AR_SIMPLE_REPLACE");
        assert_eq!(arabic_homophones_rule().id(), "AR_HOMOPHONES_REPLACE");
        assert_eq!(arabic_redundancy_rule().id(), "AR_REDUNDANCY_REPLACE");
        assert_eq!(arabic_wordiness_rule().id(), "AR_WORDINESS_REPLACE");
        assert_eq!(arabic_word_coherency_rule().id(), "AR_WORD_COHERENCY");
        assert_eq!(
            arabic_wrong_word_in_context_rule().id(),
            "ARABIC_WRONG_WORD_IN_CONTEXT"
        );
    }

    #[test]
    fn arabic_word_coherency_flags_mixed_spelling() {
        use og_core::rule::TextLevelRule as _;
        let rule = arabic_word_coherency_rule();
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
        let s1 = mk("شؤون", 0);
        let s2 = mk("شئون", 6);
        let matches = rule.match_text("شؤون شئون", &[s1, s2]);
        assert_eq!(matches.len(), 1, "second variant should be flagged");
    }

    #[test]
    fn arabic_double_punctuation_flags_arabic_comma_pair() {
        use og_tokenizer::WordTokenizer as _;
        let tokenizer = og_tokenizer::DefaultWordTokenizer::new_splitting_hyphen();
        let tokens = tokenizer.tokenize("نعم ،، لقد");
        let mut sentence = og_core::AnalyzedSentence::new("نعم ،، لقد", 0, "نعم ،، لقد".len());
        sentence.set_tokens(
            tokens
                .iter()
                .map(|t| {
                    og_core::AnalyzedTokenReadings::new(og_core::AnalyzedToken::new(
                        t.text(),
                        t.start(),
                        t.end(),
                    ))
                })
                .collect(),
        );
        let matches = arabic_double_punctuation_rule().match_sentence(&sentence);
        assert!(!matches.is_empty(), "expected match on '،،'");
    }
}
