use og_core::checker::{SentenceTokenizer, WordTokenizer};
use og_core::{
    AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, Category, CheckRequest, CheckResult,
    Checker, IssueType, Language, RuleMatch, SentenceRange,
    rule::{Rule, TextLevelRule},
};
use og_rules::pattern_rule::PatternRuleEngine;
use og_spell::SpellingCheckRule;
use og_tagger::{
    ChainedDisambiguator, EnglishTagger, MultiWordChunkerDisambiguator, XmlDisambiguator,
};
use og_xml::compiler::{CompiledRule, XmlCompiler};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct XmlPatternRule {
    compiled: CompiledRule,
    engine: PatternRuleEngine,
    category: Category,
}

impl XmlPatternRule {
    pub fn new(compiled: CompiledRule) -> Self {
        Self::with_spelling(compiled, None)
    }

    pub fn with_spelling(
        compiled: CompiledRule,
        spelling: Option<Arc<og_rules::spelling_support::SpellingSupport>>,
    ) -> Self {
        Self::with_resources(compiled, spelling, None)
    }

    pub fn with_resources(
        compiled: CompiledRule,
        spelling: Option<Arc<og_rules::spelling_support::SpellingSupport>>,
        synthesizer: Option<Arc<og_rules::postponed_adjective::LangSynthesizer>>,
    ) -> Self {
        let category = Category::new(&compiled.category.id, &compiled.category.name);
        let mut engine = PatternRuleEngine::new();
        if let Some(spelling) = spelling {
            engine = engine.with_spelling_support(spelling);
        }
        if let Some(synthesizer) = synthesizer {
            engine = engine.with_synthesizer(synthesizer);
        }
        Self {
            compiled,
            engine,
            category,
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
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Grammar
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        self.engine.match_rule(&self.compiled, sentence)
    }
}

pub struct LanguageEngine {
    language: Language,
    rules: Vec<Arc<dyn Rule>>,
    text_level_rules: Vec<Arc<dyn TextLevelRule>>,
    tagger: Option<Arc<dyn og_core::checker::Tagger>>,
    disambiguator: Option<Arc<dyn og_core::checker::Disambiguator>>,
    post_chunker: Option<Arc<dyn og_core::checker::Chunker>>,
    spelling_support: Option<Arc<og_rules::spelling_support::SpellingSupport>>,
    german_speller: Option<Arc<og_spell::GermanSpeller>>,
    synthesizer: Option<Arc<og_rules::postponed_adjective::LangSynthesizer>>,
    checker: Checker,
    language_filter: Arc<dyn og_core::match_filters::LanguageMatchFilter>,
    l2_rules: std::sync::Mutex<HashMap<String, Vec<Arc<dyn Rule>>>>,
    l2_checkers: std::sync::Mutex<HashMap<String, Checker>>,
}

impl LanguageEngine {
    pub fn new(language: Language) -> Self {
        let checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge(
                language.short_code().to_string(),
            )))
            .with_word_tokenizer(Arc::new(WordTokenizerBridge::for_code(
                language.short_code(),
            )));

        let language_filter = crate::match_priority::for_code(language.short_code());
        Self {
            language,
            rules: Vec::new(),
            text_level_rules: Vec::new(),
            tagger: None,
            disambiguator: None,
            post_chunker: None,
            spelling_support: None,
            german_speller: None,
            synthesizer: None,
            checker,
            language_filter,
            l2_rules: std::sync::Mutex::new(HashMap::new()),
            l2_checkers: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn shared_for_code(code: &str) -> Option<&'static LanguageEngine> {
        static CACHE: std::sync::OnceLock<
            std::sync::Mutex<HashMap<String, &'static LanguageEngine>>,
        > = std::sync::OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let mut guard = cache.lock().unwrap();
        if let Some(engine) = guard.get(code) {
            return Some(engine);
        }
        let engine = match code {
            "en" => Self::english(),
            "de" => Self::german(),
            other => Self::for_code(other)?,
        };
        let leaked: &'static LanguageEngine = Box::leak(Box::new(engine));
        guard.insert(code.to_string(), leaked);
        Some(leaked)
    }

    pub fn english() -> Self {
        let tagger = match find_lt_resource_dir() {
            Some(lt_root) => {
                let resource_dir =
                    lt_root.join("en/src/main/resources/org/languagetool/resource/en");
                if resource_dir.exists() {
                    EnglishTagger::from_resource_dir(&resource_dir)
                } else {
                    EnglishTagger::new()
                }
            }
            None => EnglishTagger::new(),
        };

        let tagger_arc: Arc<dyn og_core::checker::Tagger> = Arc::new(tagger);

        let mut engine = Self {
            language: Language::English,
            rules: Vec::new(),
            text_level_rules: Vec::new(),
            tagger: Some(tagger_arc),
            disambiguator: None,
            post_chunker: None,
            spelling_support: None,
            german_speller: None,
            synthesizer: None,
            checker: Checker::new()
                .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge("en".to_string())))
                .with_word_tokenizer(Arc::new(WordTokenizerBridge::Default(true))),
            language_filter: crate::match_priority::for_code("en"),
            l2_rules: std::sync::Mutex::new(HashMap::new()),
            l2_checkers: std::sync::Mutex::new(HashMap::new()),
        };

        let mut spelling_support: Option<Arc<og_rules::spelling_support::SpellingSupport>> = None;
        if let Some(lt_root) = find_lt_resource_dir() {
            let resource_dir = lt_root.join("en/src/main/resources/org/languagetool/resource/en");
            og_tagger::chunker::load_opennlp_chunker(&resource_dir);
            if let Some(support) = find_lt_core_resource_dir().and_then(|core_resource_dir| {
                og_rules::spelling_support::SpellingSupport::dictionary_from_dir(&resource_dir).map(
                    |dict| {
                        og_rules::spelling_support::SpellingSupport::new_for_language(
                            std::sync::Arc::new(dict),
                            &core_resource_dir,
                            &resource_dir,
                            "en",
                        )
                    },
                )
            }) {
                spelling_support = Some(Arc::new(support));
            }
            engine.spelling_support = spelling_support.clone();

            let rules_dir = lt_root.join("en/src/main/resources/org/languagetool/rules/en");
            let grammar_path = grammar_xml_path("en", &lt_root);
            if grammar_path.exists()
                && let Ok(count) = engine.load_xml_file(&grammar_path)
            {
                eprintln!("Loaded {} XML rules for English", count);
            }
            for file in ["style.xml", "grammar_custom.xml"] {
                let path = rules_dir.join(file);
                if !path.exists() {
                    continue;
                }
                match engine.load_xml_file(&path) {
                    Ok(count) => eprintln!("Loaded {} rules from {file}", count),
                    Err(e) => eprintln!("Warning: failed to load {file}: {e}"),
                }
            }
            for file in [
                "en-US/grammar.xml",
                "en-US/style.xml",
                "en-US/grammar-premium.xml",
            ] {
                let path = rules_dir.join(file);
                if !path.exists() {
                    continue;
                }
                match engine.load_xml_file(&path) {
                    Ok(count) => eprintln!("Loaded {} rules from {file}", count),
                    Err(e) => eprintln!("Warning: failed to load {file}: {e}"),
                }
            }

            let disambig_path = lt_root
                .join("en/src/main/resources/org/languagetool/resource/en/disambiguation.xml");
            if disambig_path.exists()
                && let Ok(xml) = std::fs::read_to_string(&disambig_path)
            {
                match XmlDisambiguator::from_xml(&xml) {
                    Ok(disambiguator) => {
                        eprintln!(
                            "Loaded {} disambiguation rules for English",
                            disambiguator.rule_count()
                        );
                        let mut chain: Vec<Arc<dyn og_core::checker::Disambiguator>> =
                            vec![Arc::new(disambiguator)];
                        if let Some(multiword) =
                            MultiWordChunkerDisambiguator::from_resource_dir(&resource_dir)
                        {
                            chain.insert(0, Arc::new(multiword));
                        }
                        if let Some(core_resource_dir) = find_lt_core_resource_dir() {
                            if let Some(global) =
                                MultiWordChunkerDisambiguator::spelling_global(&core_resource_dir)
                            {
                                chain.insert(0, Arc::new(global));
                            }
                        }
                        engine.disambiguator = Some(Arc::new(ChainedDisambiguator::new(chain)));
                    }
                    Err(e) => eprintln!("Warning: Failed to load disambiguation rules: {}", e),
                }
            }

            let det_a_path =
                lt_root.join("en/src/main/resources/org/languagetool/rules/en/det_a.txt");
            let det_an_path =
                lt_root.join("en/src/main/resources/org/languagetool/rules/en/det_an.txt");
            let mut native_rules: Vec<Arc<dyn Rule>> = Vec::new();
            if det_a_path.exists()
                && det_an_path.exists()
                && let (Ok(det_a), Ok(det_an)) = (
                    std::fs::read_to_string(&det_a_path),
                    std::fs::read_to_string(&det_an_path),
                )
            {
                let mut avsan = crate::en::AvsAnRule::new();
                avsan.load_data(&det_a, &det_an);
                native_rules.push(Arc::new(avsan));
            }

            native_rules.push(Arc::new(crate::en::SimpleReplaceRule::english_default()));
            native_rules.push(Arc::new(
                crate::en::SimpleReplaceRule::english_redundancies(),
            ));
            native_rules.push(Arc::new(crate::en::SimpleReplaceRule::english_profanity()));
            native_rules.push(Arc::new(crate::en::SimpleReplaceRule::american_replace()));
            native_rules.push(Arc::new(crate::en::SimpleReplaceRule::british_replace()));
            native_rules.push(Arc::new(crate::en::SimpleReplaceRule::new_zealand_replace()));

            native_rules.push(Arc::new(
                og_rules::native_rules::DoublePunctuationRule::new(),
            ));
            native_rules.push(Arc::new(
                og_rules::native_rules::UppercaseSentenceStartRule::new(),
            ));
            native_rules.push(Arc::new(og_rules::native_rules::CommaWhitespaceRule::new()));

            native_rules.push(Arc::new(crate::en::ContractionSpellingRule::new()));
            native_rules.push(Arc::new(crate::en::SpecificCaseRule::new()));
            native_rules.push(Arc::new(crate::en::DiacriticsRule::new()));
            native_rules.push(Arc::new(crate::en::PlainEnglishRule::new()));
            native_rules.push(Arc::new(crate::en::CompoundRule::new()));
            native_rules.push(Arc::new(crate::en::DashRule::new()));
            native_rules.push(Arc::new(crate::en::WrongWordInContextRule::new()));
            native_rules.push(Arc::new(crate::en::EnglishWordRepeatRule::new()));
            native_rules.push(Arc::new(
                crate::en::unit_conversion::UnitConversionRule::new(),
            ));

            let hunspell_dir =
                lt_root.join("en/src/main/resources/org/languagetool/resource/en/hunspell");

            let dict = variant_dictionary(&hunspell_dir, "US");
            eprintln!("Loaded {} words from Morfologik dictionary", dict.len());

            let mut ignore_words = HashSet::new();
            if let Ok(words) = std::fs::read_to_string(hunspell_dir.join("ignore.txt")) {
                for line in words.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        ignore_words.insert(word.to_lowercase());
                    }
                }
            }

            if !dict.is_empty() {
                let spell_rule = SpellingCheckRule::new_shared(
                    Arc::new(dict),
                    "MORFOLOGIK_RULE_EN_US",
                    HashSet::new(),
                )
                .with_ignore_words(ignore_words);
                eprintln!("Loaded spellchecker with {} words", spell_rule.dict_size());
                native_rules.push(Arc::new(spell_rule));
            }
            engine.add_rules(native_rules);
        }

        let mut extra_text_level: Vec<Arc<dyn TextLevelRule>> = vec![Arc::new(
            crate::en::repeated_words::EnglishRepeatedWordsRule::new(),
        )];
        let text_level_rules: [Arc<dyn TextLevelRule>; 13] = [
            Arc::new(og_rules::text_level_rules::MultipleWhitespaceRule::new()),
            Arc::new(og_rules::text_level_rules::SentenceWhitespaceRule::new()),
            Arc::new(og_rules::text_level_rules::GenericUnpairedBracketsRule::new()),
            Arc::new(og_rules::text_level_rules::GenericUnpairedQuotesRule::english()),
            Arc::new(og_rules::text_level_rules::LongSentenceRule::new(40)),
            Arc::new(og_rules::text_level_rules::WordRepeatBeginningRule::new()),
            Arc::new(crate::en::ConsistentApostrophesRule::new()),
            Arc::new(crate::en::WordCoherencyRule::new()),
            Arc::new(og_rules::text_level_rules::WhiteSpaceBeforeParagraphEndRule::new()),
            Arc::new(og_rules::text_level_rules::EmptyLineRule::new()),
            Arc::new(crate::en::StyleFrequencyRule::adjective()),
            Arc::new(crate::en::StyleFrequencyRule::noun()),
            Arc::new(crate::en::StyleFrequencyRule::verb()),
        ];
        engine.add_text_level_rules(text_level_rules);
        engine.add_text_level_rules(extra_text_level.drain(..));
        engine.add_rule(Arc::new(
            og_rules::text_level_rules::WhiteSpaceAtBeginOfParagraphRule::new(),
        ));

        if let Some(model) = og_languagemodel::DiskLanguageModel::shared_for_language("en") {
            engine.add_rule(Arc::new(
                crate::upper_case_ngram::EnglishUpperCaseNgramRule::new(model.clone()),
            ));
            engine.add_rule(Arc::new(
                crate::confusion_probability::ConfusionRule::new_english(model.clone()),
            ));
            engine.add_rule(Arc::new(
                crate::en::ngram_probability::EnglishNgramProbabilityRule::new(model),
            ));
        }

        engine.rebuild_checker();
        engine
    }

    pub fn german() -> Self {
        let mut engine = Self::new(Language::German);

        let mut shared_dict: Option<Arc<og_spell::Dictionary>> = None;
        let mut german_resource_dir: Option<PathBuf> = None;
        let mut german_tagger: Option<Arc<og_tagger::GermanTagger>> = None;
        let mut german_speller: Option<Arc<og_spell::GermanSpeller>> = None;
        if let Some(lt_root) = find_lt_resource_dir() {
            let resource_dir = lt_root.join("de/src/main/resources/org/languagetool/resource/de");
            german_resource_dir = Some(resource_dir.clone());
            if let Some(tagger) = og_tagger::GermanTagger::from_resource_dir(&resource_dir) {
                let tagger = Arc::new(tagger);
                german_tagger = Some(tagger.clone());
                engine.tagger = Some(tagger as Arc<dyn og_core::checker::Tagger>);
            }
            let disambig_path = resource_dir.join("disambiguation.xml");
            if disambig_path.exists()
                && let Ok(xml) = std::fs::read_to_string(&disambig_path)
                && let Ok(disambiguator) = XmlDisambiguator::from_xml(&xml)
            {
                engine.disambiguator = Some(Arc::new(disambiguator));
            }

            let mut dict = og_spell::Dictionary::new();
            if let Some(words) = load_german_wordlist(&resource_dir) {
                for word in &words {
                    dict.add_word(word);
                }
                eprintln!("Loaded {} German dictionary words", words.len());
            }
            if let Ok(extra) = std::fs::read_to_string(resource_dir.join("hunspell/spelling.txt")) {
                for line in extra.lines() {
                    for word in og_rules::spelling_support::prepare_line_for_speller(
                        og_rules::spelling_support::SpellerVariant::German,
                        line,
                    ) {
                        dict.add_word(&word);
                    }
                }
            }
            for word in og_rules::spelling_support::german_hunspell_words(&resource_dir) {
                dict.add_word(&word);
            }
            if !dict.is_empty() {
                let dict_arc = Arc::new(dict);
                if let Some(core_resource_dir) = find_lt_core_resource_dir() {
                    engine.spelling_support = Some(Arc::new(
                        og_rules::spelling_support::SpellingSupport::new_for_language(
                            dict_arc.clone(),
                            &core_resource_dir,
                            &resource_dir,
                            "de",
                        ),
                    ));
                }
                shared_dict = Some(dict_arc.clone());
                let speller = Arc::new(og_spell::GermanSpeller::new(
                    dict_arc.clone(),
                    &resource_dir,
                    find_lt_core_resource_dir().as_deref(),
                    engine.tagger.clone(),
                    "de-DE",
                ));
                german_speller = Some(speller.clone());
                engine.german_speller = Some(speller.clone());
                engine.add_rule(Arc::new(og_spell::GermanSpellerRule::new(
                    speller,
                    "MORFOLOGIK_RULE_DE_DE",
                )));
                engine.add_rule(Arc::new(crate::de::CompoundInfinitivRule::new(Some(
                    dict_arc,
                ))));
            }

            let rules_dir = lt_root.join("de/src/main/resources/org/languagetool/rules/de");
            for file in ["grammar.xml", "style.xml", "grammar_custom.xml"] {
                let path = rules_dir.join(file);
                if !path.exists() {
                    continue;
                }
                match engine.load_xml_file(&path) {
                    Ok(count) => eprintln!("Loaded {} rules from de/{file}", count),
                    Err(e) => eprintln!("Warning: failed to load de/{file}: {e}"),
                }
            }
        }

        engine.add_rule(Arc::new(crate::de::SimpleReplaceRule::new()));
        engine.add_rule(Arc::new(
            og_rules::native_rules::UppercaseSentenceStartRule::new(),
        ));
        let synth_dictionary = german_resource_dir
            .as_deref()
            .and_then(og_tagger::german_tagger::shared_german_dictionary);
        engine.add_rule(Arc::new(crate::de::AgreementRule::new(
            engine.tagger.clone(),
            engine.disambiguator.clone(),
            shared_dict,
            synth_dictionary.clone(),
        )));
        engine.add_rule(Arc::new(crate::de::AgreementRule2::new(
            synth_dictionary.clone(),
        )));
        engine.add_rule(Arc::new(crate::de::CaseRule::new(
            german_tagger.clone(),
            engine.spelling_support.clone(),
        )));
        engine.add_rule(Arc::new(crate::de::GermanDashRule));
        engine.add_text_level_rule(Arc::new(crate::de::VerbAgreementRule::new(
            synth_dictionary.clone(),
        )));
        engine.add_rule(Arc::new(crate::de::SubjectVerbAgreementRule::new(
            german_tagger.clone(),
        )));
        engine.add_rule(Arc::new(crate::de::OldSpellingRule::new("DE")));
        engine.add_rule(Arc::new(crate::de::MissingVerbRule::new(
            engine.tagger.clone(),
        )));
        engine.add_rule(Arc::new(crate::de::GermanWordRepeatRule::new()));
        engine.add_rule(Arc::new(
            crate::de::MissingCommaRelativeClauseRule::in_front(),
        ));
        engine.add_rule(Arc::new(crate::de::MissingCommaRelativeClauseRule::behind()));
        engine.add_rule(Arc::new(crate::de::GermanWrongWordInContextRule));
        engine.add_rule(Arc::new(crate::de::WiederVsWiderRule));
        engine.add_rule(Arc::new(crate::de::RedundantModalOrAuxiliaryVerb));
        engine.add_rule(Arc::new(crate::de::GermanUnitConversionRule::new()));
        engine.add_text_level_rule(Arc::new(crate::de::GermanWordCoherencyRule));
        engine.add_text_level_rule(Arc::new(crate::de::CompoundCoherencyRule));
        engine.add_text_level_rule(Arc::new(crate::de::DuUpperLowerCaseRule));
        engine.add_text_level_rule(Arc::new(crate::de::SimilarNameRule));
        engine.add_text_level_rule(Arc::new(crate::de::GermanParagraphRepeatBeginningRule));
        engine.add_text_level_rule(Arc::new(crate::de::GermanRepeatedWordsRule));
        engine.add_text_level_rule(Arc::new(crate::de::GermanReadabilityRule::new(true)));
        engine.add_text_level_rule(Arc::new(crate::de::GermanReadabilityRule::new(false)));
        engine.add_text_level_rule(Arc::new(crate::de::GermanStyleRepeatedWordRule::new()));
        engine.add_text_level_rule(Arc::new(crate::de::StyleTooOftenUsedWordRule::verb()));
        engine.add_text_level_rule(Arc::new(crate::de::StyleTooOftenUsedWordRule::noun()));
        engine.add_text_level_rule(Arc::new(crate::de::StyleTooOftenUsedWordRule::adjective()));
        engine.add_text_level_rule(Arc::new(crate::de::StatisticSentenceStyleRule::new(
            crate::de::SentenceCondition::Passive,
        )));
        engine.add_text_level_rule(Arc::new(crate::de::StatisticSentenceStyleRule::new(
            crate::de::SentenceCondition::ModalVerb,
        )));
        engine.add_text_level_rule(Arc::new(crate::de::StatisticSentenceStyleRule::new(
            crate::de::SentenceCondition::Man,
        )));
        engine.add_text_level_rule(Arc::new(crate::de::StatisticSentenceStyleRule::new(
            crate::de::SentenceCondition::ConjunctionAtBegin,
        )));
        engine.add_text_level_rule(Arc::new(crate::de::StatisticWordStyleRule::new(
            crate::de::WordCondition::FillerWords,
        )));
        engine.add_text_level_rule(Arc::new(crate::de::StatisticWordStyleRule::new(
            crate::de::WordCondition::NonSignificantVerbs,
        )));
        engine.add_text_level_rule(Arc::new(crate::de::StatisticWordStyleRule::new(
            crate::de::WordCondition::UnnecessaryPhrases,
        )));

        engine.add_rule(Arc::new(crate::de::german_double_punctuation_rule()));
        engine.add_rule(Arc::new(
            og_rules::native_rules::CommaWhitespaceRule::new().with_domain_after_dot_exception(),
        ));
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
            og_rules::text_level_rules::GenericUnpairedQuotesRule::new()
                .with_id("DE_UNPAIRED_QUOTES")
                .with_quote_symbols(vec![
                    ("\u{201E}".to_string(), "\u{201C}".to_string()),
                    ("\u{00BB}".to_string(), "\u{00AB}".to_string()),
                    ("\u{00AB}".to_string(), "\u{00BB}".to_string()),
                    ("\"".to_string(), "\"".to_string()),
                    ("'".to_string(), "'".to_string()),
                    ("\u{201A}".to_string(), "\u{2018}".to_string()),
                    ("\u{203A}".to_string(), "\u{2039}".to_string()),
                    ("\u{2039}".to_string(), "\u{203A}".to_string()),
                ]),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::WhiteSpaceBeforeParagraphEndRule::new(),
        ));
        engine.add_rule(Arc::new(
            og_rules::text_level_rules::WhiteSpaceAtBeginOfParagraphRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(og_rules::text_level_rules::EmptyLineRule::new()));
        engine.add_text_level_rule(Arc::new(crate::de::GermanLongParagraphRule::new()));
        engine.add_text_level_rule(Arc::new(
            crate::de::GermanPunctuationMarkAtParagraphEnd::new(),
        ));
        engine.add_text_level_rule(Arc::new(og_rules::text_level_rules::LongSentenceRule::new(
            40,
        )));
        engine.add_text_level_rule(Arc::new(crate::de::GermanWordRepeatBeginningRule::new()));
        engine.add_text_level_rule(Arc::new(crate::de::StyleRepeatedVeryShortSentences::new()));
        engine.add_text_level_rule(Arc::new(crate::de::StyleRepeatedSentenceBeginning::new()));

        engine.post_chunker = Some(og_tagger::german_chunker::shared_german_chunker());

        if let Some(model) = og_languagemodel::DiskLanguageModel::shared_for_language("de") {
            engine.add_rule(Arc::new(
                crate::upper_case_ngram::GermanUpperCaseNgramRule::new(model.clone()),
            ));
            engine.add_rule(Arc::new(
                crate::confusion_probability::ConfusionRule::new_german(model.clone()),
            ));
            engine.add_rule(Arc::new(crate::de::ProhibitedCompoundRule::new(
                model,
                german_speller.clone(),
            )));
        }

        engine.rebuild_checker();
        engine
    }

    pub fn german_variant(&self, code: &str) -> Self {
        let variant_dir = match code {
            "de-AT" => "de-DE-AT",
            "de-CH" => "de-CH",
            _ => "de-DE-AT",
        };
        let mut rules: Vec<Arc<dyn Rule>> = self.rules.clone();
        if let Some(lt_root) = find_lt_resource_dir() {
            let path = lt_root.join(format!(
                "de/src/main/resources/org/languagetool/rules/de/{variant_dir}/grammar.xml"
            ));
            if path.exists() {
                let compiler = XmlCompiler::new();
                if let Ok(xml) = std::fs::read_to_string(&path)
                    && let Ok(rule_set) = compiler.compile_file(&xml)
                {
                    rules.extend(rule_set.rules.into_iter().map(|compiled| {
                        Arc::new(XmlPatternRule::with_spelling(
                            compiled,
                            self.spelling_support.clone(),
                        )) as Arc<dyn Rule>
                    }));
                }
            }
        }
        let country = code.rsplit('-').next().unwrap_or("DE").to_string();
        rules.retain(|r| r.id() != "OLD_SPELLING_RULE");
        rules.push(Arc::new(crate::de::OldSpellingRule::new(&country)));
        let mut dict = og_spell::Dictionary::new();
        if let Some(lt_root) = find_lt_resource_dir() {
            let hunspell_dir =
                lt_root.join("de/src/main/resources/org/languagetool/resource/de/hunspell");
            if let Ok(words) = std::fs::read_to_string(hunspell_dir.join("spelling.txt")) {
                for line in words.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        dict.add_word(word);
                    }
                }
            }
            if let Ok(words) =
                std::fs::read_to_string(hunspell_dir.join(format!("spelling-de-{country}.txt")))
            {
                for line in words.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        dict.add_word(word);
                    }
                }
            }
        }
        if let Some(lt_root) = find_lt_resource_dir()
            && let Some(words) = load_german_wordlist(
                &lt_root.join("de/src/main/resources/org/languagetool/resource/de"),
            )
        {
            for word in &words {
                dict.add_word(word);
            }
        }
        if !dict.is_empty() {
            let id = format!("MORFOLOGIK_RULE_DE_{country}");
            rules.retain(|r| r.id() != "MORFOLOGIK_RULE_DE_DE");
            let variant_code = format!("de-{country}");
            if let Some(speller) = &self.german_speller {
                let variant_speller =
                    Arc::new(speller.with_shared_data(Arc::new(dict), &variant_code));
                rules.push(Arc::new(og_spell::GermanSpellerRule::new(
                    variant_speller,
                    id,
                )));
            } else {
                rules.push(Arc::new(og_spell::SpellingCheckRule::new_shared(
                    Arc::new(dict),
                    id,
                    HashSet::new(),
                )));
            }
        }
        let mut engine = Self::new(Language::from_code(code).unwrap_or(Language::German));
        engine.rules = rules;
        engine.text_level_rules = self.text_level_rules.clone();
        engine.tagger = self.tagger.clone();
        engine.disambiguator = self.disambiguator.clone();
        engine.spelling_support = self.spelling_support.clone();
        engine.german_speller = self.german_speller.clone();
        engine.synthesizer = self.synthesizer.clone();
        engine.post_chunker = self.post_chunker.clone();
        engine.rebuild_checker();
        engine
    }

    pub fn for_code(code: &str) -> Option<Self> {
        let language = Language::from_code(code)?;
        let short = language.short_code().to_string();
        let config = match short.as_str() {
            "da" => ("danish", "HUNSPELL_RULE"),
            "fr" => ("french", "FR_SPELLING_RULE"),
            "es" => ("spanish", "MORFOLOGIK_RULE_ES"),
            "pt" => ("portuguese", "MORFOLOGIK_RULE_PT_PT"),
            "nl" => ("dutch", "MORFOLOGIK_RULE_NL_NL"),
            "pl" => ("polish", "MORFOLOGIK_RULE_PL_PL"),
            "ru" => ("russian", "MORFOLOGIK_RULE_RU_RU"),
            "it" => ("italian", "MORFOLOGIK_RULE_IT_IT"),
            "ar" => ("arabic", "HUNSPELL_RULE_AR"),
            "ast" => ("asturian", "MORFOLOGIK_RULE_AST"),
            "be" => ("", "MORFOLOGIK_RULE_BE_BY"),
            "br" => ("breton", "MORFOLOGIK_RULE_BR_FR"),
            "el" => ("greek", "MORFOLOGIK_RULE_EL_GR"),
            "gl" => ("galician", "HUNSPELL_RULE"),
            "ro" => ("romanian", "MORFOLOGIK_RULE_RO_RO"),
            "sv" => ("swedish", "HUNSPELL_RULE"),
            _ => return None,
        };

        let mut engine = Self::new(language);

        let Some(lt_root) = find_lt_resource_dir() else {
            return Some(engine);
        };
        let resource_dir = lt_root.join(format!(
            "{short}/src/main/resources/org/languagetool/resource/{short}"
        ));
        let tagger = if short == "fr" {
            og_tagger::GenericTagger::french(&resource_dir)
        } else if short == "ru" {
            og_tagger::GenericTagger::russian(&resource_dir)
        } else {
            og_tagger::GenericTagger::from_resource_dir(&resource_dir, config.0)
        };
        if short == "ar" && tagger.has_dictionary() {
            let resource = resource_dir.clone();
            engine.tagger = Some(Arc::new(
                og_tagger::arabic_tagger::ArabicTagger::from_resource_dir(&resource),
            ));
        }
        let has_manual = resource_dir.join("added.txt").exists();
        if engine.tagger.is_none() && (tagger.has_dictionary() || has_manual) {
            engine.tagger = Some(Arc::new(tagger));
        }
        if short == "ru" {
            engine.post_chunker = Some(og_tagger::russian_chunker::shared_russian_chunker());
        }
        let disambig_path = resource_dir.join("disambiguation.xml");
        if disambig_path.exists()
            && let Ok(xml) = std::fs::read_to_string(&disambig_path)
            && let Ok(disambiguator) = XmlDisambiguator::from_xml(&xml)
            && let Some(core_resource_dir) = find_lt_core_resource_dir()
        {
            engine.disambiguator = Some(og_tagger::hybrid_disambiguator(
                &short,
                &resource_dir,
                &core_resource_dir,
                disambiguator,
            ));
        }

        let mut dict = wordlist_dictionary(&resource_dir, config.0);
        if let Ok(extra) = std::fs::read_to_string(resource_dir.join("hunspell/spelling.txt")) {
            for line in extra.lines() {
                let word = line.trim();
                if !word.is_empty() && !word.starts_with('#') {
                    dict.add_word(word);
                }
            }
        }
        let mut ignore_words = HashSet::new();
        if let Ok(words) = std::fs::read_to_string(resource_dir.join("hunspell/ignore.txt")) {
            for line in words.lines() {
                let word = line.trim();
                if !word.is_empty() && !word.starts_with('#') {
                    ignore_words.insert(word.to_lowercase());
                }
            }
        }
        let mut prohibit = HashSet::new();
        if let Ok(words) = std::fs::read_to_string(resource_dir.join("hunspell/prohibit.txt")) {
            for line in words.lines() {
                let word = line.trim();
                if !word.is_empty() && !word.starts_with('#') {
                    prohibit.insert(word.to_string());
                }
            }
        }
        if !dict.is_empty() {
            let dict = Arc::new(dict);
            engine.add_rule(Arc::new(
                og_spell::SpellingCheckRule::new_shared(dict.clone(), config.1, HashSet::new())
                    .with_ignore_words(ignore_words),
            ));
            if let Some(core_resource_dir) = find_lt_core_resource_dir() {
                engine.spelling_support = Some(Arc::new(
                    og_rules::spelling_support::SpellingSupport::new_for_language(
                        dict,
                        &core_resource_dir,
                        &resource_dir,
                        &short,
                    ),
                ));
            }
        }
        engine.synthesizer =
            og_rules::postponed_adjective::LangSynthesizer::for_language(&resource_dir, &short)
                .map(Arc::new);

        let rules_dir = lt_root.join(format!(
            "{short}/src/main/resources/org/languagetool/rules/{short}"
        ));
        for file in ["grammar.xml", "style.xml"] {
            let path = rules_dir.join(file);
            if !path.exists() {
                continue;
            }
            match engine.load_xml_file(&path) {
                Ok(count) => eprintln!("Loaded {} rules from {short}/{file}", count),
                Err(e) => eprintln!("Warning: failed to load {short}/{file}: {e}"),
            }
        }

        engine.add_rules([
            Arc::new(og_rules::native_rules::DoublePunctuationRule::new()) as Arc<dyn Rule>,
            Arc::new(og_rules::native_rules::CommaWhitespaceRule::new()),
        ]);
        let brackets_rule: Arc<dyn TextLevelRule> = if short == "it" {
            Arc::new(
                og_rules::text_level_rules::GenericUnpairedBracketsRule::new().with_symbols(
                    vec!["[", "(", "{", "\u{bb}", "\u{ab}"],
                    vec!["]", ")", "}", "\u{ab}", "\u{bb}"],
                ),
            )
        } else {
            Arc::new(og_rules::text_level_rules::GenericUnpairedBracketsRule::new())
        };
        engine.add_text_level_rules([
            Arc::new(og_rules::text_level_rules::MultipleWhitespaceRule::new())
                as Arc<dyn TextLevelRule>,
            Arc::new(og_rules::text_level_rules::SentenceWhitespaceRule::new()),
            brackets_rule,
        ]);

        match short.as_str() {
            "it" => {
                engine.add_rules([
                    Arc::new(og_rules::native_rules::WhitespaceBeforePunctuationRule::new())
                        as Arc<dyn Rule>,
                    Arc::new(og_rules::native_rules::UppercaseSentenceStartRule::new()),
                    Arc::new(crate::it::ItalianWordRepeatRule::new()),
                ]);
            }
            "ro" => {
                engine.add_rules([
                    Arc::new(crate::ro::romanian_replace_rule()) as Arc<dyn Rule>,
                    Arc::new(crate::ro::romanian_compound_rule()),
                ]);
            }
            "sv" => {
                engine.add_rule(Arc::new(crate::sv::swedish_compound_rule()) as Arc<dyn Rule>);
                engine.add_text_level_rule(Arc::new(crate::sv::swedish_word_coherency_rule()));
            }
            "ar" => {
                engine.add_rules([
                    Arc::new(crate::ar::arabic_double_punctuation_rule()) as Arc<dyn Rule>,
                    Arc::new(crate::ar::arabic_comma_whitespace_rule()),
                    Arc::new(crate::ar::arabic_question_mark_whitespace_rule()),
                    Arc::new(crate::ar::arabic_semicolon_whitespace_rule()),
                    Arc::new(crate::ar::arabic_word_repeat_rule()),
                    Arc::new(crate::ar::arabic_darja_rule()),
                    Arc::new(crate::ar::arabic_diacritics_rule()),
                    Arc::new(crate::ar::arabic_simple_replace_rule()),
                    Arc::new(crate::ar::arabic_homophones_rule()),
                    Arc::new(crate::ar::arabic_redundancy_rule()),
                    Arc::new(crate::ar::arabic_wordiness_rule()),
                ]);
            }
            _ => {}
        }

        engine.rebuild_checker();
        Some(engine)
    }

    pub fn load_xml_rules(&mut self, xml: &str) -> Result<usize, String> {
        self.load_xml_rules_with_source(xml, None)
    }

    pub fn load_xml_file(&mut self, path: &Path) -> Result<usize, String> {
        let xml = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let source_file = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string);
        self.load_xml_rules_with_base(&xml, source_file.as_deref(), path.parent())
    }

    fn load_xml_rules_with_source(
        &mut self,
        xml: &str,
        source_file: Option<&str>,
    ) -> Result<usize, String> {
        self.load_xml_rules_with_base(xml, source_file, None)
    }

    fn load_xml_rules_with_base(
        &mut self,
        xml: &str,
        source_file: Option<&str>,
        base_dir: Option<&Path>,
    ) -> Result<usize, String> {
        let compiler = XmlCompiler::new();
        let rule_set = compiler
            .compile_file_with_base(xml, base_dir)
            .map_err(|e| format!("XML compilation error: {}", e))?;

        let count = rule_set.rules.len();
        for mut compiled_rule in rule_set.rules {
            compiled_rule.source_file = source_file.map(str::to_string);
            let rule = Arc::new(XmlPatternRule::with_resources(
                compiled_rule,
                self.spelling_support.clone(),
                self.synthesizer.clone(),
            ));
            self.rules.push(rule);
        }

        self.rebuild_checker();
        Ok(count)
    }

    pub fn add_rule(&mut self, rule: Arc<dyn Rule>) {
        self.rules.push(rule);
        self.rebuild_checker();
    }

    pub fn add_rules(&mut self, rules: impl IntoIterator<Item = Arc<dyn Rule>>) {
        self.rules.extend(rules);
        self.rebuild_checker();
    }

    pub fn add_text_level_rule(&mut self, rule: Arc<dyn TextLevelRule>) {
        self.text_level_rules.push(rule);
        self.rebuild_checker();
    }

    pub fn add_text_level_rules(
        &mut self,
        rules: impl IntoIterator<Item = Arc<dyn TextLevelRule>>,
    ) {
        self.text_level_rules.extend(rules);
        self.rebuild_checker();
    }

    fn rebuild_checker(&mut self) {
        let mut checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge(
                self.language.short_code().to_string(),
            )))
            .with_word_tokenizer(Arc::new(WordTokenizerBridge::for_code(
                self.language.short_code(),
            )))
            .with_language_filter(self.language_filter.clone())
            .set_rules(self.rules.clone())
            .set_text_level_rules(self.text_level_rules.clone());

        if let Some(ref tagger) = self.tagger {
            checker = checker
                .with_tagger(tagger.clone())
                .with_chunker(Arc::new(og_tagger::chunker::PostDisambiguationChunker));
        }

        if let Some(ref disambiguator) = self.disambiguator {
            checker = checker.with_disambiguator(disambiguator.clone());
        }

        if let Some(ref chunker) = self.post_chunker {
            checker = checker.with_post_chunker(chunker.clone());
        }

        self.checker = checker;
    }

    pub fn check(&self, text: &str) -> CheckResult {
        let request = CheckRequest::new(text, self.language.clone());
        self.checker.check(&request)
    }

    pub fn check_with_request(&self, request: &CheckRequest) -> CheckResult {
        let short_code = match request.mother_tongue.as_ref() {
            Some(mt) => mt.short_code().to_string(),
            None => return self.checker.check(request),
        };
        {
            let cache = self.l2_checkers.lock().unwrap();
            if let Some(checker) = cache.get(&short_code) {
                return checker.check(request);
            }
        }
        let l2 = self.l2_rules(&short_code);
        if l2.is_empty() {
            return self.checker.check(request);
        }
        let mut rules = self.rules.clone();
        rules.extend(l2);
        let checker = self.build_checker(rules);
        let result = checker.check(request);
        let mut cache = self.l2_checkers.lock().unwrap();
        cache.insert(short_code, checker);
        result
    }

    fn l2_rules(&self, mother_tongue_code: &str) -> Vec<Arc<dyn Rule>> {
        {
            let cache = self.l2_rules.lock().unwrap();
            if let Some(rules) = cache.get(mother_tongue_code) {
                return rules.clone();
            }
        }
        let loaded = self.load_l2_rules(mother_tongue_code);
        let mut cache = self.l2_rules.lock().unwrap();
        cache.insert(mother_tongue_code.to_string(), loaded.clone());
        loaded
    }

    fn load_l2_rules(&self, mother_tongue_code: &str) -> Vec<Arc<dyn Rule>> {
        let Some(lt_root) = find_lt_resource_dir() else {
            return Vec::new();
        };
        let module = self.language.short_code();
        let path = lt_root
            .join(format!(
                "{}/src/main/resources/org/languagetool/rules/{}",
                module, module
            ))
            .join(format!("grammar-l2-{}.xml", mother_tongue_code));
        if !path.exists() {
            return Vec::new();
        }
        let compiler = XmlCompiler::new();
        let xml = match std::fs::read_to_string(&path) {
            Ok(xml) => xml,
            Err(_) => return Vec::new(),
        };
        let rule_set = match compiler.compile_file(&xml) {
            Ok(rule_set) => rule_set,
            Err(_) => return Vec::new(),
        };
        let mut loaded: Vec<Arc<dyn Rule>> = rule_set
            .rules
            .into_iter()
            .map(|compiled| {
                Arc::new(XmlPatternRule::with_spelling(
                    compiled,
                    self.spelling_support.clone(),
                )) as Arc<dyn Rule>
            })
            .collect();
        if self.language.short_code() == "en"
            && let Some(model) = og_languagemodel::DiskLanguageModel::shared_for_language("en")
        {
            loaded.push(Arc::new(
                crate::confusion_probability::ConfusionRule::new_english_for_l2(
                    model,
                    mother_tongue_code,
                ),
            ));
        }
        loaded
    }

    fn build_checker(&self, rules: Vec<Arc<dyn Rule>>) -> Checker {
        let mut checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge(
                self.language.short_code().to_string(),
            )))
            .with_word_tokenizer(Arc::new(WordTokenizerBridge::for_code(
                self.language.short_code(),
            )))
            .with_language_filter(self.language_filter.clone())
            .set_rules(rules)
            .set_text_level_rules(self.text_level_rules.clone());

        if let Some(ref tagger) = self.tagger {
            checker = checker
                .with_tagger(tagger.clone())
                .with_chunker(Arc::new(og_tagger::chunker::PostDisambiguationChunker));
        }
        if let Some(ref disambiguator) = self.disambiguator {
            checker = checker.with_disambiguator(disambiguator.clone());
        }
        checker
    }

    pub fn variant(&self, code: &str) -> Self {
        let country = code.rsplit('-').next().unwrap_or("US").to_uppercase();
        let mut extra_words = HashSet::new();
        let mut ignore_words = HashSet::new();
        let mut shared_dict: Option<Arc<og_spell::Dictionary>> = None;
        if let Some(lt_root) = find_lt_resource_dir() {
            let hunspell_dir =
                lt_root.join("en/src/main/resources/org/languagetool/resource/en/hunspell");
            if let Ok(words) =
                std::fs::read_to_string(hunspell_dir.join(format!("spelling_en-{country}.txt")))
            {
                for line in words.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        extra_words.insert(word.to_string());
                    }
                }
            }
            if let Ok(words) = std::fs::read_to_string(hunspell_dir.join("ignore.txt")) {
                for line in words.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        ignore_words.insert(word.to_lowercase());
                    }
                }
            }
            shared_dict = Some(Arc::new(variant_dictionary(&hunspell_dir, &country)));
        }

        let mut rules: Vec<Arc<dyn Rule>> = self
            .rules
            .iter()
            .filter(|r| !r.id().starts_with("MORFOLOGIK_RULE_EN_"))
            .cloned()
            .collect();
        if let Some(lt_root) = find_lt_resource_dir() {
            let variant_dir = lt_root.join("en/src/main/resources/org/languagetool/rules/en");
            for file in [format!("{code}/grammar.xml"), format!("{code}/style.xml")] {
                let path = variant_dir.join(file);
                if !path.exists() {
                    continue;
                }
                let compiler = XmlCompiler::new();
                if let Ok(xml) = std::fs::read_to_string(&path)
                    && let Ok(rule_set) = compiler.compile_file(&xml)
                {
                    rules.extend(rule_set.rules.into_iter().map(|compiled| {
                        Arc::new(XmlPatternRule::with_spelling(
                            compiled,
                            self.spelling_support.clone(),
                        )) as Arc<dyn Rule>
                    }));
                }
            }
        }
        if let Some(dict) = shared_dict {
            let id = format!("MORFOLOGIK_RULE_EN_{country}");
            let spell_rule = SpellingCheckRule::new_shared(dict, id, extra_words)
                .with_ignore_words(ignore_words);
            rules.push(Arc::new(spell_rule));
        }

        let mut engine =
            Self::new(Language::from_code(code).unwrap_or_else(|| self.language.clone()));
        engine.rules = rules;
        engine.text_level_rules = self.text_level_rules.clone();
        engine.tagger = self.tagger.clone();
        engine.disambiguator = self.disambiguator.clone();
        engine.spelling_support = self.spelling_support.clone();
        engine.rebuild_checker();
        engine
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn rule_ids(&self) -> Vec<&str> {
        self.rules.iter().map(|r| r.id()).collect()
    }

    pub fn language(&self) -> &Language {
        &self.language
    }
}

fn load_german_wordlist(resource_dir: &Path) -> Option<Vec<String>> {
    let dict = og_tagger::german_tagger::shared_german_dictionary(resource_dir)?;
    Some(dict.word_list())
}

fn wordlist_dictionary(resource_dir: &Path, dict_stem: &str) -> og_spell::Dictionary {
    match og_tagger::generic_tagger::shared_dictionary(resource_dir, dict_stem) {
        Some(shared) => og_spell::Dictionary::from_shared_words(Arc::new(
            og_spell::morfologik_view::MorfologikView::new(shared),
        )),
        None => og_spell::Dictionary::new(),
    }
}

fn variant_dictionary(hunspell_dir: &Path, country: &str) -> og_spell::Dictionary {
    let mut dict = og_spell::Dictionary::new();
    let mut sources = vec![
        format!("en_{country}.words.txt"),
        "spelling.txt".to_string(),
    ];
    sources.push(format!("spelling_en-{country}.txt"));
    for source in sources {
        if let Ok(words) = std::fs::read_to_string(hunspell_dir.join(source)) {
            for line in words.lines() {
                let word = line.trim();
                if !word.is_empty() && !word.starts_with('#') {
                    dict.add_word(word);
                }
            }
        }
    }
    dict
}

struct SentenceSplitterBridge(String);

impl SentenceTokenizer for SentenceSplitterBridge {
    fn split(&self, text: &str) -> Vec<SentenceRange> {
        use og_tokenizer::SentenceTokenizer as _;
        let sentences = match og_tokenizer::srx::srx_splitter(&self.0) {
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

enum WordTokenizerBridge {
    Default(bool),
    German,
    French(Arc<og_tokenizer::FrenchWordTokenizer>),
    Arabic,
}

impl WordTokenizerBridge {
    fn for_code(code: &str) -> Self {
        if code == "en" {
            return Self::Default(true);
        }
        if code == "de" {
            return Self::German;
        }
        if code == "ar" {
            return Self::Arabic;
        }
        if code != "fr" {
            return Self::Default(false);
        }
        static FR: std::sync::OnceLock<Option<Arc<og_tokenizer::FrenchWordTokenizer>>> =
            std::sync::OnceLock::new();
        let cached = FR.get_or_init(|| {
            let tagger = find_lt_resource_dir().map(|lt_root| {
                let dir = lt_root.join("fr/src/main/resources/org/languagetool/resource/fr");
                Arc::new(og_tagger::GenericTagger::french(&dir))
            })?;
            Some(Arc::new(og_tokenizer::FrenchWordTokenizer::new(Arc::new(
                move |w: &str| tagger.is_word_tagged(w),
            ))))
        });
        match cached {
            Some(fr) => Self::French(fr.clone()),
            None => Self::Default(false),
        }
    }
}

impl WordTokenizer for WordTokenizerBridge {
    fn tokenize(&self, text: &str, offset: usize) -> Vec<AnalyzedTokenReadings> {
        use og_tokenizer::WordTokenizer as _;
        let tokens = match self {
            Self::Default(true) => {
                let tokenizer = og_tokenizer::DefaultWordTokenizer::new();
                tokenizer.split_underscores(tokenizer.tokenize(text))
            }
            Self::Default(false) => {
                og_tokenizer::DefaultWordTokenizer::new_generic().tokenize(text)
            }
            Self::German => og_tokenizer::GermanWordTokenizer::new().tokenize(text),
            Self::French(fr) => fr.tokenize(text),
            Self::Arabic => {
                og_tokenizer::DefaultWordTokenizer::new_splitting_hyphen().tokenize(text)
            }
        };
        tokens
            .into_iter()
            .map(|t| {
                let at = AnalyzedToken::new(t.text(), t.start() + offset, t.end() + offset);
                AnalyzedTokenReadings::new(at)
            })
            .collect()
    }
}

pub fn find_lt_core_resource_dir() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from(
            "languagetool/languagetool-core/src/main/resources/org/languagetool/resource",
        ),
        PathBuf::from(
            "../languagetool/languagetool-core/src/main/resources/org/languagetool/resource",
        ),
        PathBuf::from(
            "/home/agent/opengrammar-rs/languagetool/languagetool-core/src/main/resources/org/languagetool/resource",
        ),
    ];
    for candidate in &candidates {
        if candidate.exists() {
            return candidate.canonicalize().ok();
        }
    }
    None
}

pub fn find_lt_resource_dir() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("languagetool/languagetool-language-modules"),
        PathBuf::from("../languagetool/languagetool-language-modules"),
        PathBuf::from("../languagetool-language-modules"),
        PathBuf::from("../../languagetool-language-modules"),
        PathBuf::from("/home/agent/opengrammar-rs/languagetool/languagetool-language-modules"),
    ];
    for candidate in &candidates {
        if candidate.exists() {
            return candidate.canonicalize().ok();
        }
    }
    None
}

pub fn grammar_xml_path(lang_code: &str, lt_root: &Path) -> PathBuf {
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
    use std::sync::{Arc, OnceLock};

    fn shared_english() -> Arc<LanguageEngine> {
        static ENGINE: OnceLock<Arc<LanguageEngine>> = OnceLock::new();
        ENGINE
            .get_or_init(|| Arc::new(LanguageEngine::english()))
            .clone()
    }

    fn shared_german() -> Arc<LanguageEngine> {
        static ENGINE: OnceLock<Arc<LanguageEngine>> = OnceLock::new();
        ENGINE
            .get_or_init(|| Arc::new(LanguageEngine::german()))
            .clone()
    }

    #[test]
    fn matches_in_later_sentences_keep_absolute_offsets() {
        let engine = shared_german();
        let text = "Das ist ein vollkommen korrekter Satz. Dies ist ein fhcsal.";
        let typo_at = text.find("fhcsal").expect("test text contains typo");
        let result = engine.check(text);
        let hit = result
            .matches
            .iter()
            .find(|m| m.length() == "fhcsal".len() && m.offset() == typo_at);
        assert!(
            hit.is_some(),
            "expected a match at byte {typo_at} for the typo in the second sentence, got {:?}",
            result
                .matches
                .iter()
                .map(|m| (m.rule().id().to_string(), m.offset(), m.length()))
                .collect::<Vec<_>>()
        );
        let hit = hit.expect("checked above");
        assert_eq!(&text[hit.offset()..hit.offset() + hit.length()], "fhcsal");
    }

    #[test]
    fn mother_tongue_adds_l2_rules_to_the_check() {
        let base = shared_english();
        let plain = base.rules.len();
        let mut request = CheckRequest::new("Test", Language::English);
        request.mother_tongue = Some(Language::German);
        let _ = base.check_with_request(&request);
        let cached = base
            .l2_rules
            .lock()
            .unwrap()
            .get("de")
            .cloned()
            .unwrap_or_default();
        assert!(
            !cached.is_empty(),
            "expected grammar-l2-de.xml rules for motherTongue=de"
        );
        assert!(cached.len() <= plain || plain > 0);
    }

    #[test]
    fn paragraph_rules_fire_through_the_srx_pipeline() {
        let engine = shared_english();
        let mut request = CheckRequest::new("Hello world. \n\nNext paragraph", Language::English);
        request.enabled_rules = vec![
            "WHITESPACE_PARAGRAPH".to_string(),
            "WHITESPACE_PARAGRAPH_BEGIN".to_string(),
        ];
        request.picky = true;
        let result = engine.check_with_request(&request);
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            ids.contains(&"WHITESPACE_PARAGRAPH"),
            "expected WHITESPACE_PARAGRAPH in {ids:?}"
        );
    }

    #[test]
    fn danish_engine_registers_with_tagger_and_rules() {
        let engine =
            LanguageEngine::for_code("da").expect("Danish engine must be registered via for_code");
        assert!(
            engine.rule_count() > 10,
            "expected Danish rules to load, got {}",
            engine.rule_count()
        );
        assert!(
            engine.tagger.is_some(),
            "expected the Danish morfologik tagger to load"
        );
        let result = engine.check("Dette er en tes.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            ids.contains(&"HUNSPELL_RULE"),
            "expected Danish speller to fire on a typo, got {ids:?}"
        );
    }

    #[test]
    fn romanian_engine_registers_with_tagger_and_natives() {
        let engine = LanguageEngine::for_code("ro")
            .expect("Romanian engine must be registered via for_code");
        assert!(
            engine.rule_count() > 10,
            "expected Romanian rules to load, got {}",
            engine.rule_count()
        );
        assert!(
            engine.tagger.is_some(),
            "expected the Romanian morfologik tagger to load"
        );
        let result = engine.check("Nu am fost acasã.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            ids.contains(&"MORFOLOGIK_RULE_RO_RO"),
            "expected the Romanian speller to fire on a typo, got {ids:?}"
        );
    }

    #[test]
    fn swedish_engine_registers_with_tagger_and_natives() {
        let engine =
            LanguageEngine::for_code("sv").expect("Swedish engine must be registered via for_code");
        assert!(
            engine.rule_count() > 10,
            "expected Swedish rules to load, got {}",
            engine.rule_count()
        );
        assert!(
            engine.tagger.is_some(),
            "expected the Swedish morfologik tagger to load"
        );
        let result = engine.check("Han arbetar med datorrz.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            ids.contains(&"HUNSPELL_RULE"),
            "expected the Swedish speller to fire on a typo, got {ids:?}"
        );
    }

    #[test]
    fn russian_engine_registers_tagger_normalization_and_chunker() {
        let engine =
            LanguageEngine::for_code("ru").expect("Russian engine must be registered via for_code");
        assert!(
            engine.rule_count() > 10,
            "expected Russian rules to load, got {}",
            engine.rule_count()
        );
        assert!(
            engine.tagger.is_some(),
            "expected the Russian morfologik tagger to load"
        );
        assert!(
            engine.post_chunker.is_some(),
            "expected the Russian post-disambiguation chunker"
        );
        let result = engine.check("Крепкий стол.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            !ids.contains(&"Unify_Adj_NN_case"),
            "correct ADJ+NN pair must not be flagged, got {ids:?}"
        );
    }

    #[test]
    fn spelling_global_chunker_shields_speller_and_replace_rules() {
        let engine = shared_english();
        for text in [
            "There is Givenchy here.",
            "There is Frito-Lay here.",
            "We met Emmanuel Macron yesterday.",
        ] {
            let mut request = CheckRequest::new(text, Language::English);
            request.enabled_rules = vec!["MORFOLOGIK_RULE_EN_US".to_string()];
            let result = engine.check_with_request(&request);
            let hits: Vec<usize> = result.matches.iter().map(|m| m.offset()).collect();
            assert!(
                hits.is_empty(),
                "spelling_global entry must not be flagged for {text:?}, got {hits:?}"
            );
        }
    }

    #[test]
    fn italian_engine_wires_java_relevant_natives() {
        let engine =
            LanguageEngine::for_code("it").expect("Italian engine must be registered via for_code");
        let ids = engine.rule_ids();
        for expected in [
            "WHITESPACE_PUNCTUATION",
            "UPPERCASE_SENTENCE_START",
            "ITALIAN_WORD_REPEAT_RULE",
        ] {
            assert!(
                ids.contains(&expected),
                "expected {expected} in Italian natives, got {ids:?}"
            );
        }

        let result = engine.check("questa \u{e8} una prova : con due punti");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            ids.contains(&"WHITESPACE_PUNCTUATION"),
            "expected WHITESPACE_PUNCTUATION on space before colon, got {ids:?}"
        );
        assert!(
            ids.contains(&"UPPERCASE_SENTENCE_START"),
            "expected UPPERCASE_SENTENCE_START on lowercase start, got {ids:?}"
        );

        let result = engine.check("Il lessico si andava via via modificando.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            !ids.contains(&"ITALIAN_WORD_REPEAT_RULE"),
            "'via via' must be ignored, got {ids:?}"
        );
    }

    #[test]
    fn arabic_engine_wires_java_relevant_natives() {
        let engine =
            LanguageEngine::for_code("ar").expect("Arabic engine must be registered via for_code");
        let ids = engine.rule_ids();
        for expected in [
            "ARABIC_DOUBLE_PUNCTUATION",
            "ARABIC_COMMA_PARENTHESIS_WHITESPACE",
            "ARABIC_QM_WHITESPACE",
            "ARABIC_SC_WHITESPACE",
            "ARABIC_WORD_REPEAT_RULE",
            "AR_DARJA_REPLACE",
            "AR_DIACRITICS_REPLACE",
        ] {
            assert!(
                ids.contains(&expected),
                "expected {expected} in Arabic natives, got {ids:?}"
            );
        }

        let result = engine.check("لقد.. نجحنا.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            ids.contains(&"ARABIC_DOUBLE_PUNCTUATION"),
            "expected ARABIC_DOUBLE_PUNCTUATION on '..', got {ids:?}"
        );

        let result = engine.check("نعم ؟ لقد نجحنا.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            ids.contains(&"ARABIC_QM_WHITESPACE"),
            "expected ARABIC_QM_WHITESPACE on space before '؟', got {ids:?}"
        );

        let result = engine.check("نفذت التعليمات خطوة خطوة.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            !ids.contains(&"ARABIC_WORD_REPEAT_RULE"),
            "'خطوة خطوة' must be ignored, got {ids:?}"
        );
    }

    #[test]
    fn german_engine_loads_rules_and_checks_text() {
        let engine = shared_german();
        assert!(
            engine.rule_count() > 100,
            "expected German rules to load, got {}",
            engine.rule_count()
        );
        let result = engine.check("Der Haus ist gross.");
        let ids: Vec<&str> = result.matches.iter().map(|m| m.rule().id()).collect();
        assert!(
            !ids.is_empty(),
            "expected German rules to fire on 'Der Haus ist gross.', got none"
        );
    }

    #[test]
    fn variant_dictionaries_follow_java_hunspell_differences() {
        let lt_root = match find_lt_resource_dir() {
            Some(p) => p,
            None => return,
        };
        let hunspell_dir =
            lt_root.join("en/src/main/resources/org/languagetool/resource/en/hunspell");
        let us = variant_dictionary(&hunspell_dir, "US");
        let gb = variant_dictionary(&hunspell_dir, "GB");
        assert!(us.contains("color"));
        assert!(!us.contains("colour"));
        assert!(gb.contains("colour"));
        assert!(us.len() > 100_000 && gb.len() > 100_000);
    }

    #[test]
    fn german_variants_carry_their_own_morfologik_rule_and_variant_grammar() {
        let base = shared_german();
        let base_ids: Vec<String> = base.rules.iter().map(|r| r.id().to_string()).collect();
        assert!(base_ids.iter().any(|i| i == "MORFOLOGIK_RULE_DE_DE"));

        let ch = base.german_variant("de-CH");
        let ch_ids: Vec<String> = ch.rules.iter().map(|r| r.id().to_string()).collect();
        assert!(
            ch_ids.iter().any(|i| i == "MORFOLOGIK_RULE_DE_CH"),
            "de-CH must swap in its own morfologik rule"
        );
        assert!(!ch_ids.iter().any(|i| i == "MORFOLOGIK_RULE_DE_DE"));
        assert!(
            ch_ids.iter().any(|i| i == "GRUESSE_CH_SPELLING_RULE"),
            "de-CH must load de-CH/grammar.xml rules"
        );
        assert!(
            !ch_ids.iter().any(|i| i == "GRUESSE_SPELLING_RULE"),
            "de-DE-AT rules must not leak into de-CH"
        );
        assert_eq!(ch.language().code(), "de-CH");

        let at = base.german_variant("de-AT");
        let at_ids: Vec<String> = at.rules.iter().map(|r| r.id().to_string()).collect();
        assert!(
            at_ids.iter().any(|i| i == "MORFOLOGIK_RULE_DE_AT"),
            "de-AT must swap in its own morfologik rule"
        );
        assert!(
            at_ids.iter().any(|i| i == "GRUESSE_SPELLING_RULE"),
            "de-AT must load de-DE-AT/grammar.xml rules"
        );
        assert!(
            !at_ids.iter().any(|i| i == "GRUESSE_CH_SPELLING_RULE"),
            "de-CH rules must not leak into de-AT"
        );
        assert_eq!(at.language().code(), "de-AT");
    }

    #[test]
    fn regional_variants_carry_their_own_morfologik_rule() {
        let base = shared_english();
        let ids: Vec<String> = base.rules.iter().map(|r| r.id().to_string()).collect();
        assert!(ids.iter().any(|i| i == "MORFOLOGIK_RULE_EN_US"));
        assert!(!ids.iter().any(|i| i == "MORFOLOGIK_RULE_EN_GB"));

        let gb = base.variant("en-GB");
        let gb_ids: Vec<String> = gb.rules.iter().map(|r| r.id().to_string()).collect();
        assert!(gb_ids.iter().any(|i| i == "MORFOLOGIK_RULE_EN_GB"));
        assert!(!gb_ids.iter().any(|i| i == "MORFOLOGIK_RULE_EN_US"));
        assert_eq!(gb.language().code(), "en-GB");
    }

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

        let engine = shared_english();
        let count = engine.rule_count();
        println!("Loaded {} rules from English grammar.xml", count);
        assert!(
            count > 4000,
            "Expected the real English grammar, got {count}"
        );

        let result = engine.check("This is a test.");
        assert_eq!(result.language.code, "en-US");
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
        assert!(!result.matches.is_empty());
    }
    #[test]
    fn xml_rules_loaded_from_file_carry_source_file() {
        let mut engine = LanguageEngine::new(Language::English);
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <rules lang="en">
            <category id="GRAMMAR" name="Grammar">
                <rule id="SRC_FILE_RULE" name="Test">
                    <pattern>
                        <token>tehest</token>
                    </pattern>
                    <message>Did you mean <suggestion>test</suggestion>?</message>
                </rule>
            </category>
        </rules>"#;
        let path = std::env::temp_dir().join("og-engine-source-file-test-grammar.xml");
        std::fs::write(&path, xml).unwrap();
        let count = engine.load_xml_file(&path).unwrap();
        assert_eq!(count, 1);

        let result = engine.check("tehest");
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].rule().id(), "SRC_FILE_RULE");
        assert_eq!(
            result.matches[0].rule().source_file(),
            Some("og-engine-source-file-test-grammar.xml")
        );
        let _ = std::fs::remove_file(&path);
    }
}
