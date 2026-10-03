pub fn belarusian_simple_replace_rule() -> crate::pt::simple_replace2::SimpleReplace2Rule {
    let mut rule = crate::pt::simple_replace2::SimpleReplace2Rule::new(
        "BE_SIMPLE_REPLACE",
        "Пошук прастамоўяў і памылковых фраз",
        "Памылка?",
        "«$match» — памылка, нелітаратурны выраз або прастамоўе, правільна: $suggestions",
    )
    .with_separator(", ")
    .with_category(og_core::Category::new("MISC", "Агульныя правілы"))
    .with_issue_type(og_core::IssueType::Misspelling);
    rule.load_data(include_str!(
        "../../../../languagetool/languagetool-language-modules/be/src/main/resources/org/languagetool/rules/be/replace.txt"
    ));
    rule
}

pub fn belarusian_specific_case_rule() -> crate::specific_case::SpecificCaseRule {
    crate::specific_case::SpecificCaseRule::from_data(
        "BE_SPECIFIC_CASE",
        "Напісанне спецыяльных найменняў у верхнім або ніжнім рэгістры",
        "Вялікія літары",
        "Уласныя імёны і назвы пішуцца з вялікай літары.",
        "Калі гэта уласнае імя або назва, выкарыстоўвайце прапанаванае напісанне.",
        include_str!(
            "../../../../languagetool/languagetool-language-modules/be/src/main/resources/org/languagetool/resource/be/specific_case.txt"
        ),
    )
}

pub fn belarusian_paragraph_repeat_beginning_rule()
-> crate::paragraph_repeat_beginning::ParagraphRepeatBeginningRule {
    crate::paragraph_repeat_beginning::ParagraphRepeatBeginningRule::new(
        "Супадае з пачаткам абзаца",
        "Супадае з апошнім абзацам",
        "Стыль",
    )
}

#[cfg(test)]
mod tests {
    use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, rule::Rule as _};

    fn sentence(words: &[&str]) -> AnalyzedSentence {
        let text = words.join(" ");
        let mut s = AnalyzedSentence::new(&text, 0, text.len());
        let mut tokens = vec![AnalyzedTokenReadings::new(
            AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
        )];
        let mut offset = 0;
        for word in words {
            let start = offset;
            let end = start + word.len();
            offset = end + 1;
            tokens.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                *word, start, end,
            )));
        }
        s.set_tokens(tokens);
        s
    }

    #[test]
    fn specific_case_pins_the_java_example_pair() {
        let rule = super::belarusian_specific_case_rule();
        assert_eq!(rule.id(), "BE_SPECIFIC_CASE");
        let matches = rule.match_sentence(&sentence(&[
            "Вялікая",
            "айчынная",
            "Вайна",
            "—",
            "гэта",
            "спецыфічны",
            "тэрмін",
        ]));
        assert_eq!(
            matches.len(),
            1,
            "Java example must be flagged: {matches:?}"
        );
        assert_eq!(
            matches[0].replacements()[0].value(),
            "Вялікая Айчынная вайна"
        );
        assert_eq!(
            matches[0].message(),
            "Калі гэта уласнае імя або назва, выкарыстоўвайце прапанаванае напісанне."
        );
    }

    #[test]
    fn simple_replace_pins_the_java_example_pair() {
        let engine = crate::engine::LanguageEngine::shared_for_code("be").expect("be engine");
        let mut request = og_core::CheckRequest::new(
            "З большага, гэта быў добры дзень.",
            engine.language().clone(),
        );
        request.enabled_rules = vec!["BE_SIMPLE_REPLACE".to_string()];
        let matches = engine.check_with_request(&request);
        let hits: Vec<&og_core::RuleMatch> = matches
            .matches
            .iter()
            .filter(|m| m.rule().id() == "BE_SIMPLE_REPLACE")
            .collect();
        assert_eq!(hits.len(), 1, "Java example 'З більшага' must be flagged");
        assert!(
            hits[0]
                .replacements()
                .iter()
                .any(|r| r.value() == "Збольшага"),
            "sentence-start capital must carry into the suggestion"
        );
        assert_eq!(
            hits[0].message(),
            "«З большага» — памылка, нелітаратурны выраз або прастамоўе, правільна: <suggestion>збольшага</suggestion>"
        );
    }

    #[test]
    fn paragraph_repeat_beginning_uses_the_java_id_and_default_off() {
        use og_core::rule::TextLevelRule as _;
        let rule = super::belarusian_paragraph_repeat_beginning_rule();
        assert_eq!(rule.id(), "PARAGRAPH_REPEAT_BEGINNING_RULE");
        assert!(!rule.is_default_on());
        assert_eq!(rule.description(), "Супадае з пачаткам абзаца");
    }
}
