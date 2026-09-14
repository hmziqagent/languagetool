//! Data-backed perf/robustness test: compile the real LT English grammar.xml.
//!
//! Skipped when the upstream `languagetool/` checkout is not present next to
//! the repository root (CI runs without it).

use std::time::Instant;

fn english_grammar_xml() -> Option<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/rules/en/grammar.xml"
    );
    std::fs::read_to_string(path).ok()
}

#[test]
fn compile_real_english_grammar() {
    let Some(xml) = english_grammar_xml() else {
        eprintln!("skipping: languagetool/ checkout not present");
        return;
    };

    let start = Instant::now();
    let compiled = og_xml::compiler::XmlCompiler::new()
        .compile_file(&xml)
        .expect("grammar.xml must compile");
    let first = start.elapsed();

    // Rebuild: measures cross-engine reuse of compiled regexes.
    let start = Instant::now();
    let compiled2 = og_xml::compiler::XmlCompiler::new()
        .compile_file(&xml)
        .expect("grammar.xml must compile (2nd)");
    let second = start.elapsed();

    eprintln!(
        "grammar.xml: {} rules, first compile {:?}, second compile {:?}",
        compiled.rules.len(),
        first,
        second
    );
    assert_eq!(compiled.rules.len(), compiled2.rules.len());
    assert!(compiled.rules.len() > 5000, "expected 5.5k+ rules");
}
