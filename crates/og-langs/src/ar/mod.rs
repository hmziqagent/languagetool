use crate::en::SimpleReplaceRule;
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
) -> SimpleReplaceRule {
    let mut rule = SimpleReplaceRule::new(id, name, "STYLE", "Style");
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
    )
}

pub fn arabic_simple_replace_rule() -> SimpleReplaceRule {
    replace_rule(
        "قاعدة تطابق الكلمات التي يجب تجنبها وتقترح تصويبا لها",
        "قاعدة تطابق الكلمات التي يجب تجنبها وتقترح تصويبا لها",
        "خطأ، يفضل أن  يقال: قل $suggestions",
        " أو  ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/replaces.txt"
        ),
    )
}

pub fn arabic_homophones_rule() -> SimpleReplaceRule {
    replace_rule(
        "كلمات متشابهة لفظا للتوضيح، يرجى التحقق منها مثل تشابه الظاء والضاد.",
        "كلمات متشابهة لفظا يرجى التحقق منها",
        "قل $suggestions",
        " أو  ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/homophones.txt"
        ),
    )
}

pub fn arabic_redundancy_rule() -> SimpleReplaceRule {
    replace_rule(
        "1. تكرار (عام)",
        "تكرار",
        "'$match' تعبير فيه تكرار.في بعض الحالات، يستحسن استعمال $suggestions",
        " أو ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/redundancies.txt"
        ),
    )
}

pub fn arabic_wordiness_rule() -> SimpleReplaceRule {
    replace_rule(
        "2. حشو(تعبير فيه تكرار)",
        "حشو (تعبير فيه تكرار)",
        "'$match' تعبير فيه حشو يفضل أن يقال $suggestions",
        " أو ",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/rules/ar/wordiness.txt"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::Rule as _;

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
