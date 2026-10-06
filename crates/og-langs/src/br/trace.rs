#[test]
fn probe_klanv_needs_the_token_level_match_transform() {
    let (rules, sentences, events) =
        crate::br::example_suite::trace_rule_matches("KLANV_PE_GLANVOCH", "Klañv pe klañvoc’h.");
    assert!(
        !rules.is_empty(),
        "KLANV_PE_GLANVOCH sub-rules must compile"
    );
    let tokens = sentences[0].non_whitespace_tokens();
    let klañvoc_h = tokens
        .iter()
        .find(|t| t.token().token() == "klañvoc’h")
        .expect("klañvoc’h token");
    assert!(
        klañvoc_h
            .readings()
            .iter()
            .any(|r| r.pos_tags().iter().any(|t| t == "J cmp")),
        "the dictionary carries the J cmp reading, so the residue is not tagger-side: {:?}",
        klañvoc_h.readings()
    );
    let backref_gate = events.iter().find(|e| {
        e.token == "klañvoc’h" && e.pattern_idx == 2 && e.cause.starts_with("backref-no=Some(0)")
    });
    assert!(
        backref_gate.is_some(),
        "the third element must fail exactly at the dropped <match no=\"0\"> transform \
         (og-xml parser.rs drops regexp_match/regexp_replace, backreference_matches then \
         falls back to text equality with element 1's 'Klañv'); got {events:#?}"
    );
    let matched = crate::br::example_suite::all_grammar_matches("Klañv pe klañvoc’h.");
    assert!(
        !matched.iter().any(|(id, _, _)| id == "KLANV_PE_GLANVOCH"),
        "flips when the shared engine gains the token-level <match no=\"0\"> transform"
    );
}

#[test]
fn probe_och_an_needs_the_token_level_match_transform() {
    let (rules, _, events) = crate::br::example_suite::trace_rule_matches(
        "OCH_AN",
        "Diaesoc’h diaesañ eo kavout labour.",
    );
    assert!(!rules.is_empty(), "OCH_AN sub-rules must compile");
    let backref_gate = events.iter().find(|e| {
        e.token == "diaesañ" && e.pattern_idx == 1 && e.cause.starts_with("backref-no=Some(0)")
    });
    assert!(
        backref_gate.is_some(),
        "the second element must fail exactly at the dropped <match no=\"0\"> transform \
         (Java compiles 'Diaesoc’h' through oc’h$→añ into 'Diaesañ'); got {events:#?}"
    );
    let matched =
        crate::br::example_suite::all_grammar_matches("Diaesoc’h diaesañ eo kavout labour.");
    assert!(
        !matched.iter().any(|(id, _, _)| id == "OCH_AN"),
        "flips when the shared engine gains the token-level <match no=\"0\"> transform"
    );
}

#[test]
fn probe_deiz_deiziad_month_with_u2019_is_trimmed_by_the_shared_date_filter() {
    let (_, sentences, _) = crate::br::example_suite::trace_rule_matches(
        "DEIZ_DEIZIAD",
        "D’ar Merc’her 6 a viz C’hwevrer 2014.",
    );
    let tokens = sentences[0].non_whitespace_tokens();
    let month = tokens
        .iter()
        .find(|t| t.token().token().contains("hwevrer"))
        .expect("month token");
    assert_eq!(
        month.token().token(),
        "C’hwevrer",
        "the tokenizer restores the U+2019 form that Java’s getMonth compares against"
    );
    let curly =
        crate::br::example_suite::all_grammar_matches("D’ar Merc’her 6 a viz C’hwevrer 2014.");
    assert!(
        !curly.iter().any(|(id, _, _)| id == "DEIZ_DEIZIAD"),
        "C’hwevrer (U+2019) month must currently stay dropped: Java’s \
         StringTools.trimSpecialCharacters (StringTools.java:91) keeps U+2019 via \
         (?U)\\p{{Punct}} (Unicode Pf), while the shared trim_special_chars helper \
         (pattern_rule.rs:4736) only keeps is_ascii_punctuation and strips it"
    );
    let ascii =
        crate::br::example_suite::all_grammar_matches("D’ar Merc’her 6 a viz Gwengolo 2014.");
    assert!(
        ascii.iter().any(|(id, _, _)| id == "DEIZ_DEIZIAD"),
        "the Gwengolo variant differs only in the month word, which the same \
         alternation element covers, so the pattern side is proven working and the \
         drop sits in the shared date filter's month parse: {ascii:?}"
    );
}
