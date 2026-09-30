use crate::{
    AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, CheckResult, DetectedLanguageRate,
    ExtendedSentenceRange, Language, LanguageDetectedInfo, LanguageInfo, RuleMatch, SoftwareInfo,
    Warnings, match_filters::LanguageMatchFilter, rule::Rule,
};
use std::sync::Arc;

pub use crate::rule::TextLevelRule;

#[derive(Debug, Clone)]
pub struct SentenceRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct CheckRequest {
    pub text: String,
    pub language: Language,
    pub mother_tongue: Option<Language>,
    pub enabled_rules: Vec<String>,
    pub disabled_rules: Vec<String>,
    pub enabled_categories: Vec<String>,
    pub disabled_categories: Vec<String>,
    pub level: Option<String>,
    pub picky: bool,
}

impl CheckRequest {
    pub fn new(text: impl Into<String>, language: Language) -> Self {
        Self {
            text: text.into(),
            language,
            mother_tongue: None,
            enabled_rules: Vec::new(),
            disabled_rules: Vec::new(),
            enabled_categories: Vec::new(),
            disabled_categories: Vec::new(),
            level: None,
            picky: false,
        }
    }

    pub fn rule_enabled_by_id(&self, rule_id: &str, default_on: bool, picky_tagged: bool) -> bool {
        if !self.enabled_rules.is_empty() && !self.enabled_rules.iter().any(|r| r == rule_id) {
            return false;
        }
        if self.disabled_rules.iter().any(|r| r == rule_id) {
            return false;
        }
        let explicitly_enabled = self.enabled_rules.iter().any(|r| r == rule_id);
        if !default_on {
            return explicitly_enabled;
        }
        let picky_level = self.picky || self.level.as_deref() == Some("picky");
        !(picky_tagged && !picky_level && !explicitly_enabled)
    }

    pub fn category_enabled(&self, category_id: &str) -> bool {
        if !self.enabled_categories.is_empty()
            && !self.enabled_categories.iter().any(|c| c == category_id)
        {
            return false;
        }
        if self.disabled_categories.iter().any(|c| c == category_id) {
            return false;
        }
        true
    }

    pub fn has_category_filters(&self) -> bool {
        !self.enabled_categories.is_empty() || !self.disabled_categories.is_empty()
    }

    pub fn is_rule_enabled(
        &self,
        rule_id: &str,
        category_id: &str,
        default_on: bool,
        picky_tagged: bool,
    ) -> bool {
        if !self.enabled_rules.is_empty() && !self.enabled_rules.iter().any(|r| r == rule_id) {
            return false;
        }
        if self.disabled_rules.iter().any(|r| r == rule_id) {
            return false;
        }
        if !self.enabled_categories.is_empty()
            && !self.enabled_categories.iter().any(|c| c == category_id)
        {
            return false;
        }
        if self.disabled_categories.iter().any(|c| c == category_id) {
            return false;
        }
        let explicitly_enabled = self.enabled_rules.iter().any(|r| r == rule_id);
        if !default_on {
            return explicitly_enabled;
        }
        let picky_level = self.picky || self.level.as_deref() == Some("picky");
        !(picky_tagged && !picky_level && !explicitly_enabled)
    }
}

pub trait SentenceTokenizer: Send + Sync {
    fn split(&self, text: &str) -> Vec<SentenceRange>;
}

pub trait WordTokenizer: Send + Sync {
    fn tokenize(&self, text: &str, offset: usize) -> Vec<AnalyzedTokenReadings>;
}

pub trait Tagger: Send + Sync {
    fn tag(&self, tokens: &mut [AnalyzedTokenReadings]);
}

pub trait Disambiguator: Send + Sync {
    fn disambiguate(&self, sentence: &mut AnalyzedSentence);
}

pub trait Chunker: Send + Sync {
    fn add_chunk_tags(&self, sentence: &mut AnalyzedSentence);
}

pub struct Checker {
    sentence_tokenizer: Option<Arc<dyn SentenceTokenizer>>,
    word_tokenizer: Option<Arc<dyn WordTokenizer>>,
    tagger: Option<Arc<dyn Tagger>>,
    disambiguator: Option<Arc<dyn Disambiguator>>,
    chunker: Option<Arc<dyn Chunker>>,
    post_chunker: Option<Arc<dyn Chunker>>,
    rules: Vec<Arc<dyn Rule>>,
    text_level_rules: Vec<Arc<dyn TextLevelRule>>,
    language_filter: Option<Arc<dyn LanguageMatchFilter>>,
    clean_overlapping_matches: bool,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            sentence_tokenizer: None,
            word_tokenizer: None,
            tagger: None,
            disambiguator: None,
            chunker: None,
            post_chunker: None,
            rules: Vec::new(),
            text_level_rules: Vec::new(),
            language_filter: None,
            clean_overlapping_matches: true,
        }
    }

    pub fn with_sentence_tokenizer(mut self, tokenizer: Arc<dyn SentenceTokenizer>) -> Self {
        self.sentence_tokenizer = Some(tokenizer);
        self
    }

    pub fn with_word_tokenizer(mut self, tokenizer: Arc<dyn WordTokenizer>) -> Self {
        self.word_tokenizer = Some(tokenizer);
        self
    }

    pub fn with_tagger(mut self, tagger: Arc<dyn Tagger>) -> Self {
        self.tagger = Some(tagger);
        self
    }

    pub fn with_chunker(mut self, chunker: Arc<dyn Chunker>) -> Self {
        self.chunker = Some(chunker);
        self
    }

    pub fn with_post_chunker(mut self, chunker: Arc<dyn Chunker>) -> Self {
        self.post_chunker = Some(chunker);
        self
    }

    pub fn with_disambiguator(mut self, disambiguator: Arc<dyn Disambiguator>) -> Self {
        self.disambiguator = Some(disambiguator);
        self
    }

    pub fn add_rule(mut self, rule: Arc<dyn Rule>) -> Self {
        self.rules.push(rule);
        self
    }

    pub fn set_rules(mut self, rules: Vec<Arc<dyn Rule>>) -> Self {
        self.rules = rules;
        self
    }

    pub fn add_text_level_rule(mut self, rule: Arc<dyn TextLevelRule>) -> Self {
        self.text_level_rules.push(rule);
        self
    }

    pub fn set_text_level_rules(mut self, rules: Vec<Arc<dyn TextLevelRule>>) -> Self {
        self.text_level_rules = rules;
        self
    }

    pub fn with_language_filter(mut self, filter: Arc<dyn LanguageMatchFilter>) -> Self {
        self.language_filter = Some(filter);
        self
    }

    pub fn with_clean_overlapping_matches(mut self, clean: bool) -> Self {
        self.clean_overlapping_matches = clean;
        self
    }

    pub fn rules(&self) -> &[Arc<dyn Rule>] {
        &self.rules
    }

    pub fn text_level_rules(&self) -> &[Arc<dyn TextLevelRule>] {
        &self.text_level_rules
    }

    pub fn check(&self, request: &CheckRequest) -> CheckResult {
        let text = &request.text;

        let sentence_ranges = if let Some(ref tokenizer) = self.sentence_tokenizer {
            tokenizer.split(text)
        } else {
            vec![SentenceRange {
                start: 0,
                end: text.len(),
            }]
        };

        let mut all_matches: Vec<RuleMatch> = Vec::new();
        let mut analyzed_sentences = Vec::new();

        for range in &sentence_ranges {
            let sentence_text = text[range.start..range.end].to_string();
            let mut sentence = AnalyzedSentence::new(&sentence_text, range.start, range.end);

            if let Some(ref tokenizer) = self.word_tokenizer {
                let mut tokens = tokenizer.tokenize(&sentence_text, range.start);
                let start_token = AnalyzedTokenReadings::new(
                    AnalyzedToken::new("<S>", range.start, range.start)
                        .with_pos_tags(vec!["SENT_START".to_string()]),
                );
                tokens.insert(0, start_token);
                sentence.set_tokens(tokens);
            }

            if let Some(ref tagger) = self.tagger {
                let tokens = &mut sentence.tokens_mut();
                tagger.tag(tokens);
            }

            if let Some(ref chunker) = self.chunker {
                chunker.add_chunk_tags(&mut sentence);
            }

            // Step 2b2: Add SENT_END tag to last non-whitespace token (like Java's setSentEnd())
            {
                let tokens = sentence.tokens_mut();
                for i in (0..tokens.len()).rev() {
                    if !tokens[i].is_whitespace() && !tokens[i].has_pos_tag("SENT_START") {
                        if !tokens[i].has_pos_tag("SENT_END") {
                            tokens[i].add_sent_end();
                        }
                        break;
                    }
                }
            }

            if let Some(ref disambiguator) = self.disambiguator {
                disambiguator.disambiguate(&mut sentence);
            }

            if let Some(ref chunker) = self.post_chunker {
                chunker.add_chunk_tags(&mut sentence);
            }

            let has_cat_filters = request.has_category_filters();
            for rule in &self.rules {
                if has_cat_filters {
                    if !request.is_rule_enabled(
                        rule.id(),
                        rule.category().id().as_str(),
                        rule.is_default_on(),
                        rule.is_picky(),
                    ) {
                        continue;
                    }
                } else if !request.rule_enabled_by_id(
                    rule.id(),
                    rule.is_default_on(),
                    rule.is_picky(),
                ) {
                    continue;
                }
                let matches = rule.match_sentence(&sentence);
                all_matches.extend(matches);
            }

            analyzed_sentences.push(sentence);
        }

        let has_cat_filters = request.has_category_filters();
        for rule in &self.text_level_rules {
            if has_cat_filters {
                if !request.is_rule_enabled(
                    rule.id(),
                    rule.category().id().as_str(),
                    rule.is_default_on(),
                    rule.is_picky(),
                ) {
                    continue;
                }
            } else if !request.rule_enabled_by_id(rule.id(), rule.is_default_on(), rule.is_picky())
            {
                continue;
            }
            let matches = rule.match_text(text, &analyzed_sentences);
            all_matches.extend(matches);
        }

        crate::match_filters::same_rule_group_filter(&mut all_matches);
        if self.clean_overlapping_matches || self.language_filter.is_some() {
            static BASE: crate::match_filters::BaseLanguageMatchFilter =
                crate::match_filters::BaseLanguageMatchFilter;
            let language_filter: &dyn LanguageMatchFilter =
                self.language_filter.as_deref().unwrap_or(&BASE);
            language_filter.filter_rule_matches(&mut all_matches, text);
            if self.clean_overlapping_matches {
                all_matches = crate::match_filters::clean_overlapping_filter(
                    all_matches,
                    text,
                    language_filter,
                );
            }
        }

        let sentence_ranges_json: Vec<Vec<usize>> = sentence_ranges
            .iter()
            .map(|r| vec![r.start, r.end])
            .collect();

        let extended_sentence_ranges: Vec<ExtendedSentenceRange> = sentence_ranges
            .iter()
            .map(|r| {
                let text = &text[r.start..r.end];
                let second_char_is_space = text
                    .char_indices()
                    .nth(1)
                    .is_some_and(|(i, c)| c == ' ' && i == 1);
                let leading = if second_char_is_space {
                    text.len() - text.trim_start().len()
                } else {
                    0
                };
                let trimmed_len = text.trim().len();
                ExtendedSentenceRange {
                    from_offset: r.start + leading,
                    to: r.start + trimmed_len,
                    detected_languages: vec![DetectedLanguageRate {
                        language: request.language.short_code().to_string(),
                        rate: 1.0,
                    }],
                }
            })
            .collect();

        let language_info = LanguageInfo {
            code: request.language.code().to_string(),
            name: request.language.name().to_string(),
            detected: LanguageDetectedInfo {
                name: request.language.name().to_string(),
                code: request.language.code().to_string(),
                confidence: 1.0,
                source: None,
            },
        };

        CheckResult {
            software: SoftwareInfo::default(),
            warnings: Warnings::default(),
            language: language_info,
            matches: all_matches,
            sentence_ranges: sentence_ranges_json,
            extended_sentence_ranges,
        }
    }
}

impl Default for Checker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod rule_enabled_tests {
    use super::*;

    fn request() -> CheckRequest {
        CheckRequest::new("x", Language::English)
    }

    #[test]
    fn default_on_rules_run_without_parameters() {
        assert!(request().is_rule_enabled("SOME_RULE", "GRAMMAR", true, false));
    }

    #[test]
    fn default_off_rules_stay_off_even_at_picky_level() {
        assert!(!request().is_rule_enabled("STYLE_RULE", "STYLE", false, false));
        let mut picky = request();
        picky.picky = true;
        assert!(!picky.is_rule_enabled("STYLE_RULE", "STYLE", false, false));
        let mut leveled = request();
        leveled.level = Some("picky".to_string());
        assert!(!leveled.is_rule_enabled("STYLE_RULE", "STYLE", false, false));
    }

    #[test]
    fn default_off_rules_run_when_explicitly_enabled() {
        let mut req = request();
        req.enabled_rules = vec!["STYLE_RULE".to_string()];
        assert!(req.is_rule_enabled("STYLE_RULE", "STYLE", false, false));
    }

    #[test]
    fn picky_tagged_default_on_rules_need_picky_level() {
        assert!(!request().is_rule_enabled("DASH_RULE", "TYPOGRAPHY", true, true));
        let mut picky = request();
        picky.picky = true;
        assert!(picky.is_rule_enabled("DASH_RULE", "TYPOGRAPHY", true, true));
        let mut leveled = request();
        leveled.level = Some("picky".to_string());
        assert!(leveled.is_rule_enabled("DASH_RULE", "TYPOGRAPHY", true, true));
        let mut enabled = request();
        enabled.enabled_rules = vec!["DASH_RULE".to_string()];
        assert!(enabled.is_rule_enabled("DASH_RULE", "TYPOGRAPHY", true, true));
    }

    #[test]
    fn disabled_rules_suppress_matches() {
        let mut req = request();
        req.disabled_rules = vec!["SOME_RULE".to_string()];
        assert!(!req.is_rule_enabled("SOME_RULE", "GRAMMAR", true, false));
        assert!(req.is_rule_enabled("OTHER_RULE", "GRAMMAR", true, false));
    }

    #[test]
    fn enabled_rules_restrict_to_the_listed_set() {
        let mut req = request();
        req.enabled_rules = vec!["STYLE_RULE".to_string()];
        assert!(!req.is_rule_enabled("SOME_RULE", "GRAMMAR", true, false));
        assert!(req.is_rule_enabled("STYLE_RULE", "STYLE", false, false));
    }

    #[test]
    fn category_filters_apply() {
        let mut req = request();
        req.disabled_categories = vec!["STYLE".to_string()];
        assert!(!req.is_rule_enabled("STYLE_RULE", "STYLE", true, false));
        assert!(req.is_rule_enabled("SOME_RULE", "GRAMMAR", true, false));

        let mut req = request();
        req.enabled_categories = vec!["GRAMMAR".to_string()];
        assert!(!req.is_rule_enabled("STYLE_RULE", "STYLE", true, false));
        assert!(req.is_rule_enabled("SOME_RULE", "GRAMMAR", true, false));
    }
}

#[cfg(test)]
mod pipeline_order_tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    struct RecordingTagger(usize);
    impl Tagger for RecordingTagger {
        fn tag(&self, _tokens: &mut [AnalyzedTokenReadings]) {
            assert_eq!(
                SEQUENCE.fetch_add(1, Ordering::SeqCst),
                self.0,
                "tagger must run at its pipeline position"
            );
        }
    }

    struct RecordingDisambiguator(usize);
    impl Disambiguator for RecordingDisambiguator {
        fn disambiguate(&self, _sentence: &mut AnalyzedSentence) {
            assert_eq!(
                SEQUENCE.fetch_add(1, Ordering::SeqCst),
                self.0,
                "disambiguator must run after the chunker"
            );
        }
    }

    struct RecordingChunker(usize);
    impl Chunker for RecordingChunker {
        fn add_chunk_tags(&self, _sentence: &mut AnalyzedSentence) {
            assert_eq!(
                SEQUENCE.fetch_add(1, Ordering::SeqCst),
                self.0,
                "chunker must run before the disambiguator (JLanguageTool.java:1806-1808)"
            );
        }
    }

    struct NoSplit;
    impl SentenceTokenizer for NoSplit {
        fn split(&self, text: &str) -> Vec<SentenceRange> {
            vec![SentenceRange {
                start: 0,
                end: text.len(),
            }]
        }
    }

    struct NoTokenize;
    impl WordTokenizer for NoTokenize {
        fn tokenize(&self, text: &str, offset: usize) -> Vec<AnalyzedTokenReadings> {
            vec![AnalyzedTokenReadings::new(AnalyzedToken::new(
                text,
                offset,
                offset + text.len(),
            ))]
        }
    }

    #[test]
    fn checker_runs_tag_chunk_then_disambiguate() {
        SEQUENCE.store(0, Ordering::SeqCst);
        let checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(NoSplit))
            .with_word_tokenizer(Arc::new(NoTokenize))
            .with_tagger(Arc::new(RecordingTagger(0)))
            .with_chunker(Arc::new(RecordingChunker(1)))
            .with_disambiguator(Arc::new(RecordingDisambiguator(2)));
        let request = CheckRequest::new("hi", Language::English);
        checker.check(&request);
        assert_eq!(SEQUENCE.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn checker_works_without_chunker() {
        let checker = Checker::new()
            .with_sentence_tokenizer(Arc::new(NoSplit))
            .with_word_tokenizer(Arc::new(NoTokenize));
        let request = CheckRequest::new("hi", Language::English);
        let result = checker.check(&request);
        assert!(result.matches.is_empty());
    }
}
