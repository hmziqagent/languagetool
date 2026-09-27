use og_core::checker::{Chunker as _, Disambiguator, SentenceTokenizer, WordTokenizer};
use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, SentenceRange};
use og_rules::pattern_rule::PatternRuleEngine;
use og_tagger::{
    ChainedDisambiguator, ContextualDisambiguator, EnglishTagger, Tagger, XmlDisambiguator,
};
use og_xml::compiler::{CompiledRuleSet, XmlCompiler};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ExampleTestResult {
    pub rule_id: String,
    pub passed: bool,
    pub message: String,
    pub example_text: String,
    pub expected_type: String,
}

#[derive(Debug)]
pub struct TestRunSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<ExampleTestResult>,
}

impl TestRunSummary {
    pub fn is_all_passed(&self) -> bool {
        self.failed == 0
    }
}

struct TestWordTokenizer;

impl WordTokenizer for TestWordTokenizer {
    fn tokenize(&self, text: &str, offset: usize) -> Vec<AnalyzedTokenReadings> {
        let tokenizer = og_tokenizer::DefaultWordTokenizer::new();
        use og_tokenizer::WordTokenizer as _;
        let tokens = tokenizer.tokenize(text);
        tokens
            .into_iter()
            .map(|t| {
                let at = AnalyzedToken::new(t.text(), t.start() + offset, t.end() + offset);
                AnalyzedTokenReadings::new(at)
            })
            .collect()
    }
}

#[allow(dead_code)]
struct TestSentenceTokenizer;

impl SentenceTokenizer for TestSentenceTokenizer {
    fn split(&self, text: &str) -> Vec<SentenceRange> {
        use og_tokenizer::SentenceTokenizer as _;
        let sentences = match og_tokenizer::srx::srx_splitter("en") {
            Some(srx) => srx.split(text),
            None => og_tokenizer::DefaultSentenceSplitter::new().split(text),
        };
        sentences
            .into_iter()
            .map(|s| SentenceRange {
                start: s.start(),
                end: s.end(),
            })
            .collect()
    }
}

pub enum AnyTagger {
    English(EnglishTagger),
    German(og_tagger::GermanTagger),
}

impl AnyTagger {
    fn tag(&self, tokens: &[&str]) -> Vec<AnalyzedTokenReadings> {
        match self {
            AnyTagger::English(t) => Tagger::tag(t, tokens),
            AnyTagger::German(t) => Tagger::tag(t, tokens),
        }
    }
}

pub struct TestRunner {
    language: String,
    pattern_engine: PatternRuleEngine,
    tagger: AnyTagger,
    disambiguator: Option<Arc<dyn Disambiguator>>,
}

impl TestRunner {
    pub fn new() -> Self {
        Self::for_language("en")
    }

    pub fn for_language(code: &str) -> Self {
        let lt_modules =
            Path::new("/home/agent/opengrammar-rs/languagetool/languagetool-language-modules");
        let resource_dir = lt_modules.join(format!(
            "{code}/src/main/resources/org/languagetool/resource/{code}"
        ));
        let core_resource_dir = Path::new(
            "/home/agent/opengrammar-rs/languagetool/languagetool-core/src/main/resources/org/languagetool/resource",
        );

        let (tagger, disambiguator, pattern_engine) = std::thread::scope(|scope| {
            let tagger_h = scope.spawn(|| {
                if code == "de" && resource_dir.exists() {
                    og_tagger::GermanTagger::from_resource_dir(&resource_dir)
                        .map(AnyTagger::German)
                        .unwrap_or_else(|| AnyTagger::English(EnglishTagger::new()))
                } else if resource_dir.exists() {
                    AnyTagger::English(EnglishTagger::from_resource_dir(&resource_dir))
                } else {
                    AnyTagger::English(EnglishTagger::new())
                }
            });

            let disambiguator_h = scope.spawn(|| {
                let disambig_path = resource_dir.join("disambiguation.xml");
                match std::fs::read_to_string(&disambig_path) {
                    Ok(xml) => match XmlDisambiguator::from_xml(&xml) {
                        Ok(d) => {
                            eprintln!("Loaded {} disambiguation rules", d.rule_count());
                            let stages: Vec<Arc<dyn Disambiguator>> = if code == "en" {
                                vec![Arc::new(d), Arc::new(ContextualDisambiguator)]
                            } else {
                                vec![Arc::new(d)]
                            };
                            Some(Arc::new(ChainedDisambiguator::new(stages))
                                as Arc<dyn Disambiguator>)
                        }
                        Err(e) => {
                            eprintln!("Warning: failed to load disambiguation: {}", e);
                            None
                        }
                    },
                    Err(_) if code == "en" => {
                        Some(Arc::new(ContextualDisambiguator) as Arc<dyn Disambiguator>)
                    }
                    Err(_) => None,
                }
            });

            let engine_h = scope.spawn(|| {
                let support = if code == "en" && resource_dir.exists() && core_resource_dir.exists()
                {
                    og_rules::spelling_support::SpellingSupport::from_dirs(
                        &lt_modules.join("en/src/main/resources/org/languagetool/resource/en"),
                        core_resource_dir,
                    )
                } else if code == "de" && resource_dir.exists() && core_resource_dir.exists() {
                    let dictionary = og_tagger::morfologik::MorfologikDictionary::load(
                        &resource_dir.join("german.dict"),
                        &resource_dir.join("german.info"),
                    )
                    .map(|dict| {
                        let mut d = og_spell::Dictionary::new();
                        for word in dict.word_list() {
                            d.add_word(&word);
                        }
                        d
                    });
                    dictionary.map(|dictionary| {
                        og_rules::spelling_support::SpellingSupport::new_with(
                            dictionary,
                            core_resource_dir,
                            &resource_dir,
                        )
                    })
                } else {
                    None
                };
                match support {
                    Some(support) => {
                        PatternRuleEngine::new().with_spelling_support(std::sync::Arc::new(support))
                    }
                    None => PatternRuleEngine::new(),
                }
            });

            (
                tagger_h.join().unwrap(),
                disambiguator_h.join().unwrap(),
                engine_h.join().unwrap(),
            )
        });

        Self {
            language: code.to_string(),
            pattern_engine,
            tagger,
            disambiguator,
        }
    }

    fn tokenize_sentence(&self, text: &str) -> AnalyzedSentence {
        let tokenizer = TestWordTokenizer;
        let mut sentence = AnalyzedSentence::new(text, 0, text.len());
        let mut tokens = tokenizer.tokenize(text, 0);
        let start_token = AnalyzedTokenReadings::new(
            AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
        );
        tokens.insert(0, start_token);

        let token_texts: Vec<String> = tokens
            .iter()
            .skip(1)
            .map(|t| t.token().token().to_string())
            .collect();
        let text_refs: Vec<&str> = token_texts.iter().map(|s| s.as_str()).collect();
        let tagged = self.tagger.tag(&text_refs);

        for (i, tagged_atr) in tagged.into_iter().enumerate() {
            let token_idx = i + 1;
            if token_idx < tokens.len() {
                let readings = tagged_atr.into_readings();
                if !readings.is_empty() {
                    let primary = tokens[token_idx].token().clone();
                    let all_tags: Vec<String> = readings
                        .iter()
                        .flat_map(|r| r.pos_tags().to_vec())
                        .collect();
                    let updated_primary =
                        AnalyzedToken::new(primary.token(), primary.start(), primary.end())
                            .with_pos_tags(all_tags)
                            .with_lemma(readings[0].lemma().unwrap_or(primary.token()).to_string());

                    tokens[token_idx] =
                        AnalyzedTokenReadings::new(updated_primary).with_readings(readings);
                }
            }
        }

        sentence.set_tokens(tokens);

        let tokens = sentence.tokens_mut();
        for i in (0..tokens.len()).rev() {
            if !tokens[i].is_whitespace()
                && !tokens[i]
                    .token()
                    .pos_tags()
                    .contains(&"SENT_START".to_string())
            {
                if !tokens[i].has_pos_tag("SENT_END") {
                    tokens[i].add_sent_end();
                }
                break;
            }
        }

        if self.language == "en" {
            self.apply_chunking(&mut sentence);
        }

        if let Some(ref disambiguator) = self.disambiguator {
            disambiguator.disambiguate(&mut sentence);
        }

        if self.language == "de" {
            og_tagger::german_chunker::shared_german_chunker().add_chunk_tags(&mut sentence);
        }

        sentence
    }

    fn apply_chunking(&self, sentence: &mut AnalyzedSentence) {
        og_tagger::chunker::apply_chunk_tags(sentence);
    }

    fn extract_marker(text: &str) -> (String, Option<(usize, usize)>) {
        let start_marker = "<marker>";
        let end_marker = "</marker>";

        let start_idx = text.find(start_marker);
        let end_idx = text.find(end_marker);

        match (start_idx, end_idx) {
            (Some(s), Some(e)) => {
                let marker_start = s;
                let marker_end = e - start_marker.len();
                let clean = text[..s].to_string()
                    + &text[s + start_marker.len()..e]
                    + &text[e + end_marker.len()..];
                (clean, Some((marker_start, marker_end)))
            }
            _ => (text.to_string(), None),
        }
    }

    pub fn run_xml_example_tests(&self, xml: &str) -> TestRunSummary {
        let mut results = Vec::new();
        let compiler = XmlCompiler::new();

        let rule_set = match compiler.compile_file(xml) {
            Ok(rs) => rs,
            Err(e) => {
                results.push(ExampleTestResult {
                    rule_id: "PARSE_ERROR".to_string(),
                    passed: false,
                    message: format!("Failed to parse XML: {}", e),
                    example_text: String::new(),
                    expected_type: "parse".to_string(),
                });
                return TestRunSummary {
                    total: 1,
                    passed: 0,
                    failed: 1,
                    results,
                };
            }
        };

        self.run_rule_set_example_tests(&rule_set)
    }

    pub fn debug_sentence(&self, text: &str) -> AnalyzedSentence {
        self.tokenize_sentence(text)
    }

    pub fn run_rule_set_example_tests(&self, rule_set: &CompiledRuleSet) -> TestRunSummary {
        use rayon::prelude::*;

        let pairs: Vec<(&og_xml::compiler::CompiledRule, &og_xml::types::XmlExample)> = rule_set
            .rules
            .iter()
            .flat_map(|rule| rule.examples.iter().map(move |e| (rule, e)))
            .collect();

        let results: Vec<ExampleTestResult> = pairs
            .par_iter()
            .map(|(rule, example)| {
                let (clean_text, marker_range) = Self::extract_marker(&example.text);
                let sentence = self.tokenize_sentence(&clean_text);
                let matches = self.pattern_engine.match_rule(rule, &sentence);
                Self::evaluate_example(rule, example, &clean_text, marker_range, &matches)
            })
            .collect();

        let passed = results.iter().filter(|r| r.passed).count();
        let failed = results.len() - passed;
        TestRunSummary {
            total: results.len(),
            passed,
            failed,
            results,
        }
    }

    fn evaluate_example(
        rule: &og_xml::compiler::CompiledRule,
        example: &og_xml::types::XmlExample,
        clean_text: &str,
        marker_range: Option<(usize, usize)>,
        matches: &[og_core::RuleMatch],
    ) -> ExampleTestResult {
        match example.example_type {
            og_xml::types::XmlExampleType::Incorrect => {
                if matches.is_empty() {
                    ExampleTestResult {
                        rule_id: rule.id.clone(),
                        passed: false,
                        message: format!(
                            "Expected match on incorrect example but got none: '{}'",
                            clean_text
                        ),
                        example_text: clean_text.to_string(),
                        expected_type: "incorrect".to_string(),
                    }
                } else if let Some((m_start, m_end)) = marker_range {
                    let first_match = &matches[0];
                    let match_start = first_match.offset();
                    let match_end = match_start + first_match.length();
                    let marker_in_match = match_start <= m_start && match_end >= m_end;
                    if marker_in_match {
                        ExampleTestResult {
                            rule_id: rule.id.clone(),
                            passed: true,
                            message: format!(
                                "Match {}..{} contains marker {}..{}",
                                match_start, match_end, m_start, m_end
                            ),
                            example_text: clean_text.to_string(),
                            expected_type: "incorrect".to_string(),
                        }
                    } else {
                        ExampleTestResult {
                            rule_id: rule.id.clone(),
                            passed: false,
                            message: format!(
                                "Match {}..{} does not contain marker {}..{}",
                                match_start, match_end, m_start, m_end
                            ),
                            example_text: clean_text.to_string(),
                            expected_type: "incorrect".to_string(),
                        }
                    }
                } else {
                    ExampleTestResult {
                        rule_id: rule.id.clone(),
                        passed: true,
                        message: format!("Matched correctly: '{}'", clean_text),
                        example_text: clean_text.to_string(),
                        expected_type: "incorrect".to_string(),
                    }
                }
            }
            og_xml::types::XmlExampleType::Correct => {
                if !matches.is_empty() {
                    ExampleTestResult {
                        rule_id: rule.id.clone(),
                        passed: false,
                        message: format!(
                            "Expected no match on correct example but got {}: '{}'",
                            matches.len(),
                            clean_text
                        ),
                        example_text: clean_text.to_string(),
                        expected_type: "correct".to_string(),
                    }
                } else {
                    ExampleTestResult {
                        rule_id: rule.id.clone(),
                        passed: true,
                        message: format!("No match on correct example: '{}'", clean_text),
                        example_text: clean_text.to_string(),
                        expected_type: "correct".to_string(),
                    }
                }
            }
            og_xml::types::XmlExampleType::TriggersError => ExampleTestResult {
                rule_id: rule.id.clone(),
                passed: true,
                message: format!("Triggers error (known FP): '{}'", clean_text),
                example_text: clean_text.to_string(),
                expected_type: "triggers_error".to_string(),
            },
        }
    }

    pub fn run_xml_file_tests(&self, path: &Path) -> Result<TestRunSummary, String> {
        let xml = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        Ok(self.run_xml_example_tests(&xml))
    }

    pub fn run_pattern_test(
        &self,
        rule_id: &str,
        xml: &str,
        sentence_text: &str,
        expected_match: bool,
    ) -> ExampleTestResult {
        let compiler = XmlCompiler::new();
        let rule_set = match compiler.compile_file(xml) {
            Ok(rs) => rs,
            Err(e) => {
                return ExampleTestResult {
                    rule_id: rule_id.to_string(),
                    passed: false,
                    message: format!("Parse error: {}", e),
                    example_text: sentence_text.to_string(),
                    expected_type: "single".to_string(),
                };
            }
        };

        let rule = match rule_set.rules.iter().find(|r| r.id == rule_id) {
            Some(r) => r,
            None => {
                return ExampleTestResult {
                    rule_id: rule_id.to_string(),
                    passed: false,
                    message: format!("Rule '{}' not found", rule_id),
                    example_text: sentence_text.to_string(),
                    expected_type: "single".to_string(),
                };
            }
        };

        let sentence = self.tokenize_sentence(sentence_text);
        let matches = self.pattern_engine.match_rule(rule, &sentence);

        let matched = !matches.is_empty();
        ExampleTestResult {
            rule_id: rule_id.to_string(),
            passed: matched == expected_match,
            message: if matched == expected_match {
                format!("OK: '{}'", sentence_text)
            } else {
                format!(
                    "FAIL: '{}' - expected {}, got {}",
                    sentence_text,
                    if expected_match { "match" } else { "no match" },
                    if matched { "match" } else { "no match" }
                )
            },
            example_text: sentence_text.to_string(),
            expected_type: "single".to_string(),
        }
    }
}

impl Default for TestRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test_support {
    use super::*;
    use std::sync::OnceLock;

    const GRAMMAR_PATH: &str = "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/rules/en/grammar.xml";
    const DE_GRAMMAR_PATH: &str = "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/rules/de/grammar.xml";

    pub fn german_runner() -> &'static TestRunner {
        static RUNNER: OnceLock<TestRunner> = OnceLock::new();
        RUNNER.get_or_init(|| TestRunner::for_language("de"))
    }

    pub fn german_grammar_rules() -> &'static CompiledRuleSet {
        static GRAMMAR: OnceLock<CompiledRuleSet> = OnceLock::new();
        GRAMMAR.get_or_init(|| {
            let xml =
                std::fs::read_to_string(DE_GRAMMAR_PATH).expect("Failed to read de grammar.xml");
            XmlCompiler::new()
                .compile_file(&xml)
                .expect("Failed to compile de grammar.xml")
        })
    }

    pub fn german_grammar_available() -> bool {
        Path::new(DE_GRAMMAR_PATH).exists()
    }

    pub fn shared_runner() -> &'static TestRunner {
        static RUNNER: OnceLock<TestRunner> = OnceLock::new();
        RUNNER.get_or_init(TestRunner::new)
    }

    pub fn shared_grammar_rules() -> &'static CompiledRuleSet {
        static GRAMMAR: OnceLock<CompiledRuleSet> = OnceLock::new();
        GRAMMAR.get_or_init(|| {
            let xml = std::fs::read_to_string(GRAMMAR_PATH).expect("Failed to read grammar.xml");
            XmlCompiler::new()
                .compile_file(&xml)
                .expect("Failed to compile grammar.xml")
        })
    }

    pub fn grammar_available() -> bool {
        Path::new(GRAMMAR_PATH).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_xml(rules_body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                {}
            </category>
        </rules>"#,
            rules_body
        )
    }

    #[test]
    fn test_runner_incorrect_example_passes() {
        let runner = test_support::shared_runner();
        let xml = simple_xml(
            r#"
            <rule id="TEST1" name="Test">
                <pattern>
                    <token>their</token>
                    <token>is</token>
                </pattern>
                <message>Did you mean <suggestion>there is</suggestion>?</message>
                <example correction="there is">Their <marker>is</marker> a problem.</example>
                <example>There is no problem.</example>
            </rule>
        "#,
        );

        let summary = runner.run_xml_example_tests(&xml);
        assert_eq!(summary.total, 2);
        assert_eq!(
            summary.passed,
            2,
            "Failed: {:?}",
            summary
                .results
                .iter()
                .filter(|r| !r.passed)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_runner_correct_example_no_match() {
        let runner = test_support::shared_runner();
        let xml = simple_xml(
            r#"
            <rule id="TEST2" name="Test">
                <pattern>
                    <token>xyzzy</token>
                </pattern>
                <message>This should not match.</message>
                <example>This is a normal sentence.</example>
            </rule>
        "#,
        );

        let summary = runner.run_xml_example_tests(&xml);
        assert_eq!(summary.total, 1);
        assert!(summary.is_all_passed());
    }

    #[test]
    fn test_runner_regexp_rule() {
        let runner = test_support::shared_runner();
        let xml = simple_xml(
            r#"
            <rule id="TEST3" name="Test">
                <pattern>
                    <token regexp="yes">teh</token>
                </pattern>
                <message>Did you mean <suggestion>the</suggestion>?</message>
                <example correction="the"><marker>Teh</marker> quick brown fox.</example>
                <example>The quick brown fox.</example>
            </rule>
        "#,
        );

        let summary = runner.run_xml_example_tests(&xml);
        for r in &summary.results {
            if !r.passed {
                eprintln!("FAILED: {} - {}", r.rule_id, r.message);
            }
        }
        assert!(
            summary
                .results
                .iter()
                .any(|r| r.passed && r.expected_type == "correct")
        );
    }

    #[test]
    fn test_runner_multiple_rules() {
        let runner = test_support::shared_runner();
        let xml = simple_xml(
            r#"
            <rule id="RULE_A" name="Rule A">
                <pattern>
                    <token>hello</token>
                    <token>world</token>
                </pattern>
                <message>Hi!</message>
                <example><marker>Hello world</marker> test.</example>
                <example>Hi there.</example>
            </rule>
            <rule id="RULE_B" name="Rule B">
                <pattern>
                    <token>goodbye</token>
                    <token>world</token>
                </pattern>
                <message>Bye!</message>
                <example><marker>Goodbye world</marker> test.</example>
                <example>See you.</example>
            </rule>
        "#,
        );

        let summary = runner.run_xml_example_tests(&xml);
        assert_eq!(summary.total, 4, "Should have 4 examples");
    }

    #[test]
    fn test_runner_marker_extraction() {
        let (clean, range) = TestRunner::extract_marker("Hello <marker>world</marker> test");
        assert_eq!(clean, "Hello world test");
        assert_eq!(range, Some((6, 11)));
    }

    #[test]
    fn test_runner_no_marker() {
        let (clean, range) = TestRunner::extract_marker("Hello world test");
        assert_eq!(clean, "Hello world test");
        assert!(range.is_none());
    }

    #[test]
    fn test_runner_single_pattern_test() {
        let runner = test_support::shared_runner();
        let xml = simple_xml(
            r#"
            <rule id="SINGLE_TEST" name="Single">
                <pattern>
                    <token>foo</token>
                    <token>bar</token>
                </pattern>
                <message>Test</message>
            </rule>
        "#,
        );

        let result = runner.run_pattern_test("SINGLE_TEST", &xml, "foo bar baz", true);
        assert!(result.passed, "{}", result.message);

        let result = runner.run_pattern_test("SINGLE_TEST", &xml, "baz qux", false);
        assert!(result.passed, "{}", result.message);
    }

    #[test]
    fn test_runner_xml_parse_error() {
        let runner = test_support::shared_runner();
        let xml = r#"<?xml version="1.0"?><rules><category id="X"><rule id="TEST"><pattern>"#;
        let summary = runner.run_xml_example_tests(xml);
        assert!(summary.total <= 1);
    }

    #[test]
    fn test_runner_rule_not_found() {
        let runner = test_support::shared_runner();
        let xml = simple_xml(
            r#"
            <rule id="EXISTS" name="Exists">
                <pattern><token>test</token></pattern>
                <message>Test</message>
            </rule>
        "#,
        );

        let result = runner.run_pattern_test("NONEXISTENT", &xml, "test", true);
        assert!(!result.passed);
        assert!(result.message.contains("not found"));
    }

    #[test]
    fn test_runner_negate_token() {
        let runner = test_support::shared_runner();
        let xml = simple_xml(
            r#"
            <rule id="NEG_TEST" name="Negate">
                <pattern>
                    <token negate="yes">good</token>
                    <token>morning</token>
                </pattern>
                <message>Not good morning.</message>
                <example type="incorrect"><marker>Bad morning</marker> today.</example>
                <example>Good morning everyone.</example>
            </rule>
        "#,
        );

        let summary = runner.run_xml_example_tests(&xml);
        for r in &summary.results {
            if !r.passed {
                eprintln!(
                    "FAILED: {} ({}) - {}",
                    r.rule_id, r.expected_type, r.message
                );
            }
        }
        assert!(summary.is_all_passed(), "Negate token test failed");
    }

    #[test]
    #[ignore]
    fn test_load_real_english_grammar_xml() {
        if !test_support::grammar_available() {
            eprintln!("Skipping: English grammar.xml not found");
            return;
        }

        let rule_set = test_support::shared_grammar_rules();

        let total = rule_set.rules.len();
        eprintln!("Loaded {} rules from English grammar.xml", total);

        assert!(total > 4000, "Expected 4000+ rules, got {}", total);

        let with_antipatterns = rule_set
            .rules
            .iter()
            .filter(|r| !r.antipatterns.is_empty())
            .count();
        let with_marker = rule_set
            .rules
            .iter()
            .filter(|r| r.pattern.marker_start.is_some())
            .count();
        let with_exceptions = rule_set
            .rules
            .iter()
            .filter(|r| r.pattern.tokens.iter().any(|t| !t.exceptions.is_empty()))
            .count();
        let with_skip = rule_set
            .rules
            .iter()
            .filter(|r| r.pattern.tokens.iter().any(|t| t.skip != 0))
            .count();
        let with_min_max = rule_set
            .rules
            .iter()
            .filter(|r| {
                r.pattern
                    .tokens
                    .iter()
                    .any(|t| t.min.is_some() || t.max.is_some())
            })
            .count();
        let with_postag = rule_set
            .rules
            .iter()
            .filter(|r| r.pattern.tokens.iter().any(|t| t.postag.is_some()))
            .count();

        eprintln!("Rules with antipatterns: {}", with_antipatterns);
        eprintln!("Rules with pattern markers: {}", with_marker);
        eprintln!("Rules with exceptions: {}", with_exceptions);
        eprintln!("Rules with skip: {}", with_skip);
        eprintln!("Rules with min/max: {}", with_min_max);
        eprintln!("Rules with postag: {}", with_postag);

        assert!(
            with_antipatterns > 1000,
            "Expected 1000+ rules with antipatterns, got {}",
            with_antipatterns
        );
        assert!(
            with_exceptions > 500,
            "Expected 500+ rules with exceptions, got {}",
            with_exceptions
        );
        assert!(
            with_skip > 100,
            "Expected 100+ rules with skip, got {}",
            with_skip
        );
    }

    #[test]
    #[ignore]
    fn test_real_german_grammar_examples() {
        if !test_support::german_grammar_available() {
            eprintln!("Skipping: German grammar.xml not found");
            return;
        }

        let rules_handle = std::thread::spawn(test_support::german_grammar_rules);
        let runner = test_support::german_runner();
        let rules = rules_handle.join().expect("grammar compile thread");
        let summary = runner.run_rule_set_example_tests(rules);

        eprintln!("Total German examples tested: {}", summary.total);
        eprintln!("Passed: {}", summary.passed);
        eprintln!("Failed: {}", summary.failed);
        eprintln!(
            "Pass rate: {:.1}%",
            if summary.total > 0 {
                100.0 * summary.passed as f64 / summary.total as f64
            } else {
                0.0
            }
        );

        let mut correct_fp = 0;
        let mut no_match = 0;
        let mut marker = 0;
        for r in &summary.results {
            if r.passed {
                continue;
            }
            if r.message.contains("correct -") || r.message.contains("Expected no match") {
                correct_fp += 1;
            } else if r.message.contains("marker") {
                marker += 1;
            } else {
                no_match += 1;
            }
        }
        eprintln!(
            "  Incorrect (no match): {no_match}\n  Correct (unexpected match): {correct_fp}\n  Incorrect (marker mismatch): {marker}"
        );

        let mut fail_file = String::new();
        for r in &summary.results {
            if !r.passed {
                fail_file.push_str(&format!("FAIL: [{}] {}\n", r.rule_id, r.message));
            }
        }
        let _ = std::fs::write("/tmp/test_failures_de.txt", fail_file);

        eprintln!("Summary: {}/{} passed", summary.passed, summary.total);
    }

    #[test]
    #[ignore]
    fn test_real_english_grammar_examples() {
        if !test_support::grammar_available() {
            eprintln!("Skipping: English grammar.xml not found");
            return;
        }

        let rules_handle = std::thread::spawn(test_support::shared_grammar_rules);
        let runner = test_support::shared_runner();
        let rules = rules_handle.join().expect("grammar compile thread");
        let summary = runner.run_rule_set_example_tests(rules);

        eprintln!("Total examples tested: {}", summary.total);
        eprintln!("Passed: {}", summary.passed);
        eprintln!("Failed: {}", summary.failed);
        eprintln!(
            "Pass rate: {:.1}%",
            if summary.total > 0 {
                100.0 * summary.passed as f64 / summary.total as f64
            } else {
                0.0
            }
        );

        let mut fail_by_reason: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for r in &summary.results {
            if !r.passed {
                let reason = if r.message.contains("POS") {
                    "pos_tag".to_string()
                } else if r.message.contains("match") {
                    "no_match".to_string()
                } else if r.message.contains("marker") {
                    "marker_mismatch".to_string()
                } else {
                    "other".to_string()
                };
                *fail_by_reason.entry(reason).or_insert(0) += 1;
            }
        }
        eprintln!("\nFailure categories:");
        for (reason, count) in &fail_by_reason {
            eprintln!("  {}: {}", reason, count);
        }

        let mut rule_fail_counts: std::collections::HashMap<String, (usize, usize, usize)> =
            std::collections::HashMap::new();
        for r in &summary.results {
            if !r.passed {
                let entry = rule_fail_counts
                    .entry(r.rule_id.clone())
                    .or_insert((0, 0, 0));
                if r.expected_type == "incorrect" {
                    entry.0 += 1;
                } else {
                    entry.1 += 1;
                }
                if r.message.contains("marker") {
                    entry.2 += 1;
                }
            }
        }
        let mut sorted_fails: Vec<_> = rule_fail_counts.iter().collect();
        sorted_fails.sort_by_key(|(_, counts)| std::cmp::Reverse(counts.0 + counts.1));
        eprintln!("\nTop 30 failing rules:");
        for (rule_id, (incorrect, correct, marker)) in sorted_fails.iter().take(30) {
            eprintln!(
                "  {}: {} no-match, {} false-positive, {} marker-mismatch",
                rule_id, incorrect, correct, marker
            );
        }

        let failures: Vec<_> = summary
            .results
            .iter()
            .filter(|r| !r.passed)
            .take(100)
            .collect();
        for f in &failures {
            eprintln!("FAIL: [{}] {} - {}", f.rule_id, f.expected_type, f.message);
        }

        if let Ok(all_failures) = std::fs::File::create("/tmp/test_failures_all.txt") {
            use std::io::Write;
            let mut out = std::io::BufWriter::new(all_failures);
            for r in &summary.results {
                if !r.passed {
                    writeln!(
                        out,
                        "FAIL: [{}] {} - {}",
                        r.rule_id, r.expected_type, r.message
                    )
                    .ok();
                }
            }
        }

        let incorrect_no_match = summary
            .results
            .iter()
            .filter(|r| {
                !r.passed && r.expected_type == "incorrect" && r.message.contains("Expected match")
            })
            .count();
        let correct_unexpected = summary
            .results
            .iter()
            .filter(|r| !r.passed && r.expected_type == "correct")
            .count();
        let incorrect_marker = summary
            .results
            .iter()
            .filter(|r| {
                !r.passed
                    && r.expected_type == "incorrect"
                    && r.message.contains("does not contain marker")
            })
            .count();
        eprintln!("\nFailure breakdown:");
        eprintln!("  Incorrect (no match): {}", incorrect_no_match);
        eprintln!("  Correct (unexpected match): {}", correct_unexpected);
        eprintln!("  Incorrect (marker mismatch): {}", incorrect_marker);

        let pass_rate = if summary.total > 0 {
            summary.passed as f64 / summary.total as f64
        } else {
            0.0
        };
        assert!(
            pass_rate > 0.10,
            "Expected at least 10% pass rate, got {:.1}%",
            pass_rate * 100.0
        );
    }

    #[test]
    #[ignore]
    fn test_it_is_rule_its_cool_man() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rulegroup id="IT_IS" name="Test">
                    <rule>
                        <pattern>
                            <marker>
                                <token>its</token>
                            </marker>
                            <token postag="JJ">
                                <exception regexp="yes">first|second|third|last</exception>
                            </token>
                            <token>man</token>
                        </pattern>
                        <message>Did you mean <suggestion>it's</suggestion>?</message>
                        <example correction="It's"><marker>Its</marker> cool man!</example>
                    </rule>
                </rulegroup>
            </category>
        </rules>"#;

        let runner = test_support::shared_runner();
        let summary = runner.run_xml_example_tests(xml);

        let sentence = runner.tokenize_sentence("Its cool man!");
        let nwt: Vec<_> = sentence.non_whitespace_tokens();
        eprintln!("Non-whitespace tokens for 'Its cool man!':");
        for (i, token) in nwt.iter().enumerate() {
            eprintln!(
                "  [{}] '{}' pos={:?} has_JJ={}",
                i,
                token.token().token(),
                token.token().pos_tags(),
                token.has_pos_tag("JJ")
            );
        }

        let compiler = og_xml::compiler::XmlCompiler::new();
        let rule_set = compiler.compile_file(xml).unwrap();
        eprintln!("Compiled {} rules", rule_set.rules.len());
        for rule in &rule_set.rules {
            eprintln!(
                "Rule {} has {} pattern tokens",
                rule.id,
                rule.pattern.tokens.len()
            );
            for (i, t) in rule.pattern.tokens.iter().enumerate() {
                eprintln!(
                    "  token[{}]: text={:?} postag={:?} negate={}",
                    i, t.text, t.postag, t.negate
                );
            }
            let matches = runner.pattern_engine.match_rule(rule, &sentence);
            eprintln!("  matches: {} matches found", matches.len());
        }

        for r in &summary.results {
            if !r.passed {
                eprintln!("FAILED: {}", r.message);
            }
        }
        assert!(
            summary.is_all_passed(),
            "IT_IS[10] rule should match 'Its cool man!'"
        );
    }

    #[test]
    #[ignore]
    fn test_debug_pipeline_stages() {
        let runner = test_support::shared_runner();
        for text in [
            "We want go to the cinema.",
            "You will see what an American looks like.",
        ] {
            let tokenizer = TestWordTokenizer;
            let mut sentence = AnalyzedSentence::new(text, 0, text.len());
            let mut tokens = tokenizer.tokenize(text, 0);
            let start_token = AnalyzedTokenReadings::new(
                AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
            );
            tokens.insert(0, start_token);
            let token_texts: Vec<String> = tokens
                .iter()
                .skip(1)
                .map(|t| t.token().token().to_string())
                .collect();
            let text_refs: Vec<&str> = token_texts.iter().map(|s| s.as_str()).collect();
            let tagged = runner.tagger.tag(&text_refs);
            eprintln!("== '{}' AFTER TAG:", text);
            for (i, atr) in tagged.iter().enumerate() {
                eprintln!(
                    "  [{}] {:?} readings={:?}",
                    i,
                    text_refs.get(i),
                    atr.readings()
                        .iter()
                        .map(|r| r.pos_tags().to_vec())
                        .collect::<Vec<_>>()
                );
            }
            let merged: Vec<AnalyzedTokenReadings> = tagged
                .iter()
                .map(|atr| {
                    let at = atr.token().clone();
                    AnalyzedTokenReadings::new(at).with_readings(atr.readings().to_vec())
                })
                .collect();
            sentence.set_tokens(merged);
            if let Some(ref d) = runner.disambiguator {
                d.disambiguate(&mut sentence);
            }
            eprintln!("== '{}' AFTER DISAMBIG:", text);
            for t in sentence.tokens() {
                if t.is_whitespace() {
                    continue;
                }
                eprintln!(
                    "  {:?} readings={:?} lemmas={:?}",
                    t.token().token(),
                    t.readings()
                        .iter()
                        .map(|r| r.pos_tags().to_vec())
                        .collect::<Vec<_>>(),
                    t.readings()
                        .iter()
                        .map(|r| r.lemma().map(|l| l.to_string()))
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn test_debug_matching_subrules() {
        let runner = test_support::shared_runner();
        let sentences = [
            "RWE said it has open positions",
            "Do you have change?",
            "we have open positions sometimes.",
        ];
        let rule_set = test_support::shared_grammar_rules();
        for text in sentences {
            let sentence = runner.tokenize_sentence(text);
            let mut matched = 0;
            for rule in &rule_set.rules {
                if rule.id != "HAVE_PART_AGREEMENT" {
                    continue;
                }
                let matches = runner.pattern_engine.match_rule(rule, &sentence);
                if !matches.is_empty() {
                    matched += 1;
                    eprintln!(
                        "MATCH [{}] sub={:?} on '{}' at {}..{}",
                        rule.id,
                        rule.sub_id,
                        text,
                        matches[0].offset(),
                        matches[0].offset() + matches[0].length()
                    );
                    for (i, t) in rule.pattern.tokens.iter().enumerate() {
                        eprintln!(
                            "  token[{}]: text={:?} postag={:?} chunk={:?} chunk_re={:?}",
                            i, t.text, t.postag, t.chunk, t.chunk_re
                        );
                    }
                }
            }
            if matched == 0 {
                eprintln!("no match on '{}'", text);
            }
        }
    }

    #[test]
    #[ignore]
    fn test_debug_a_nns_fps() {
        let runner = test_support::shared_runner();
        let sentences = vec![
            "I want spend time with my friends.",
            "I would like spend time with my friends.",
        ];

        for text in &sentences {
            let sentence = runner.tokenize_sentence(text);
            let nwt: Vec<_> = sentence.non_whitespace_tokens();
            eprintln!("\n'{}':", text);
            for (i, token) in nwt.iter().enumerate() {
                let chunk = token.chunk().unwrap_or("-");
                eprintln!(
                    "  [{}] {:15} pos={:?} chunk={}",
                    i,
                    format!("'{}'", token.token().token()),
                    token.token().pos_tags(),
                    chunk
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn test_debug_pos_tags_for_failures() {
        let runner = test_support::shared_runner();
        let sentences = vec![
            "Grammatical errors is bad.",
            "Restart PHP and try replicate your issue.",
            "I would like test the tool.",
            "We want go to the cinema.",
            "I love listen to music.",
            "RWE said it has open positions",
            "Do you have change?",
            "We want test it.",
            "Grammatical errors always makes your text look bad.",
            "Heat rates has no impact on numbers.",
            "I want spend time with my friends.",
            "Can I tell if its using the repeater?",
            "He is friend.",
            "We have visited the client on 7 October 2025.",
            "I would love listen your contribution.",
            "He never looks the front door.",
            "This sound slike an error.",
            "We want test it.",
            "But its much better like this.",
            "I think its much better now.",
            "Its much easier.",
            "Let me know when its under pressure.",
            "It's to early.",
            "He spent 10% to much.",
            "I better stop before I go to far.",
            "The dogs barks loudly.",
            "I would like see us make this work.",
            "They don't want let me learn.",
            "I want spend time with my friends.",
            "Restart PHP and try replicate your issue.",
            "I would like test the tool.",
            "We want go to the cinema.",
            "I love listen to music.",
            "RWE said it has open positions",
            "Do you have change?",
        ];

        for text in &sentences {
            let sentence = runner.tokenize_sentence(text);
            let nwt: Vec<_> = sentence.non_whitespace_tokens();
            eprintln!("\n'{}':", text);
            for (i, token) in nwt.iter().enumerate() {
                let chunk = token.chunk().unwrap_or("-");
                eprintln!(
                    "  [{}] {:15} pos={:?} chunk={}",
                    i,
                    format!("'{}'", token.token().token()),
                    token.token().pos_tags(),
                    chunk
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn test_been_part_agreement_debug() {
        let runner = test_support::shared_runner();

        let sentence = runner.tokenize_sentence("I'm been prepared.");
        let _nwt: Vec<_> = sentence.non_whitespace_tokens();

        let compiled_rule_set = test_support::shared_grammar_rules();

        let to_too_rules: Vec<_> = compiled_rule_set
            .rules
            .iter()
            .filter(|r| r.id == "TO_TOO")
            .collect();
        eprintln!("\nTO_TOO rules found: {}", to_too_rules.len());
        for (i, rule) in to_too_rules.iter().enumerate() {
            eprintln!("  Rule {}: {} pattern tokens", i, rule.pattern.tokens.len());
            let sentence2 = runner.tokenize_sentence("It's to early.");
            let matches = runner.pattern_engine.match_rule(rule, &sentence2);
            if !matches.is_empty() {
                eprintln!("    MATCHED! {} matches", matches.len());
            }
        }

        let been_rules: Vec<_> = compiled_rule_set
            .rules
            .iter()
            .filter(|r| r.id.contains("BEEN_PART"))
            .collect();
        assert!(!been_rules.is_empty(), "Should find BEEN_PART rules");

        let mut any_matched = false;
        for rule in been_rules.iter() {
            let matches = runner.pattern_engine.match_rule(rule, &sentence);
            if !matches.is_empty() {
                any_matched = true;
            }
        }
        assert!(
            any_matched,
            "At least one BEEN_PART rule should match 'I'm been prepared.'"
        );
    }

    #[test]
    #[ignore]
    fn test_to_too_inflected_matching() {
        let runner = test_support::shared_runner();
        let compiled = test_support::shared_grammar_rules();

        let test_cases = vec![
            ("This is to funny.", true),
            ("It is to small", true),
            ("It's to early.", true),
            ("They were to good for you to miss.", true),
            ("He spent 10% to much.", true),
        ];

        let to_too_rules: Vec<_> = compiled.rules.iter().filter(|r| r.id == "TO_TOO").collect();

        for (text, should_match) in &test_cases {
            let sentence = runner.tokenize_sentence(text);
            let any_matched = to_too_rules
                .iter()
                .any(|rule| !runner.pattern_engine.match_rule(rule, &sentence).is_empty());
            assert_eq!(
                any_matched, *should_match,
                "TO_TOO match for '{}' expected {}, got {}",
                text, should_match, any_matched
            );
        }
    }

    #[test]
    #[ignore]
    fn test_common_pos_tag_coverage() {
        let runner = test_support::shared_runner();

        fn tags_for(runner: &TestRunner, word: &str) -> Vec<String> {
            let sentence = runner.tokenize_sentence(word);
            let nwt: Vec<_> = sentence.non_whitespace_tokens();
            let token = nwt
                .iter()
                .find(|t| t.token().token() != "<S>")
                .expect("should have a non-<S> token");
            let mut tags: Vec<String> = token
                .readings()
                .iter()
                .flat_map(|r| r.pos_tags().to_vec())
                .collect();
            tags.sort();
            tags.dedup();
            tags
        }

        fn check(runner: &TestRunner, word: &str, expected_tags: &[&str]) -> Vec<String> {
            let actual = tags_for(runner, word);

            let mut missing: Vec<&str> = expected_tags
                .iter()
                .filter(|t| !actual.iter().any(|a| a == **t))
                .copied()
                .collect();
            missing.sort();

            let status = if missing.is_empty() { "OK" } else { "MISSING" };
            eprintln!(
                "  {:20} expected={:?}  actual={:?}  {}",
                format!("'{}'", word),
                expected_tags,
                actual,
                status,
            );
            if !missing.is_empty() {
                eprintln!("                         missing tags: {:?}", missing);
            }

            actual
        }

        eprintln!("\n=== VBZ (3rd person singular present) ===");
        for word in &["needs", "wants", "likes", "tries"] {
            check(runner, word, &["VBZ"]);
        }

        eprintln!("\n=== VB (base form verb) ===");
        for word in &["need", "want", "like", "try"] {
            check(runner, word, &["VB"]);
        }
        for word in &["go", "make", "contact", "solve", "mount"] {
            check(runner, word, &["VB"]);
        }
        for word in &["compete", "bring", "send", "extend"] {
            check(runner, word, &["VB"]);
        }

        eprintln!("\n=== VB + NN (verb and noun) ===");
        for word in &["open", "reopen", "close"] {
            check(runner, word, &["VB", "NN"]);
        }
        for word in &["help", "support", "sleep", "exercise"] {
            check(runner, word, &["NN", "VB"]);
        }
        for word in &["love", "fear", "care"] {
            check(runner, word, &["VB", "NN"]);
        }

        eprintln!("\n=== JJ (adjective) ===");
        for word in &[
            "cool",
            "awesome",
            "great",
            "easy",
            "happy",
            "perfect",
            "recommendable",
        ] {
            check(runner, word, &["JJ"]);
        }
        for word in &["okay", "worth"] {
            check(runner, word, &["JJ"]);
        }

        eprintln!("\n=== VBG (gerund / present participle) ===");
        for word in &["happening", "going", "writing"] {
            check(runner, word, &["VBG"]);
        }

        eprintln!("\n=== VBN (past participle) ===");
        for word in &["done", "been", "given"] {
            check(runner, word, &["VBN"]);
        }

        eprintln!("\n=== NNS (plural noun) ===");
        for word in &["phenomena", "criteria", "stimuli"] {
            check(runner, word, &["NNS"]);
        }
        for word in &["freshmen", "women", "gentlemen"] {
            check(runner, word, &["NNS"]);
        }

        eprintln!("\n=== JJR / JJ (comparative and base adjective) ===");
        for word in &["better", "easier"] {
            check(runner, word, &["JJR"]);
        }
        {
            let word = &"expensive";
            check(runner, word, &["JJ"]);
        }
    }
}

#[test]
#[ignore]
fn test_debug_have_part_agreement_fp() {
    let runner = test_support::shared_runner();
    let sentences = vec![
        "Do you have change?",
        "Like other insects, beetles have open circulatory systems, based on hemolymph rather than blood.",
        "RWE said it has open positions",
        "We have soap.",
        "We do have soap.",
        "We do not have soap.",
        "I have contact with aliens.",
        "Do you have Telegram?",
        "The family that my friend had was his cousin and his sister.",
        "In the Sahara region some oases have palm trees.",
        "It had feature limitations such as...",
        "A rabbit has long ears.",
        "The tool my teacher had was archaic.",
        "Do you have bicycle in your garage?",
        "For any additional questions that you may have please contact me.",
        "We have only option 2 available.",
        "We have only option #2 available.",
        "I don't have job We need all the food we can get.",
        "The block of wood had saw marks on it.",
        "The sun had rose before I awoke.",
        "The block of wood has saw marks on it.",
        "For example, monarchical societies often had a system of \"social ranks\" which were collectivist because the social rank one had or did not have was more important than his or her individual will, and the specific rank in question could only be overridden in very limited cases.",
        "By the mid-10th century, the Samanid dynasty had crumble in the face of attacks from Turkish tribes to the north and from the Ghaznavids, a rising Turkic Muslim dynasty in Afghanistan.",
        "What I'm trying to have are papers on the Silver Leaf resort thingy.",
        "What I like to have are papers on the Silver Leaf resort thingy.",
        "What I really would like to have are papers on the Silver Leaf resort thingy.",
        "The options that my brother should have are the only ...",
        "The options my brother should have are the only ...",
        "The options he should have are the only ...",
        "we have open positions sometimes.",
        "There are a few I have open questions about or am waiting to receive copies of the contract.",
        "I am taking a scuba diving class and I have open water dives on both Saturday and Sunday.",
        "we don't have open access to the transmission grid",
        "In this case I think 100 is necessary because once a strike has open interest, we must continue to support it.",
        "If we restrict to strikes with a lower delta, we face the problem of not offering enough strikes and not making a market in options that have open EOL interest that have moved closer to the money.",
        "When the basis vectors have norm 1, the coordinate functionals e*n have...",
        "the coordinate functionals e*n have norm 2C in the dual of X.",
    ];

    for text in &sentences {
        let sentence = runner.tokenize_sentence(text);
        let nwt: Vec<_> = sentence.non_whitespace_tokens();
        eprintln!("\n'{}':", text);
        for (i, token) in nwt.iter().enumerate() {
            let chunk = token.chunk().unwrap_or("-");
            eprintln!(
                "  [{}] {:15} pos={:?} chunk={}",
                i,
                format!("'{}'", token.token().token()),
                token.token().pos_tags(),
                chunk
            );
        }
    }
}

#[test]
#[ignore]
fn test_debug_missing_to_before_verb() {
    let runner = test_support::shared_runner();
    let sentences = vec![
        "We need test it first.",
        "I would like see us make this work.",
        "I like go to the pool.",
        "He needs go there.",
        "I want spend time with my friends.",
        "They want improve his skills.",
        "Try get in our heads.",
        "I like hang out with the crew.",
        "Installer will detect PHP 7 and try install.",
        "He needs go",
    ];

    for text in &sentences {
        let sentence = runner.tokenize_sentence(text);
        let nwt: Vec<_> = sentence.non_whitespace_tokens();
        eprintln!("\n'{}':", text);
        for (i, token) in nwt.iter().enumerate() {
            let chunk = token.chunk().unwrap_or("-");
            eprintln!(
                "  [{}] {:15} pos={:?} chunk={}",
                i,
                format!("'{}'", token.token().token()),
                token.token().pos_tags(),
                chunk
            );
        }
    }
}

#[test]
#[ignore]
fn test_debug_chunking_with_whitespace() {
    let runner = test_support::shared_runner();
    let sentence = runner.tokenize_sentence("The client is here.");
    let tokens = sentence.tokens();
    eprintln!(
        "ALL tokens for 'The client is here.' ({} total):",
        tokens.len()
    );
    for (i, token) in tokens.iter().enumerate() {
        let tags = token.token().pos_tags();
        let tag_str = if tags.is_empty() {
            "(none)".to_string()
        } else {
            format!("{:?}", tags)
        };
        let ws = if token.is_whitespace() { " [WS]" } else { "" };
        eprintln!(
            "  [{}] {:10} pos={} chunk={}{}",
            i,
            format!(
                "'{}'",
                token.token().token().replace('\n', "\\n").replace(' ', "·")
            ),
            tag_str,
            token.chunk().unwrap_or("-"),
            ws
        );
    }
}

#[test]
#[ignore]
fn test_it_is_2_debug() {
    let runner = test_support::shared_runner();
    let sentences = vec![
        "But its much better like this.",
        "I think its much better now.",
        "Its much easier.",
    ];

    for text in &sentences {
        let sentence = runner.tokenize_sentence(text);
        eprintln!("\n--- '{}' ---", text);
        let nwt: Vec<_> = sentence.non_whitespace_tokens();
        for (i, token) in nwt.iter().enumerate() {
            eprintln!(
                "  [{}] {:15} pos={:?} chunk={}",
                i,
                format!("'{}'", token.token().token()),
                token.token().pos_tags(),
                token.chunk().unwrap_or("-")
            );
        }
    }

    let rule_set = test_support::shared_grammar_rules();

    let text = "But its much better like this.";
    let sentence = runner.tokenize_sentence(text);
    let mut rule_num = 0;
    for rule in &rule_set.rules {
        if rule.id == "IT_IS_2" {
            rule_num += 1;
            let matches = runner.pattern_engine.match_rule(rule, &sentence);
            eprintln!("\nIT_IS_2[{}]: {} matches", rule_num, matches.len());
            if !matches.is_empty() {
                for m in &matches {
                    eprintln!("  match: {:?}", m);
                }
            }
        }
    }
}

#[test]
#[ignore]
fn test_debug_a_infinitive_fps() {
    let runner = test_support::shared_runner();
    let sentences = vec![
        "Checkout or add a subscribe box to the cart.",
        "If there is already a notify email...",
        "I hope this will help to explain why I am hesitant to endorse a centralize unit commitment implementation.",
        "Let's have a listen to what he wants to say",
        "He had a listen.",
        "a show",
        "He pulled a stun gun.",
        "Simply book an Excite Travel Vacations package by October 31, 2001.",
        "Smith's bill would ban the use of methyl tertiary butyl ether, an oxygenate used in 85 percent of reformulated gasoline.",
        "He sustained the injury while chasing down a lose ball in the fourth quarter.",
        "We invite you to come an have a look.",
        "The carve outs from the LOL remain an open issue.",
        "The unify pathing issue is resolved, all buckets are filled.",
        "The integrate mw's are 10mw'w and are reflected on revised oasis request 4405.",
        "Perhaps should add the decommission of the present MIPS measurement system and the implementation of the PGAS system.",
        "The engine ran fine, the outdrive went up and down, the prop was the correct size and pitch.",
        "The proof of Cleveland's mettle came quickly: its NFL regular-season opener was against the two-time defending champion Eagles on September 16 in Philadelphia.",
        "He said we would need either a parent guarantee or a prepay to transact with occidental energy marketing.",
        "The open e and back a are often indicated in writing by the use of the letters alaph",
        "He sustained the injury while chasing down a lose ball in the fourth quarter.",
        "confirms that the deny code that we wrote is ineffective.",
        "The Disk II single-sided floppy drive used 5.25-inch floppy disks",
        "In the U.S.A be the best.",
        "the conquer stage will be more complex than decrease and conquer algorithms.",
    ];

    for text in &sentences {
        let sentence = runner.tokenize_sentence(text);
        let nwt: Vec<_> = sentence.non_whitespace_tokens();
        eprintln!("\n'{}':", text);
        for (i, token) in nwt.iter().enumerate() {
            let chunk = token.chunk().unwrap_or("-");
            eprintln!(
                "  [{}] {:15} pos={:?} chunk={}",
                i,
                format!("'{}'", token.token().token()),
                token.token().pos_tags(),
                chunk
            );
        }
    }
}
