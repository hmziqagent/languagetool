use crate::Tagger;
use crate::morfologik::MorfologikDictionary;
use og_core::AnalyzedToken;
use og_core::AnalyzedTokenReadings;
use std::path::Path;

type Readings = Vec<(Box<str>, Box<str>)>;
type ReadingMap = std::collections::HashMap<Box<str>, Readings>;

pub struct GermanTagger {
    dictionary: std::sync::Arc<MorfologikDictionary>,
    manual: ReadingMap,
    removals: ReadingMap,
}

pub fn shared_german_dictionary(dir: &Path) -> Option<std::sync::Arc<MorfologikDictionary>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<std::path::PathBuf, std::sync::Arc<MorfologikDictionary>>,
        >,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = dir.join("german.dict");
    let mut guard = cache.lock().unwrap();
    if let Some(dict) = guard.get(&key) {
        return Some(dict.clone());
    }
    let dictionary = std::sync::Arc::new(MorfologikDictionary::load(
        &dir.join("german.dict"),
        &dir.join("german.info"),
    )?);
    guard.insert(key, dictionary.clone());
    Some(dictionary)
}

impl GermanTagger {
    pub fn from_resource_dir(dir: &Path) -> Option<Self> {
        let dictionary = shared_german_dictionary(dir)?;
        Some(Self {
            dictionary,
            manual: std::collections::HashMap::new(),
            removals: std::collections::HashMap::new(),
        })
    }

    pub fn load_manual_data(&mut self, added: &str, removed: &str) {
        for line in added.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            let (Some(form), Some(lemma), Some(tag)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            self.manual
                .entry(form.into())
                .or_default()
                .push((lemma.into(), tag.trim().into()));
        }
        for line in removed.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            let (Some(form), Some(lemma), Some(tag)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            self.removals
                .entry(form.into())
                .or_default()
                .push((lemma.into(), tag.trim().into()));
        }
    }

    pub fn entry_count(&self) -> usize {
        self.dictionary.len()
    }

    fn lookup(&self, word: &str) -> Vec<(Box<str>, Box<str>)> {
        let mut readings: Readings = self
            .manual
            .get(word)
            .map(|v| v.to_vec())
            .unwrap_or_default();
        readings.extend(self.dictionary.lookup(word).iter().cloned());
        if let Some(removals) = self.removals.get(word) {
            readings.retain(|(lemma, tag)| {
                !removals
                    .iter()
                    .any(|(r_lemma, r_tag)| r_lemma == lemma && r_tag == tag)
            });
        }
        readings
    }

    pub fn tag_word(&self, surface: &str, start: usize, end: usize) -> Vec<AnalyzedToken> {
        let normalized: String;
        let word: &str = if surface.len() > 1 && surface.contains('’') {
            normalized = surface.replace('’', "'");
            &normalized
        } else {
            surface
        };
        let lower = word.to_lowercase();
        let is_lowercase = word == lower;

        let mut readings = self.readings_for(word, start, end);
        if !is_lowercase {
            readings.extend(self.readings_for(&lower, start, end));
        }
        readings
    }

    fn readings_for(&self, word: &str, start: usize, end: usize) -> Vec<AnalyzedToken> {
        self.lookup(word)
            .into_iter()
            .map(|(lemma, tag)| {
                AnalyzedToken::new(word, start, end)
                    .with_lemma(lemma.into_string())
                    .with_pos_tags(vec![tag.into_string()])
            })
            .collect()
    }
}

impl Tagger for GermanTagger {
    fn tag(&self, tokens: &[&str]) -> Vec<AnalyzedTokenReadings> {
        let mut results = Vec::with_capacity(tokens.len());
        let mut byte_offset = 0;
        for token_text in tokens {
            let start = byte_offset;
            let end = start + token_text.len();
            let readings = self.tag_word(token_text, start, end);
            let all_tags: Vec<String> = readings
                .iter()
                .flat_map(|r| r.pos_tags().to_vec())
                .collect();
            let lemma = readings
                .first()
                .and_then(|r| r.lemma().map(str::to_string))
                .unwrap_or_else(|| token_text.to_string());
            let primary = AnalyzedToken::new(*token_text, start, end)
                .with_pos_tags(all_tags)
                .with_lemma(lemma);
            results.push(AnalyzedTokenReadings::new(primary).with_readings(readings));
            byte_offset = end;
        }
        results
    }
}

impl og_core::checker::Tagger for GermanTagger {
    fn tag(&self, tokens: &mut [og_core::AnalyzedTokenReadings]) {
        for atr in tokens.iter_mut().skip(1) {
            let primary = atr.token();
            let surface = primary.token().to_string();
            let start = primary.start();
            let end = primary.end();
            let readings = self.tag_word(&surface, start, end);
            let all_tags: Vec<String> = readings
                .iter()
                .flat_map(|r| r.pos_tags().to_vec())
                .collect();
            let lemma = readings
                .first()
                .and_then(|r| r.lemma().map(str::to_string))
                .unwrap_or_else(|| surface.clone());
            let updated = AnalyzedToken::new(surface, start, end)
                .with_pos_tags(all_tags)
                .with_lemma(lemma);
            *atr = AnalyzedTokenReadings::new(updated).with_readings(readings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource_dir() -> Option<std::path::PathBuf> {
        [
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/resource/de",
            "languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/resource/de",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.join("german.dict").exists())
    }

    #[test]
    fn tags_german_nouns_with_case_readings() {
        let Some(dir) = resource_dir() else {
            return;
        };
        let Some(tagger) = GermanTagger::from_resource_dir(&dir) else {
            panic!("tagger must load");
        };
        let result = Tagger::tag(&tagger, &["Haus"]);
        assert!(
            result[0].has_pos_tag("SUB:NOM:SIN:NEU"),
            "{:?}",
            result[0].all_pos_tags()
        );
        assert!(result[0].has_pos_tag("SUB:AKK:SIN:NEU"));
    }

    #[test]
    fn capitalized_sentence_start_falls_back_to_lowercase() {
        let Some(dir) = resource_dir() else {
            return;
        };
        let Some(tagger) = GermanTagger::from_resource_dir(&dir) else {
            panic!("tagger must load");
        };
        let result = Tagger::tag(&tagger, &["Der", "Hund", "schläft"]);
        assert!(
            result[0].has_pos_tag("ART:DEF:NOM:SIN:MAS"),
            "{:?}",
            result[0].all_pos_tags()
        );
        assert!(
            result[1].has_pos_tag("SUB:NOM:SIN:MAS"),
            "{:?}",
            result[1].all_pos_tags()
        );
        assert!(
            result[2].has_pos_tag("VER:3:SIN:PRÄ:NON"),
            "{:?}",
            result[2].all_pos_tags()
        );
    }
}
