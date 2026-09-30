use crate::en::{CompoundRule, SimpleReplaceRule};

pub fn romanian_replace_rule() -> SimpleReplaceRule {
    let mut rule = SimpleReplaceRule::new(
        "RO_SIMPLE_REPLACE",
        "Cuvinte sau grupuri de cuvinte incorecte sau ieșite din uz",
        "MISC",
        "Miscellaneous",
    )
    .with_message_template(
        "'$match' este incorect sau ieșit din uz, folosiți $suggestions",
        " sau ",
    );
    rule.load_data(include_str!(
        "../../../../languagetool/languagetool-language-modules/ro/src/main/resources/org/languagetool/rules/ro/replace.txt"
    ));
    rule
}

pub fn romanian_compound_rule() -> CompoundRule {
    CompoundRule::for_language(
        "RO_COMPOUND",
        "Greșeală de scriere (cuvinte scrise legat sau cu cratimă)",
        "Cuvântul se scrie cu cratimă.",
        "Cuvântul se scrie legat.",
        "Cuvântul se scrie legat sau cu cratimă.",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/ro/src/main/resources/org/languagetool/resource/ro/compounds.txt"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::{Rule as _, TextLevelRule as _};

    #[test]
    fn romanian_replace_rule_has_java_id() {
        assert_eq!(romanian_replace_rule().id(), "RO_SIMPLE_REPLACE");
    }

    #[test]
    fn romanian_compound_rule_has_java_id() {
        assert_eq!(romanian_compound_rule().id(), "RO_COMPOUND");
    }
}
