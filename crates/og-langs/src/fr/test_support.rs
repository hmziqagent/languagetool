use og_core::checker::Disambiguator;
use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};
use og_tokenizer::WordTokenizer as _;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

pub(crate) fn fr_resource_dir() -> Option<PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("fr/src/main/resources/org/languagetool/resource/fr"))
}

pub(crate) fn fr_rules_dir() -> Option<PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("fr/src/main/resources/org/languagetool/rules/fr"))
}

pub(crate) fn apostrophe_chunk(surface: &str) -> Option<&'static str> {
    if surface.chars().count() <= 1 {
        return None;
    }
    if surface.contains('\u{2019}') {
        Some("containsTypographicApostrophe")
    } else if surface.contains('\'') {
        Some("containsTypewriterApostrophe")
    } else {
        None
    }
}

pub(crate) struct FrenchPipeline {
    tokenizer: og_tokenizer::FrenchWordTokenizer,
    tagger: Arc<og_tagger::GenericTagger>,
    disambiguator: Option<Arc<dyn Disambiguator>>,
    splitter: Option<og_tokenizer::srx::SrxSentenceSplitter>,
}

impl FrenchPipeline {
    pub fn shared() -> &'static FrenchPipeline {
        static PIPE: OnceLock<FrenchPipeline> = OnceLock::new();
        PIPE.get_or_init(|| {
            let resource_dir = fr_resource_dir().expect("fr resource dir with french.dict");
            let tagger = Arc::new(og_tagger::GenericTagger::french(&resource_dir));
            let tokenizer = og_tokenizer::FrenchWordTokenizer::new(Arc::new({
                let tagger = tagger.clone();
                move |w: &str| tagger.is_word_tagged(w)
            }));
            let disambiguator = std::fs::read_to_string(resource_dir.join("disambiguation.xml"))
                .ok()
                .and_then(|xml| og_tagger::XmlDisambiguator::from_xml(&xml).ok())
                .and_then(|d| {
                    crate::engine::find_lt_core_resource_dir()
                        .map(|core| og_tagger::hybrid_disambiguator("fr", &resource_dir, &core, d))
                });
            FrenchPipeline {
                tokenizer,
                tagger,
                disambiguator,
                splitter: og_tokenizer::srx::srx_splitter("fr"),
            }
        })
    }

    pub fn tagger_ref(&self) -> &og_tagger::GenericTagger {
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
        tokens.extend(
            self.tokenizer.tokenize(sentence_text).into_iter().map(|t| {
                AnalyzedTokenReadings::new(AnalyzedToken::new(t.text(), t.start(), t.end()))
            }),
        );
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
                let mut chunks = atr.chunks().to_vec();
                if chunks.is_empty()
                    && let Some(chunk) = apostrophe_chunk(tokens_mut[token_idx].token().token())
                {
                    chunks.push(chunk.to_string());
                }
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
            } else if token_idx < tokens_mut.len() {
                let chunk = apostrophe_chunk(tokens_mut[token_idx].token().token());
                if let Some(chunk) = chunk {
                    tokens_mut[token_idx].set_chunks(vec![chunk.to_string()]);
                }
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

pub(crate) fn analyze_french(text: &str) -> Vec<AnalyzedSentence> {
    FrenchPipeline::shared().analyze_text(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readings_join(text: &str) -> String {
        let sentences = analyze_french(text);
        let parts: Vec<String> = sentences
            .iter()
            .flat_map(|s| s.tokens().iter())
            .filter(|t| !t.is_whitespace() && t.token().token() != "<S>")
            .map(|t| {
                let readings: Vec<String> = t
                    .readings()
                    .iter()
                    .filter(|r| !r.pos_tags().is_empty() && r.pos_tags()[0] != "SENT_END")
                    .map(|r| {
                        format!(
                            "{}/[{}]{}",
                            r.lemma().unwrap_or("null"),
                            r.pos_tags().join("|"),
                            ""
                        )
                    })
                    .collect();
                format!("{}={}", t.token().token(), readings.join("|"))
            })
            .collect();
        parts.join(" ; ")
    }

    fn sorted_readings(text: &str) -> String {
        readings_join(text)
            .split(" ; ")
            .map(|part| match part.split_once('=') {
                Some((surface, readings)) => {
                    let mut items: Vec<&str> = readings.split('|').collect();
                    items.sort_unstable();
                    format!("{}={}", surface, items.join("|"))
                }
                None => part.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ; ")
    }

    #[test]
    fn java_frenchtagger_test_pins() {
        assert_eq!(
            sorted_readings("passagère"),
            "passagère=passager/[J f s]|passager/[N f s]"
        );
        assert_eq!(readings_join("blablabla"), "blablabla=blablabla/[N m s]");
        assert_eq!(
            readings_join("entr'ouvrions"),
            "entr'ouvrions=entr'ouvrir/[V sub pres 1 p]|entr'ouvrir/[V ind impa 1 p]"
        );
        assert_eq!(
            readings_join("auto-mutilées"),
            "auto-mutilées=auto-mutiler/[V ppa f p]"
        );
        assert_eq!(
            readings_join("Grand-Chambre"),
            "Grand-Chambre=grand-chambre/[N f s]"
        );
        assert_eq!(readings_join("soeur"), "soeur=sœur/[N f s]");
    }

    #[test]
    fn typewriter_apostrophe_chunk_tags_follow_frenchtagger() {
        let sentences = analyze_french("Ils ont beaucoup d'enfants. Va-t'en d’ici.");
        let chunks: Vec<Vec<String>> = sentences
            .iter()
            .flat_map(|s| s.tokens().iter())
            .map(|t| t.chunks().to_vec())
            .collect();
        let flat: Vec<String> = chunks.into_iter().flatten().collect();
        assert!(flat.contains(&"containsTypewriterApostrophe".to_string()));
        assert!(flat.contains(&"containsTypographicApostrophe".to_string()));
    }

    #[test]
    fn single_apostrophe_token_carries_no_chunk() {
        assert_eq!(apostrophe_chunk("'"), None);
        assert_eq!(
            apostrophe_chunk("d’aujourd’hui"),
            Some("containsTypographicApostrophe")
        );
        assert_eq!(
            apostrophe_chunk("aujourd'hui"),
            Some("containsTypewriterApostrophe")
        );
    }

    #[test]
    fn srx_french_sentence_splitting() {
        let ranges = FrenchPipeline::shared().sentence_ranges("M. Dupont arrive. Il part.");
        assert_eq!(ranges.len(), 2, "{ranges:?}");
    }
}
