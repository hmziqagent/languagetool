use og_core::checker::{SentenceTokenizer, WordTokenizer};
use og_core::{
    AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, Category, CheckRequest, CheckResult,
    Checker, IssueType, Language, RuleMatch, SentenceRange,
    rule::{Rule, TextLevelRule},
};
use og_rules::pattern_rule::PatternRuleEngine;
use og_spell::{Dictionary, SpellingCheckRule};
use og_tagger::EnglishTagger;
use og_tagger::XmlDisambiguator;
use og_xml::compiler::{CompiledRule, XmlCompiler};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A rule loaded from an XML grammar file, wrapping the compiled rule
/// and delegating to PatternRuleEngine for matching.
pub struct XmlPatternRule {
    compiled: CompiledRule,
    engine: PatternRuleEngine,
}

impl XmlPatternRule {
    pub fn new(compiled: CompiledRule) -> Self {
        Self {
            compiled,
            engine: PatternRuleEngine::new(),
        }
    }
}

impl Rule for XmlPatternRule {
    fn id(&self) -> &str {
        &self.compiled.id
    }

    #[allow(clippy::misnamed_getters)]
    fn description(&self) -> &str {
        &self.compiled.name
    }

    fn is_default_on(&self) -> bool {
        self.compiled.default_on
    }

    fn category(&self) -> Category {
        Category::new(&self.compiled.category.id, &self.compiled.category.name)
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Grammar
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        self.engine.match_rule(&self.compiled, sentence)
    }
}

/// Language engine that loads XML rules for a specific language
/// and provides a full checking pipeline.
pub struct LanguageEngine {
    language: Language,
    rules: Vec<Arc<dyn Rule>>,
    text_level_rules: Vec<Arc<dyn TextLevelRule>>,
    tagger: Option<Arc<dyn og_core::checker::Tagger>>,
    disambiguator: Option<Arc<dyn og_core::checker::Disambiguator>>,
    checker: Checker,
}

impl LanguageEngine {
    pub fn new(language: Language) -> Self {
        let checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge))
            .with_word_tokenizer(Arc::new(WordTokenizerBridge));

        Self {
            language,
            rules: Vec::new(),
            text_level_rules: Vec::new(),
            tagger: None,
            disambiguator: None,
            checker,
        }
    }

    /// Create an English LanguageEngine with all available rules
    pub fn english() -> Self {
        let mut tagger = EnglishTagger::new();

        // Load external data into the tagger
        if let Some(lt_root) = find_lt_resource_dir() {
            let dict_path =
                lt_root.join("en/src/main/resources/org/languagetool/resource/en/dict_decoded.txt");
            if dict_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&dict_path) {
                    let count = tagger.load_fsa_dictionary(&data);
                    eprintln!("Loaded {} FSA dictionary entries", count);
                }
            }
            let added_path =
                lt_root.join("en/src/main/resources/org/languagetool/resource/en/added.txt");
            if added_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&added_path) {
                    tagger.load_added(&data);
                }
            }
            let uncountable_path =
                lt_root.join("en/src/main/resources/org/languagetool/resource/en/uncountable.txt");
            if uncountable_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&uncountable_path) {
                    tagger.load_uncountable(&data);
                }
            }
            let partlycountable_path = lt_root
                .join("en/src/main/resources/org/languagetool/resource/en/partlycountable.txt");
            if partlycountable_path.exists() {
                if let Ok(data) = std::fs::read_to_string(&partlycountable_path) {
                    tagger.load_partlycountable(&data);
                }
            }
        }

        let tagger_arc: Arc<dyn og_core::checker::Tagger> = Arc::new(tagger);

        let mut engine = Self {
            language: Language::English,
            rules: Vec::new(),
            text_level_rules: Vec::new(),
            tagger: Some(tagger_arc),
            disambiguator: None,
            checker: Checker::new()
                .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge))
                .with_word_tokenizer(Arc::new(WordTokenizerBridge)),
        };

        // Load grammar.xml and disambiguation.xml
        if let Some(lt_root) = find_lt_resource_dir() {
            let grammar_path = grammar_xml_path("en", &lt_root);
            if grammar_path.exists() {
                if let Ok(count) = engine.load_xml_file(&grammar_path) {
                    eprintln!("Loaded {} XML rules for English", count);
                }
            }

            // Load disambiguation.xml
            let disambig_path = lt_root
                .join("en/src/main/resources/org/languagetool/resource/en/disambiguation.xml");
            if disambig_path.exists() {
                if let Ok(xml) = std::fs::read_to_string(&disambig_path) {
                    match XmlDisambiguator::from_xml(&xml) {
                        Ok(disambiguator) => {
                            eprintln!(
                                "Loaded {} disambiguation rules for English",
                                disambiguator.rule_count()
                            );
                            engine.disambiguator = Some(Arc::new(disambiguator));
                        }
                        Err(e) => eprintln!("Warning: Failed to load disambiguation rules: {}", e),
                    }
                }
            }

            // Load AvsAnRule with data files
            let det_a_path =
                lt_root.join("en/src/main/resources/org/languagetool/rules/en/det_a.txt");
            let det_an_path =
                lt_root.join("en/src/main/resources/org/languagetool/rules/en/det_an.txt");
            if det_a_path.exists() && det_an_path.exists() {
                if let (Ok(det_a), Ok(det_an)) = (
                    std::fs::read_to_string(&det_a_path),
                    std::fs::read_to_string(&det_an_path),
                ) {
                    let mut avsan = crate::en::AvsAnRule::new();
                    avsan.load_data(&det_a, &det_an);
                    engine.add_rule(Arc::new(avsan));
                }
            }

            // Add SimpleReplaceRule with data
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::english_default()));

            // Add redundancy and profanity rules (same engine, different data)
            engine.add_rule(Arc::new(
                crate::en::SimpleReplaceRule::english_redundancies(),
            ));
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::english_profanity()));
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::american_replace()));
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::british_replace()));
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::new_zealand_replace()));

            // Add generic native rules
            engine.add_rule(Arc::new(
                og_rules::native_rules::DoublePunctuationRule::new(),
            ));
            engine.add_rule(Arc::new(
                og_rules::native_rules::UppercaseSentenceStartRule::new(),
            ));
            engine.add_rule(Arc::new(og_rules::native_rules::CommaWhitespaceRule::new()));

            // Add English-specific native rules
            engine.add_rule(Arc::new(crate::en::ContractionSpellingRule::new()));
            engine.add_rule(Arc::new(crate::en::SpecificCaseRule::new()));
            engine.add_rule(Arc::new(crate::en::DiacriticsRule::new()));
            engine.add_rule(Arc::new(crate::en::PlainEnglishRule::new()));
            engine.add_rule(Arc::new(crate::en::CompoundRule::new()));
            engine.add_rule(Arc::new(crate::en::DashRule::new()));
            engine.add_rule(Arc::new(crate::en::WrongWordInContextRule::new()));
            engine.add_rule(Arc::new(crate::en::EnglishWordRepeatRule::new()));

            // n-gram-backed rules (no-ops until a corpus is trained — see
            // shared_ngram_model()).
            let lm = shared_ngram_model();
            engine.add_rule(Arc::new(crate::en::EnglishNgramProbabilityRule::new(
                lm.clone(),
            )));
            engine.add_rule(Arc::new(crate::en::UpperCaseNgramRule::new(lm.clone())));
            engine.add_rule(Arc::new(crate::en::EnglishConfusionProbabilityRule::new(
                lm.clone(),
            )));
            engine.add_rule(Arc::new(crate::en::FalseFriendRule::for_germans(
                lm.clone(),
            )));
            engine.add_rule(Arc::new(crate::en::FalseFriendRule::for_french(lm.clone())));
            engine.add_rule(Arc::new(crate::en::FalseFriendRule::for_dutch(lm.clone())));
            engine.add_rule(Arc::new(crate::en::FalseFriendRule::for_spaniards(
                lm.clone(),
            )));

            // Unit conversion suggestions (picky-only, default off)
            engine.add_rule(Arc::new(crate::en::UnitConversionRule::general()));

            // Load spellchecker with English word lists
            let mut dict = Dictionary::new();
            let hunspell_dir =
                lt_root.join("en/src/main/resources/org/languagetool/resource/en/hunspell");
            let resource_dir = lt_root.join("en/src/main/resources/org/languagetool/resource/en");

            // Load common words
            if let Ok(words) = std::fs::read_to_string(resource_dir.join("common_words.txt")) {
                for line in words.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        dict.add_word(word);
                    }
                }
            }

            // Try loading the full Morfologik decoded dictionary (259K forms)
            let dict_decoded = resource_dir.join("dict_decoded.txt");
            if dict_decoded.exists() {
                if let Ok(full_dict) = og_spell::Dictionary::from_morfologik(&dict_decoded) {
                    eprintln!(
                        "Loaded {} words from Morfologik dictionary",
                        full_dict.len()
                    );
                    dict = full_dict;
                }
            }

            // Load spelling dictionary supplements
            if let Ok(words) = std::fs::read_to_string(hunspell_dir.join("spelling.txt")) {
                for line in words.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        dict.add_word(word);
                    }
                }
            }

            // Load ignore words (words the spellchecker should skip)
            let mut ignore_words = HashSet::new();
            if let Ok(words) = std::fs::read_to_string(hunspell_dir.join("ignore.txt")) {
                for line in words.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        ignore_words.insert(word.to_lowercase());
                    }
                }
            }

            if dict.len() > 0 {
                let spell_rule = SpellingCheckRule::new(dict).with_ignore_words(ignore_words);
                eprintln!("Loaded spellchecker with {} words", spell_rule.dict_size());
                engine.add_rule(Arc::new(spell_rule));
            }
        }

        let lt_root = find_lt_resource_dir();
        if let Some(ref root) = lt_root {
            // style.xml uses the same rule schema as grammar.xml — load it
            // as additional XML pattern rules (plans/06 gap "style.xml never
            // loaded").
            let style_path = style_xml_path("en", root);
            if let Ok(count) = engine.load_xml_file(&style_path) {
                eprintln!("Loaded {count} style rules for English");
            }
        }

        // Add text-level rules
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::MultipleWhitespaceRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::SentenceWhitespaceRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::GenericUnpairedBracketsRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::GenericUnpairedQuotesRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(og_rules::text_level_rules::LongSentenceRule::new(
            30,
        )));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::WordRepeatBeginningRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(crate::en::ConsistentApostrophesRule::new()));
        engine.add_text_level_rule(Arc::new(crate::en::WordCoherencyRule::new()));
        engine.add_text_level_rule(Arc::new(crate::en::StyleFrequencyRule::adjective()));
        engine.add_text_level_rule(Arc::new(crate::en::StyleFrequencyRule::noun()));
        engine.add_text_level_rule(Arc::new(crate::en::StyleFrequencyRule::verb()));

        engine.rebuild_checker();
        engine
    }

    /// Create a German LanguageEngine (second language per the migration
    /// order in docs/PLAN.md). Backed by the real upstream resources when
    /// the LT resource root is configured:
    ///
    /// - POS tagger: `GermanTagger` on `resource/de/added.txt` (full-form
    ///   list; the binary Morfologik `german.dict` is not parsed — see the
    ///   compatibility matrix for the divergence).
    /// - XML rules: `rules/de/grammar.xml`, `rules/de/style.xml`, and
    ///   `resource/de/disambiguation.xml`.
    /// - Spelling: `GermanSpellingRule` on the igerman98 Hunspell
    ///   `de_DE.dic`/`.aff` (affix + compound aware).
    /// - Native ports: `DE_SIMPLE_REPLACE` (replace.txt) and
    ///   `DE_WORD_COHERENCY` (coherency.txt), plus the language-independent
    ///   generic rules.
    ///
    /// Without a resource root the engine degrades to generic rules only
    /// (spelling rule inactive, tagger empty, no XML rules).
    pub fn german() -> Self {
        let tagger_arc: Arc<dyn og_core::checker::Tagger> =
            Arc::new(crate::de::GermanTagger::from_resources());

        let mut engine = Self {
            language: Language::German,
            rules: Vec::new(),
            text_level_rules: Vec::new(),
            tagger: Some(tagger_arc),
            disambiguator: None,
            checker: Checker::new()
                .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge))
                .with_word_tokenizer(Arc::new(WordTokenizerBridge)),
        };

        if let Some(lt_root) = find_lt_resource_dir() {
            let grammar_path = grammar_xml_path("de", &lt_root);
            if let Ok(count) = engine.load_xml_file(&grammar_path) {
                eprintln!("Loaded {count} XML rules for German");
            }

            let style_path = style_xml_path("de", &lt_root);
            if let Ok(count) = engine.load_xml_file(&style_path) {
                eprintln!("Loaded {count} style rules for German");
            }

            let disambig_path = lt_root
                .join("de/src/main/resources/org/languagetool/resource/de/disambiguation.xml");
            if let Ok(xml) = std::fs::read_to_string(&disambig_path) {
                match XmlDisambiguator::from_xml(&xml) {
                    Ok(disambiguator) => {
                        eprintln!(
                            "Loaded {} disambiguation rules for German",
                            disambiguator.rule_count()
                        );
                        engine.disambiguator = Some(Arc::new(disambiguator));
                    }
                    Err(e) => {
                        eprintln!("Warning: Failed to load German disambiguation rules: {e}")
                    }
                }
            }

            // DE_SIMPLE_REPLACE from replace.txt (same file format as English).
            let mut replace = crate::en::SimpleReplaceRule::new(
                "DE_SIMPLE_REPLACE",
                "Möglicher Tippfehler",
                "TYPOS",
                "Mögliche Tippfehler",
            );
            if let Some(data) = og_spell::read_lt_resource(
                "de/src/main/resources/org/languagetool/rules/de/replace.txt",
            ) {
                replace.load_data(&data);
            }
            engine.add_rule(Arc::new(replace));

            // German spelling on the real igerman98 Hunspell dictionary.
            engine.add_rule(Arc::new(crate::de::GermanSpellingRule::from_resources()));

            // Generic (language-independent) rules.
            engine.add_rule(Arc::new(
                og_rules::native_rules::DoublePunctuationRule::new(),
            ));
            engine.add_rule(Arc::new(
                og_rules::native_rules::UppercaseSentenceStartRule::new(),
            ));
            engine.add_rule(Arc::new(og_rules::native_rules::CommaWhitespaceRule::new()));
        }

        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::MultipleWhitespaceRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::SentenceWhitespaceRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::GenericUnpairedBracketsRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::GenericUnpairedQuotesRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(og_rules::text_level_rules::LongSentenceRule::new(
            30,
        )));
        engine.add_text_level_rule(Arc::new(crate::de::GermanWordCoherencyRule::new()));

        engine.rebuild_checker();
        engine
    }

    /// Generic engine factory: map a language code ("en", "en-US", "de",
    /// "de-AT", ...) to a fully constructed engine. Returns `None` for
    /// languages without an implementation (everything except English and
    /// German at this point — see the migration order in docs/PLAN.md).
    ///
    /// Variant codes share the base-language engine (en-US/en-GB → the
    /// English engine, de-DE/de-AT/de-CH → the German engine); per-variant
    /// rules are not yet distinguished.
    pub fn for_code(code: &str) -> Option<Self> {
        let base = code.split('-').next().unwrap_or(code);
        match base {
            "en" => Some(Self::english()),
            "de" => Some(Self::german()),
            _ => None,
        }
    }

    /// Process-wide shared engine for a language code (PLAN Phase 12
    /// performance item: the eager grammar/style XML loads make an engine
    /// build expensive — `english()` compiles ~6,000 XML rules, ~8 s cold).
    ///
    /// Engines are immutable after construction (`check`/`check_with_request`
    /// take `&self` and reset all rules per check), so one instance can be
    /// shared Arc-wise by every consumer in the process. The first call pays
    /// the build; every later call — for the base code or any variant —
    /// returns an Arc clone of the same engine (pointer-equal). This is the
    /// og-langs-side half of engine sharing; og-api additionally wraps its
    /// own OnceLock map over language codes.
    pub fn shared_for_code(code: &str) -> Option<std::sync::Arc<Self>> {
        use std::sync::{Arc, OnceLock};
        static ENGLISH: OnceLock<Arc<LanguageEngine>> = OnceLock::new();
        static GERMAN: OnceLock<Arc<LanguageEngine>> = OnceLock::new();
        let base = code.split('-').next().unwrap_or(code);
        match base {
            "en" => Some(ENGLISH.get_or_init(|| Arc::new(Self::english())).clone()),
            "de" => Some(GERMAN.get_or_init(|| Arc::new(Self::german())).clone()),
            _ => None,
        }
    }

    /// Load grammar rules from an XML file
    pub fn load_xml_rules(&mut self, xml: &str) -> Result<usize, String> {
        let compiler = XmlCompiler::new();
        let rule_set = compiler
            .compile_file(xml)
            .map_err(|e| format!("XML compilation error: {}", e))?;

        let count = rule_set.rules.len();
        for compiled_rule in rule_set.rules {
            let rule = Arc::new(XmlPatternRule::new(compiled_rule));
            self.rules.push(rule);
        }

        // Rebuild checker with updated rules
        self.rebuild_checker();
        Ok(count)
    }

    /// Load grammar rules from a file path
    pub fn load_xml_file(&mut self, path: &Path) -> Result<usize, String> {
        let xml = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        self.load_xml_rules(&xml)
    }

    /// Add a native Rust rule
    pub fn add_rule(&mut self, rule: Arc<dyn Rule>) {
        self.rules.push(rule);
        self.rebuild_checker();
    }

    /// Add a text-level rule
    pub fn add_text_level_rule(&mut self, rule: Arc<dyn TextLevelRule>) {
        self.text_level_rules.push(rule);
        self.rebuild_checker();
    }

    fn rebuild_checker(&mut self) {
        let mut checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge))
            .with_word_tokenizer(Arc::new(WordTokenizerBridge))
            .set_rules(self.rules.clone())
            .set_text_level_rules(self.text_level_rules.clone());

        if let Some(ref tagger) = self.tagger {
            checker = checker.with_tagger(tagger.clone());
        }

        if let Some(ref disambiguator) = self.disambiguator {
            checker = checker.with_disambiguator(disambiguator.clone());
        }

        self.checker = checker;
    }

    /// Check text and return matches
    pub fn check(&self, text: &str) -> CheckResult {
        let request = CheckRequest::new(text, self.language.clone());
        self.checker.check(&request)
    }

    /// Check text with custom request options
    pub fn check_with_request(&self, request: &CheckRequest) -> CheckResult {
        self.checker.check(request)
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn language(&self) -> &Language {
        &self.language
    }
}

/// Bridge from og-tokenizer to og-core's SentenceTokenizer trait
struct SentenceSplitterBridge;

impl SentenceTokenizer for SentenceSplitterBridge {
    fn split(&self, text: &str) -> Vec<SentenceRange> {
        let splitter = og_tokenizer::DefaultSentenceSplitter::new();
        use og_tokenizer::SentenceTokenizer as _;
        let sentences = splitter.split(text);
        sentences
            .into_iter()
            .map(|s| SentenceRange {
                start: s.start(),
                end: s.end(),
            })
            .collect()
    }
}

/// Bridge from og-tokenizer to og-core's WordTokenizer trait
struct WordTokenizerBridge;

impl WordTokenizer for WordTokenizerBridge {
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

/// Build (once per process) the n-gram language model backing the
/// probability-based rules (EnglishNgramProbabilityRule, UpperCaseNgramRule,
/// EnglishConfusionProbabilityRule, the false-friend rules).
///
/// The model is trained from the plain-text corpus file named by the
/// `OG_LT_NGRAM_CORPUS` environment variable (order 3). With no variable or
/// an unreadable file the model stays empty, and every n-gram-backed rule
/// degrades to a no-op — matching Java, where these rules only exist when an
/// ngram index is configured. (Java ships a Lucene ngram index built from
/// large corpora; that binary format is not parsed here.)
pub fn shared_ngram_model() -> std::sync::Arc<og_languagemodel::LanguageModel> {
    use std::sync::OnceLock;
    static MODEL: OnceLock<std::sync::Arc<og_languagemodel::LanguageModel>> = OnceLock::new();
    MODEL
        .get_or_init(|| {
            let model = match std::env::var("OG_LT_NGRAM_CORPUS") {
                Ok(path) if !path.is_empty() => {
                    match og_languagemodel::LanguageModel::from_corpus_file(
                        std::path::Path::new(&path),
                        3,
                    ) {
                        Ok(m) => {
                            eprintln!(
                                "Trained n-gram model from {} ({} tokens)",
                                path,
                                m.token_count()
                            );
                            m
                        }
                        Err(e) => {
                            eprintln!("Warning: cannot train n-gram model from {path}: {e}");
                            og_languagemodel::LanguageModel::new(3)
                        }
                    }
                }
                _ => og_languagemodel::LanguageModel::new(3),
            };
            std::sync::Arc::new(model)
        })
        .clone()
}

/// Find the LanguageTool resource root (the `languagetool-language-modules`
/// directory of the upstream checkout).
///
/// Resolution order (see `og_spell::resource` for details):
/// 1. `OG_LT_RESOURCE_ROOT` environment variable.
/// 2. Ancestor search from the current working directory for
///    `languagetool/languagetool-language-modules`.
/// 3. Ancestor search from og-spell's compile-time manifest directory.
pub fn find_lt_resource_dir() -> Option<PathBuf> {
    og_spell::resource::lt_resource_root()
}

/// Get the style.xml path for a language
pub fn style_xml_path(lang_code: &str, lt_root: &Path) -> PathBuf {
    let module_name = lang_code.split('-').next().unwrap_or(lang_code);
    lt_root
        .join(module_name)
        .join("src/main/resources/org/languagetool/rules")
        .join(module_name)
        .join("style.xml")
}

/// Get the grammar.xml path for a language
pub fn grammar_xml_path(lang_code: &str, lt_root: &Path) -> PathBuf {
    // Map language code to module directory name
    let module_name = lang_code.split('-').next().unwrap_or(lang_code);
    lt_root
        .join(module_name)
        .join("src/main/resources/org/languagetool/rules")
        .join(module_name)
        .join("grammar.xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_engine_basic() {
        let mut engine = LanguageEngine::new(Language::English);
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="TEST_RULE" name="Test">
                    <pattern>
                        <token>their</token>
                        <token>is</token>
                    </pattern>
                    <message>Did you mean <suggestion>there is</suggestion>?</message>
                </rule>
            </category>
        </rules>"#;

        let count = engine.load_xml_rules(xml).unwrap();
        assert_eq!(count, 1);
        assert_eq!(engine.rule_count(), 1);

        let result = engine.check("their is a problem");
        assert!(!result.matches.is_empty(), "Expected at least one match");
        assert_eq!(result.matches[0].rule().id(), "TEST_RULE");
    }

    #[test]
    fn test_language_engine_no_match() {
        let mut engine = LanguageEngine::new(Language::English);
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="NO_MATCH_RULE" name="No match test">
                    <pattern>
                        <token>xyzzy</token>
                    </pattern>
                    <message>This should not match normal text.</message>
                </rule>
            </category>
        </rules>"#;

        engine.load_xml_rules(xml).unwrap();
        let result = engine.check("This is a normal sentence.");
        assert!(result.matches.is_empty());
    }

    #[test]
    fn test_language_engine_multiple_rules() {
        let mut engine = LanguageEngine::new(Language::English);
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="RULE_A" name="Rule A">
                    <pattern>
                        <token regexp="yes">teh</token>
                    </pattern>
                    <message>Did you mean <suggestion>the</suggestion>?</message>
                </rule>
                <rule id="RULE_B" name="Rule B">
                    <pattern>
                        <token>their</token>
                        <token>is</token>
                    </pattern>
                    <message>Did you mean <suggestion>there is</suggestion>?</message>
                </rule>
            </category>
        </rules>"#;

        let count = engine.load_xml_rules(xml).unwrap();
        assert_eq!(count, 2);

        let result = engine.check("teh their is");
        assert!(
            result.matches.len() >= 2,
            "Expected at least 2 matches, got {}",
            result.matches.len()
        );
    }

    #[test]
    fn test_language_engine_with_native_rules() {
        use og_rules::native_rules::WordRepeatRule;
        let mut engine = LanguageEngine::new(Language::English);
        engine.add_rule(Arc::new(WordRepeatRule::new()));

        let result = engine.check("the the quick brown fox");
        assert!(!result.matches.is_empty(), "Expected word repeat detection");
    }

    #[test]
    fn test_load_real_english_grammar() {
        let lt_root = match find_lt_resource_dir() {
            Some(p) => p,
            None => {
                eprintln!("Skipping: LT resource dir not found");
                return;
            }
        };

        let grammar_path = grammar_xml_path("en", &lt_root);
        if !grammar_path.exists() {
            eprintln!(
                "Skipping: English grammar.xml not found at {:?}",
                grammar_path
            );
            return;
        }

        let mut engine = LanguageEngine::new(Language::English);
        match engine.load_xml_file(&grammar_path) {
            Ok(count) => {
                println!("Loaded {} rules from English grammar.xml", count);
                assert!(count > 0, "Expected at least some rules");

                // Test with a simple sentence
                let result = engine.check("This is a test.");
                assert_eq!(result.language.code, "en-US");
            }
            Err(e) => {
                panic!("Failed to load English grammar.xml: {}", e);
            }
        }
    }

    #[test]
    fn test_engine_sentence_splitting() {
        let mut engine = LanguageEngine::new(Language::English);
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="CAPPED" name="Sentence start">
                    <pattern>
                        <token>hello</token>
                    </pattern>
                    <message>Capitalize.</message>
                </rule>
            </category>
        </rules>"#;
        engine.load_xml_rules(xml).unwrap();

        let result = engine.check("Well hello there. And hello again.");
        assert!(result.matches.len() >= 1);
    }

    #[test]
    fn test_for_code_known_languages() {
        for code in ["en", "en-US", "en-GB"] {
            let engine = LanguageEngine::for_code(code)
                .unwrap_or_else(|| panic!("for_code({code}) should be Some"));
            assert_eq!(engine.language().code(), "en-US");
        }
        for code in ["de", "de-DE", "de-AT", "de-CH"] {
            let engine = LanguageEngine::for_code(code)
                .unwrap_or_else(|| panic!("for_code({code}) should be Some"));
            assert_eq!(engine.language().code(), "de-DE");
        }
    }

    #[test]
    fn test_for_code_unsupported_language() {
        assert!(LanguageEngine::for_code("fr").is_none());
        assert!(LanguageEngine::for_code("zz").is_none());
    }

    #[test]
    fn test_shared_for_code_returns_pointer_equal_engines() {
        let en = LanguageEngine::shared_for_code("en").expect("shared en");
        let en_us = LanguageEngine::shared_for_code("en-US").expect("shared en-US");
        assert!(
            std::sync::Arc::ptr_eq(&en, &en_us),
            "variant code must share the base engine instance"
        );
        let de = LanguageEngine::shared_for_code("de").expect("shared de");
        assert!(
            !std::sync::Arc::ptr_eq(&en, &de),
            "different languages must not share an engine"
        );
        assert!(LanguageEngine::shared_for_code("fr").is_none());
        // The shared engine is usable for checks (immutable after build) —
        // doubled spaces fire the generic whitespace rule even without the
        // resource checkout.
        let result = en.check("This  has  doubled spaces.");
        assert!(!result.matches.is_empty());
    }

    #[test]
    fn test_german_engine_generic_rules_without_resources() {
        // Constructing the German engine must never panic, even when the
        // resource root is absent (generic + text-level rules only).
        let engine = LanguageEngine::german();
        assert!(
            engine.rule_count() > 0,
            "generic rules should be registered"
        );
        let result = engine.check("Das  ist  ein Test");
        // MultipleWhitespaceRule should fire on the double space.
        assert!(
            !result.matches.is_empty(),
            "expected generic text-level matches"
        );
    }

    #[test]
    fn test_load_real_german_grammar() {
        let lt_root = match find_lt_resource_dir() {
            Some(p) => p,
            None => {
                eprintln!("Skipping: LT resource dir not found");
                return;
            }
        };

        let grammar_path = grammar_xml_path("de", &lt_root);
        if !grammar_path.exists() {
            eprintln!(
                "Skipping: German grammar.xml not found at {:?}",
                grammar_path
            );
            return;
        }

        let mut engine = LanguageEngine::new(Language::German);
        match engine.load_xml_file(&grammar_path) {
            Ok(count) => {
                println!("Loaded {count} rules from German grammar.xml");
                assert!(count > 0, "Expected at least some German rules");
                let result = engine.check("Das ist ein Test.");
                assert_eq!(result.language.code, "de-DE");
            }
            Err(e) => {
                panic!("Failed to load German grammar.xml: {e}");
            }
        }
    }
}
