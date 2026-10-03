pub mod question_mark;
pub mod repeated_words;
pub mod replace_verbs;
pub mod simple_replace;
pub mod tagger;
pub mod unpaired_brackets;
pub mod word_repeat;
pub mod word_repeat_beginning;
pub mod word_tokenizer;

use og_core::rule::{Rule, TextLevelRule};
use std::path::PathBuf;
use std::sync::Arc;

pub fn es_resource_dir() -> Option<PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("es/src/main/resources/org/languagetool/resource/es"))
}

pub fn es_rules_dir() -> Option<PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("es/src/main/resources/org/languagetool/rules/es"))
}

pub struct SpanishWrongWordInContextRule {
    inner: crate::en::wrong_word_context::WrongWordInContextRule,
}

impl SpanishWrongWordInContextRule {
    pub fn new() -> Self {
        Self {
            inner: crate::en::wrong_word_context::WrongWordInContextRule::for_language(
                "SPANISH_WRONG_WORD_IN_CONTEXT",
                "Confusión según el contexto: $match",
                include_str!(
                    "../../../../languagetool/languagetool-language-modules/es/src/main/resources/org/languagetool/rules/es/wrongWordInContext.txt"
                ),
                "¿Quería decir <suggestion>$SUGGESTION</suggestion> en vez de \"$WRONGWORD\"?",
                "¿Quería decir <suggestion>$SUGGESTION</suggestion> ($EXPLANATION_SUGGESTION) en vez de \"$WRONGWORD\" ($EXPLANATION_WRONGWORD)?",
            ),
        }
    }
}

impl Default for SpanishWrongWordInContextRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for SpanishWrongWordInContextRule {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn is_default_on(&self) -> bool {
        self.inner.is_default_on()
    }

    fn category(&self) -> og_core::Category {
        og_core::Category::new("CONFUSED_WORDS", "Confusiones")
    }

    fn issue_type(&self) -> og_core::IssueType {
        og_core::IssueType::Grammar
    }

    fn match_sentence(&self, sentence: &og_core::AnalyzedSentence) -> Vec<og_core::RuleMatch> {
        self.inner.match_sentence(sentence)
    }
}

pub struct SpanishCompoundRule {
    inner: crate::en::compound::CompoundRule,
}

impl SpanishCompoundRule {
    pub fn new() -> Self {
        Self {
            inner: crate::en::compound::CompoundRule::for_language(
                "ES_COMPOUNDS",
                "Palabras compuestas con guion: $match",
                "Se escribe con un guion.",
                "Se escribe junto sin espacio ni guion.",
                "Se escribe junto o con un guion.",
                include_str!(
                    "../../../../languagetool/languagetool-language-modules/es/src/main/resources/org/languagetool/resource/es/compounds.txt"
                ),
            ),
        }
    }
}

impl Default for SpanishCompoundRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for SpanishCompoundRule {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn is_default_on(&self) -> bool {
        self.inner.is_default_on()
    }

    fn category(&self) -> og_core::Category {
        og_core::Category::new("COMPOUNDING", "Palabras compuestas")
    }

    fn issue_type(&self) -> og_core::IssueType {
        og_core::IssueType::Misspelling
    }

    fn match_sentence(&self, sentence: &og_core::AnalyzedSentence) -> Vec<og_core::RuleMatch> {
        self.inner.match_sentence(sentence)
    }
}

pub fn native_rules() -> (Vec<Arc<dyn Rule>>, Vec<Arc<dyn TextLevelRule>>) {
    let sentence_rules: Vec<Arc<dyn Rule>> = vec![
        Arc::new(simple_replace::SimpleReplaceRule::spanish()),
        Arc::new(SpanishWrongWordInContextRule::new()),
        Arc::new(SpanishCompoundRule::new()),
        Arc::new(simple_replace::SimpleReplaceRule::wikipedia()),
        Arc::new(replace_verbs::SimpleReplaceVerbsRule::new()),
        Arc::new(word_repeat::SpanishWordRepeatRule::new()),
        Arc::new(og_rules::native_rules::UppercaseSentenceStartRule::new()),
    ];
    let text_level_rules: Vec<Arc<dyn TextLevelRule>> = vec![
        Arc::new(question_mark::QuestionMarkRule::new()),
        Arc::new(unpaired_brackets::SpanishUnpairedBracketsRule::new()),
        Arc::new(word_repeat_beginning::SpanishWordRepeatBeginningRule::new()),
        Arc::new(repeated_words::SpanishRepeatedWordsRule::new()),
        Arc::new(og_rules::text_level_rules::LongSentenceRule::new(60)),
        Arc::new(og_rules::text_level_rules::LongParagraphRule::new(220)),
    ];
    (sentence_rules, text_level_rules)
}

pub fn spanish() -> crate::engine::LanguageEngine {
    let mut engine =
        crate::engine::LanguageEngine::for_code("es").expect("es is a registered generic language");
    let (sentence_rules, text_level_rules) = native_rules();
    engine.add_rules(sentence_rules);
    engine.add_text_level_rules(text_level_rules);
    engine
}

#[cfg(test)]
pub(crate) mod example_suite;

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use og_core::checker::Disambiguator;
    use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};
    use og_tokenizer::WordTokenizer as _;
    use std::sync::{Arc, OnceLock};

    pub struct SpanishPipeline {
        tokenizer: word_tokenizer::SpanishWordTokenizer,
        tagger: Arc<tagger::SpanishTagger>,
        disambiguator: Option<Arc<dyn Disambiguator>>,
        splitter: Option<og_tokenizer::srx::SrxSentenceSplitter>,
    }

    impl SpanishPipeline {
        pub fn shared() -> &'static SpanishPipeline {
            static PIPE: OnceLock<SpanishPipeline> = OnceLock::new();
            PIPE.get_or_init(|| {
                let resource_dir =
                    es_resource_dir().expect("es resource dir with spanish.dict present");
                let tagger = Arc::new(tagger::SpanishTagger::from_resource_dir(&resource_dir));
                let disambiguator =
                    std::fs::read_to_string(resource_dir.join("disambiguation.xml"))
                        .ok()
                        .and_then(|xml| og_tagger::XmlDisambiguator::from_xml(&xml).ok())
                        .and_then(|d| {
                            crate::engine::find_lt_core_resource_dir().map(|core| {
                                og_tagger::hybrid_disambiguator("es", &resource_dir, &core, d)
                            })
                        });
                SpanishPipeline {
                    tokenizer: word_tokenizer::SpanishWordTokenizer::new(),
                    tagger,
                    disambiguator,
                    splitter: og_tokenizer::srx::srx_splitter("es"),
                }
            })
        }

        pub fn tagger_ref(&self) -> &tagger::SpanishTagger {
            &self.tagger
        }

        pub fn sentence_ranges(&self, text: &str) -> Vec<(usize, usize)> {
            use og_tokenizer::SentenceTokenizer as _;
            match &self.splitter {
                Some(srx) => srx
                    .split(text)
                    .iter()
                    .map(|s| (s.start(), s.end()))
                    .collect(),
                None => vec![(0, text.len())],
            }
        }

        pub fn analyze_range(&self, text: &str, start: usize, end: usize) -> AnalyzedSentence {
            let sentence_text = &text[start..end];
            let mut sentence = AnalyzedSentence::new(sentence_text, 0, sentence_text.len());
            let mut tokens = Vec::new();
            let start_token = AnalyzedTokenReadings::new(
                AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
            );
            tokens.push(start_token);
            tokens.extend(self.tokenizer.tokenize(sentence_text).into_iter().map(|t| {
                AnalyzedTokenReadings::new(AnalyzedToken::new(t.text(), t.start(), t.end()))
            }));
            sentence.set_tokens(tokens);
            let token_texts: Vec<String> = sentence
                .tokens()
                .iter()
                .skip(1)
                .map(|t| t.token().token().to_string())
                .collect();
            let refs: Vec<&str> = token_texts.iter().map(|s| s.as_str()).collect();
            let tagged = og_tagger::Tagger::tag(self.tagger.as_ref(), &refs);
            let tokens_mut = sentence.tokens_mut();
            for (i, atr) in tagged.into_iter().enumerate() {
                let token_idx = i + 1;
                if token_idx < tokens_mut.len() && !atr.readings().is_empty() {
                    let chunks = atr.chunks().to_vec();
                    let readings = atr.into_readings();
                    let all_tags: Vec<String> = readings
                        .iter()
                        .flat_map(|r| r.pos_tags().to_vec())
                        .collect();
                    let lemma = readings[0]
                        .lemma()
                        .unwrap_or(tokens_mut[token_idx].token().token())
                        .to_string();
                    let mut merged = AnalyzedTokenReadings::new(
                        AnalyzedToken::new(
                            tokens_mut[token_idx].token().token(),
                            tokens_mut[token_idx].token().start(),
                            tokens_mut[token_idx].token().end(),
                        )
                        .with_pos_tags(all_tags)
                        .with_lemma(lemma),
                    )
                    .with_readings(readings);
                    if !chunks.is_empty() {
                        merged.set_chunks(chunks);
                    }
                    tokens_mut[token_idx] = merged;
                }
            }
            let tokens = sentence.tokens_mut();
            for token in tokens.iter_mut().rev() {
                if !token.is_whitespace()
                    && !token.token().pos_tags().contains(&"SENT_START".to_string())
                {
                    if !token.has_pos_tag("SENT_END") {
                        token.add_sent_end();
                    }
                    break;
                }
            }
            if let Some(disambiguator) = &self.disambiguator {
                disambiguator.disambiguate(&mut sentence);
            }
            sentence
        }

        pub fn analyze_text(&self, text: &str) -> Vec<AnalyzedSentence> {
            self.sentence_ranges(text)
                .into_iter()
                .map(|(s, e)| self.analyze_range(text, s, e))
                .collect()
        }

        pub fn analyze_offset_text(&self, text: &str) -> Vec<(AnalyzedSentence, usize)> {
            self.sentence_ranges(text)
                .into_iter()
                .map(|(s, e)| (self.analyze_range(text, s, e), s))
                .collect()
        }
    }

    pub fn analyze_spanish(text: &str) -> Vec<AnalyzedSentence> {
        SpanishPipeline::shared().analyze_text(text)
    }

    pub fn analyze_spanish_sentences(text: &str) -> Vec<AnalyzedSentence> {
        SpanishPipeline::shared().analyze_text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::SpanishPipeline;

    #[test]
    fn srx_rules_resolve_for_spanish() {
        let splitter = og_tokenizer::srx::srx_splitter("es");
        assert!(splitter.is_some(), "es SRX rules must resolve");
        use og_tokenizer::SentenceTokenizer as _;
        let splitter = splitter.unwrap();
        let sentences = splitter.split("Veni. Adiós.");
        assert_eq!(sentences.len(), 2);
        let sentences = splitter.split("1. El título sigue aquí.");
        assert_eq!(
            sentences.len(),
            1,
            "es 'Punto primero' rule keeps enumerations whole"
        );
    }

    #[test]
    fn tagger_pins_java_spanish_tagger_test() {
        let pipeline = SpanishPipeline::shared();
        let cases: &[(&str, &str)] = &[
            ("Soy", "ser VSIP1S0"),
            ("CASA", "casa NCFS000"),
            ("superincómodas", "superincómodo AQ0FP0"),
            ("autoeducan", "autoeducar VMIP3P0"),
            ("autorretratarán", "autorretratar VMIF3P0"),
            ("económico-sociales", "económico-social AQ0CP0"),
        ];
        for (word, expected) in cases {
            let tagged = og_tagger::Tagger::tag(pipeline.tagger_ref(), &[word]);
            let actual = tagged
                .first()
                .and_then(|atr| atr.readings().first())
                .map(|r| {
                    format!(
                        "{} {}",
                        r.lemma().unwrap_or("null"),
                        r.pos_tags().first().map(String::as_str).unwrap_or("null")
                    )
                })
                .unwrap_or_else(|| "null null".to_string());
            assert_eq!(&actual, expected, "tagging {word}");
        }
    }

    #[test]
    fn tagger_pins_java_untagged_and_case_behavior() {
        let pipeline = SpanishPipeline::shared();
        let tagged = og_tagger::Tagger::tag(pipeline.tagger_ref(), &["blablabla"]);
        assert!(
            tagged[0].readings().is_empty()
                || tagged[0].readings().iter().all(|r| r.pos_tags().is_empty()),
            "blablabla must stay untagged"
        );
        let tagged = og_tagger::Tagger::tag(pipeline.tagger_ref(), &["autoralentizan"]);
        assert!(
            tagged[0].readings().is_empty(),
            "autoralentizan has no valid prefix analysis (Java: [null]null)"
        );
        let tagged = og_tagger::Tagger::tag(pipeline.tagger_ref(), &["CaSa"]);
        assert!(
            tagged[0].readings().is_empty(),
            "mixed-case words get no lowercase fallback (SpanishTagger.tag isMixedCase guard)"
        );
    }
}
