use og_compat::{GoldenCase, GoldenRunner};

fn cases() -> Vec<GoldenCase> {
    vec![
        GoldenCase::new(
            "ccs-verb-guard-allows-match",
            "BOD meeting is next week and we all have to jump for it.",
        )
        .expect_with("COMMA_COMPOUND_SENTENCE", 24, 4, vec![", and"]),
        GoldenCase::new(
            "ccs-marker-includes-leading-whitespace",
            "He gave the picture to his daughter and she was so surprised that she forgot to say \"thank you\".",
        )
        .expect_with("COMMA_COMPOUND_SENTENCE", 35, 4, vec![", and"]),
        GoldenCase::new(
            "it-is-2-chunk-antipattern-suppresses",
            "Its more than 4,000 employees, located in 60 offices around the world, are ready.",
        ),
        GoldenCase::new(
            "prp-have-vb-single-token-np-carries-e-chunk",
            "He has uses a space before the colon.",
        )
        .expect("PRP_HAVE_VB", 6, 5)
        .presence_only_marker(),
    ]
}

#[test]
fn golden_parity_cases_pass() {
    let runner = GoldenRunner::english();
    let report = runner.run(&cases());
    let failures: Vec<String> = report
        .results
        .iter()
        .filter(|r| r.status != og_compat::ComparisonStatus::Pass)
        .map(|r| format!("{} ({:?}): {}", r.rule_id, r.status, r.details))
        .collect();
    assert!(
        report.all_passed(),
        "{} of {} checks failed:\n{}",
        failures.len(),
        report.results.len(),
        failures.join("\n")
    );
}
