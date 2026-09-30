use crate::en::{CompoundRule, WordCoherencyRule};

pub fn swedish_compound_rule() -> CompoundRule {
    CompoundRule::for_language(
        "SV_COMPOUNDS",
        "Särskrivningar, t.ex. 'e mail' bör skrivas 'e-mail'",
        "Dessa ord skrivs samman med bindestreck.",
        "Dessa ord skrivs samman.",
        "Dessa ord skrivs samman med eller utan bindestreck.",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/sv/src/main/resources/org/languagetool/resource/sv/compounds.txt"
        ),
    )
}

pub fn swedish_word_coherency_rule() -> WordCoherencyRule {
    WordCoherencyRule::for_language(
        "SV_WORD_COHERENCY",
        "Enhetlig och konsekvent stavning av ord när det finns stavningsvarianter att välja på.",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/sv/src/main/resources/org/languagetool/rules/sv/coherency.txt"
        ),
    )
    .with_message_template(
        "Använd endast en av stavningsvarianterna '{word1}' och '{word2}' i en och samma text.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::{Rule as _, TextLevelRule as _};

    #[test]
    fn swedish_compound_rule_has_java_id() {
        assert_eq!(swedish_compound_rule().id(), "SV_COMPOUNDS");
    }

    #[test]
    fn swedish_word_coherency_rule_has_java_id() {
        assert_eq!(swedish_word_coherency_rule().id(), "SV_WORD_COHERENCY");
    }
}
