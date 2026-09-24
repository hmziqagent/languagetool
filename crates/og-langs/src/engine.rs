use og_core::checker::{SentenceTokenizer, WordTokenizer};
use og_core::{
    AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, Category, CheckRequest, CheckResult,
    Checker, IssueType, Language, RuleMatch, SentenceRange,
    rule::{Rule, TextLevelRule},
};
use og_rules::pattern_rule::PatternRuleEngine;
use og_spell::SpellingCheckRule;
use og_tagger::{
    ChainedDisambiguator, ContextualDisambiguator, EnglishTagger, MultiWordChunkerDisambiguator,
    XmlDisambiguator,
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
        let category = Category::new(&compiled.category.id, &compiled.category.name);
        let mut engine = PatternRuleEngine::new();
        if let Some(spelling) = spelling {
            engine = engine.with_spelling_support(spelling);
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
    checker: Checker,
    l2_rules: std::sync::Mutex<HashMap<String, Vec<Arc<dyn Rule>>>>,
    l2_checkers: std::sync::Mutex<HashMap<String, Checker>>,
}

impl LanguageEngine {
    pub fn new(language: Language) -> Self {
        let checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge("en")))
            .with_word_tokenizer(Arc::new(WordTokenizerBridge));

        Self {
            language,
            rules: Vec::new(),
            text_level_rules: Vec::new(),
            tagger: None,
            disambiguator: None,
            post_chunker: None,
            spelling_support: None,
            checker,
            l2_rules: std::sync::Mutex::new(HashMap::new()),
            l2_checkers: std::sync::Mutex::new(HashMap::new()),
        }
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
            checker: Checker::new()
                .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge("en")))
                .with_word_tokenizer(Arc::new(WordTokenizerBridge)),
            l2_rules: std::sync::Mutex::new(HashMap::new()),
            l2_checkers: std::sync::Mutex::new(HashMap::new()),
        };

        let mut spelling_support: Option<Arc<og_rules::spelling_support::SpellingSupport>> = None;
        if let Some(lt_root) = find_lt_resource_dir() {
            let resource_dir = lt_root.join("en/src/main/resources/org/languagetool/resource/en");
            og_tagger::chunker::load_opennlp_chunker(&resource_dir);
            if let Some(support) = find_lt_core_resource_dir().and_then(|core_resource_dir| {
                og_rules::spelling_support::SpellingSupport::from_dirs(
                    &resource_dir,
                    &core_resource_dir,
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
                            vec![Arc::new(disambiguator), Arc::new(ContextualDisambiguator)];
                        if let Some(multiword) =
                            MultiWordChunkerDisambiguator::from_resource_dir(&resource_dir)
                        {
                            chain.insert(0, Arc::new(multiword));
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
            if det_a_path.exists()
                && det_an_path.exists()
                && let (Ok(det_a), Ok(det_an)) = (
                    std::fs::read_to_string(&det_a_path),
                    std::fs::read_to_string(&det_an_path),
                )
            {
                let mut avsan = crate::en::AvsAnRule::new();
                avsan.load_data(&det_a, &det_an);
                engine.add_rule(Arc::new(avsan));
            }

            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::english_default()));

            engine.add_rule(Arc::new(
                crate::en::SimpleReplaceRule::english_redundancies(),
            ));
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::english_profanity()));
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::american_replace()));
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::british_replace()));
            engine.add_rule(Arc::new(crate::en::SimpleReplaceRule::new_zealand_replace()));

            engine.add_rule(Arc::new(
                og_rules::native_rules::DoublePunctuationRule::new(),
            ));
            engine.add_rule(Arc::new(
                og_rules::native_rules::UppercaseSentenceStartRule::new(),
            ));
            engine.add_rule(Arc::new(og_rules::native_rules::CommaWhitespaceRule::new()));

            engine.add_rule(Arc::new(crate::en::ContractionSpellingRule::new()));
            engine.add_rule(Arc::new(crate::en::SpecificCaseRule::new()));
            engine.add_rule(Arc::new(crate::en::DiacriticsRule::new()));
            engine.add_rule(Arc::new(crate::en::PlainEnglishRule::new()));
            engine.add_rule(Arc::new(crate::en::CompoundRule::new()));
            engine.add_rule(Arc::new(crate::en::DashRule::new()));
            engine.add_rule(Arc::new(crate::en::WrongWordInContextRule::new()));
            engine.add_rule(Arc::new(crate::en::EnglishWordRepeatRule::new()));

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
                engine.add_rule(Arc::new(spell_rule));
            }
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
            og_rules::text_level_rules::GenericUnpairedQuotesRule::english(),
        ));
        engine.add_text_level_rule(Arc::new(og_rules::text_level_rules::LongSentenceRule::new(
            40,
        )));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::WordRepeatBeginningRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(crate::en::ConsistentApostrophesRule::new()));
        engine.add_text_level_rule(Arc::new(crate::en::WordCoherencyRule::new()));
        engine.add_rule(Arc::new(
            og_rules::text_level_rules::WhiteSpaceAtBeginOfParagraphRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::WhiteSpaceBeforeParagraphEndRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(og_rules::text_level_rules::EmptyLineRule::new()));
        engine.add_text_level_rule(Arc::new(crate::en::StyleFrequencyRule::adjective()));
        engine.add_text_level_rule(Arc::new(crate::en::StyleFrequencyRule::noun()));
        engine.add_text_level_rule(Arc::new(crate::en::StyleFrequencyRule::verb()));

        engine.rebuild_checker();
        engine
    }

    pub fn german() -> Self {
        let mut engine = Self::new(Language::German);

        if let Some(lt_root) = find_lt_resource_dir() {
            let resource_dir = lt_root.join("de/src/main/resources/org/languagetool/resource/de");
            if let Some(tagger) = og_tagger::GermanTagger::from_resource_dir(&resource_dir) {
                engine.tagger = Some(Arc::new(tagger));
            }
            let disambig_path = resource_dir.join("disambiguation.xml");
            if disambig_path.exists()
                && let Ok(xml) = std::fs::read_to_string(&disambig_path)
                && let Ok(disambiguator) = XmlDisambiguator::from_xml(&xml)
            {
                engine.disambiguator = Some(Arc::new(disambiguator));
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

            let mut dict = og_spell::Dictionary::new();
            if let Some(words) = load_german_wordlist(&resource_dir) {
                for word in &words {
                    dict.add_word(word);
                }
                eprintln!("Loaded {} German dictionary words", words.len());
            }
            if let Ok(extra) = std::fs::read_to_string(resource_dir.join("hunspell/spelling.txt")) {
                for line in extra.lines() {
                    let word = line.trim();
                    if !word.is_empty() && !word.starts_with('#') {
                        dict.add_word(word);
                    }
                }
            }
            if !dict.is_empty() {
                engine.add_rule(Arc::new(
                    og_spell::SpellingCheckRule::new(dict).with_id("MORFOLOGIK_RULE_DE_DE"),
                ));
            }
        }

        engine.add_rule(Arc::new(
            og_rules::native_rules::DoublePunctuationRule::new(),
        ));
        engine.add_rule(Arc::new(og_rules::native_rules::CommaWhitespaceRule::new()));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::MultipleWhitespaceRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::SentenceWhitespaceRule::new(),
        ));
        engine.add_text_level_rule(Arc::new(
            og_rules::text_level_rules::GenericUnpairedBracketsRule::new(),
        ));

        engine.post_chunker = Some(og_tagger::german_chunker::shared_german_chunker());

        engine.rebuild_checker();
        engine
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
        self.load_xml_rules_with_source(&xml, source_file.as_deref())
    }

    fn load_xml_rules_with_source(
        &mut self,
        xml: &str,
        source_file: Option<&str>,
    ) -> Result<usize, String> {
        let compiler = XmlCompiler::new();
        let rule_set = compiler
            .compile_file(xml)
            .map_err(|e| format!("XML compilation error: {}", e))?;

        let count = rule_set.rules.len();
        for mut compiled_rule in rule_set.rules {
            compiled_rule.source_file = source_file.map(str::to_string);
            let rule = Arc::new(XmlPatternRule::with_spelling(
                compiled_rule,
                self.spelling_support.clone(),
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

    pub fn add_text_level_rule(&mut self, rule: Arc<dyn TextLevelRule>) {
        self.text_level_rules.push(rule);
        self.rebuild_checker();
    }

    fn rebuild_checker(&mut self) {
        let mut checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge("en")))
            .with_word_tokenizer(Arc::new(WordTokenizerBridge))
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
        rule_set
            .rules
            .into_iter()
            .map(|compiled| {
                Arc::new(XmlPatternRule::with_spelling(
                    compiled,
                    self.spelling_support.clone(),
                )) as Arc<dyn Rule>
            })
            .collect()
    }

    fn build_checker(&self, rules: Vec<Arc<dyn Rule>>) -> Checker {
        let mut checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(SentenceSplitterBridge("en")))
            .with_word_tokenizer(Arc::new(WordTokenizerBridge))
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

    pub fn language(&self) -> &Language {
        &self.language
    }
}

fn load_german_wordlist(resource_dir: &Path) -> Option<Vec<String>> {
    let dict = og_tagger::german_tagger::shared_german_dictionary(resource_dir)?;
    Some(dict.word_list())
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

struct SentenceSplitterBridge(&'static str);

impl SentenceTokenizer for SentenceSplitterBridge {
    fn split(&self, text: &str) -> Vec<SentenceRange> {
        use og_tokenizer::SentenceTokenizer as _;
        let sentences = match og_tokenizer::srx::srx_splitter(self.0) {
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

        let mut engine = LanguageEngine::new(Language::English);
        match engine.load_xml_file(&grammar_path) {
            Ok(count) => {
                println!("Loaded {} rules from English grammar.xml", count);
                assert!(count > 0, "Expected at least some rules");

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
