use og_core::checker::Tagger as CoreTagger;
use og_core::{AnalyzedToken, AnalyzedTokenReadings};
use og_tagger::Tagger;
use std::path::Path;
use std::sync::{Arc, OnceLock};

pub struct FrenchTagger {
    inner: Arc<og_tagger::GenericTagger>,
}

fn apostrophe_replacement(surface: &str) -> (Option<String>, Option<&'static str>) {
    if surface.chars().count() <= 1 {
        return (None, None);
    }
    if surface.contains('\u{2019}') {
        (
            Some(surface.replace('\u{2019}', "'")),
            Some("containsTypographicApostrophe"),
        )
    } else if surface.contains('\'') {
        (None, Some("containsTypewriterApostrophe"))
    } else {
        (None, None)
    }
}

fn normalized_atr(atr: AnalyzedTokenReadings) -> AnalyzedTokenReadings {
    let primary = atr.token();
    let surface = primary.token();
    let (replacement, chunk) = apostrophe_replacement(surface);
    let Some(replacement) = replacement else {
        if let Some(chunk) = chunk
            && atr.chunks().is_empty()
        {
            let mut with_chunk = atr;
            with_chunk.set_chunks(vec![chunk.to_string()]);
            return with_chunk;
        }
        return atr;
    };
    let lemma = atr
        .readings()
        .first()
        .and_then(|r| r.lemma())
        .map(str::to_string)
        .unwrap_or_else(|| replacement.clone());
    let pos_tags = primary.pos_tags().to_vec();
    let updated = AnalyzedToken::new(replacement, primary.start(), primary.end())
        .with_pos_tags(pos_tags)
        .with_lemma(lemma);
    let mut next = AnalyzedTokenReadings::new(updated).with_readings(atr.into_readings());
    if let Some(chunk) = chunk {
        next.set_chunks(vec![chunk.to_string()]);
    }
    next
}

impl FrenchTagger {
    pub fn from_resource_dir(resource_dir: &Path) -> Self {
        Self {
            inner: Arc::new(og_tagger::GenericTagger::french(resource_dir)),
        }
    }

    pub fn shared_arc() -> Option<Arc<FrenchTagger>> {
        static CACHE: OnceLock<Option<Arc<FrenchTagger>>> = OnceLock::new();
        CACHE
            .get_or_init(|| {
                crate::engine::find_lt_resource_dir().map(|root| {
                    Arc::new(FrenchTagger::from_resource_dir(
                        &root.join("fr/src/main/resources/org/languagetool/resource/fr"),
                    ))
                })
            })
            .clone()
    }

    pub fn inner_is_word_tagged(&self, word: &str) -> bool {
        if word.chars().count() > 1 && word.contains('\u{2019}') {
            return self.inner.is_word_tagged(&word.replace('\u{2019}', "'"));
        }
        self.inner.is_word_tagged(word)
    }
}

impl Tagger for FrenchTagger {
    fn tag(&self, tokens: &[&str]) -> Vec<AnalyzedTokenReadings> {
        og_tagger::Tagger::tag(self.inner.as_ref(), tokens)
            .into_iter()
            .map(normalized_atr)
            .collect()
    }
}

impl CoreTagger for FrenchTagger {
    fn tag(&self, tokens: &mut [AnalyzedTokenReadings]) {
        og_core::checker::Tagger::tag(self.inner.as_ref(), tokens);
        for atr in tokens.iter_mut().skip(1) {
            let primary = atr.token();
            let surface = primary.token();
            let (replacement, chunk) = apostrophe_replacement(surface);
            let Some(replacement) = replacement else {
                if let Some(chunk) = chunk
                    && atr.chunks().is_empty()
                {
                    atr.set_chunks(vec![chunk.to_string()]);
                }
                continue;
            };
            let lemma = atr
                .readings()
                .first()
                .and_then(|r| r.lemma())
                .map(str::to_string)
                .unwrap_or_else(|| replacement.clone());
            let pos_tags = primary.pos_tags().to_vec();
            let updated = AnalyzedToken::new(replacement, primary.start(), primary.end())
                .with_pos_tags(pos_tags)
                .with_lemma(lemma);
            let readings = atr.readings().to_vec();
            let mut next = AnalyzedTokenReadings::new(updated).with_readings(readings);
            if let Some(chunk) = chunk {
                next.set_chunks(vec![chunk.to_string()]);
            }
            *atr = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagger() -> FrenchTagger {
        let resource_dir = crate::fr::test_support::fr_resource_dir().expect("fr resource dir");
        FrenchTagger::from_resource_dir(&resource_dir)
    }

    fn surfaces(tokens: &[&str]) -> Vec<String> {
        og_tagger::Tagger::tag(&tagger(), tokens)
            .iter()
            .map(|atr| atr.token().token().to_string())
            .collect()
    }

    fn joined(tokens: &[&str]) -> String {
        og_tagger::Tagger::tag(&tagger(), tokens)
            .iter()
            .map(|atr| {
                let mut readings: Vec<String> = atr
                    .readings()
                    .iter()
                    .map(|r| {
                        format!(
                            "{}/[{}]{}",
                            atr.token().token(),
                            r.lemma().unwrap_or("null"),
                            if r.pos_tags().is_empty() {
                                "null".to_string()
                            } else {
                                r.pos_tags().join("|")
                            }
                        )
                    })
                    .collect();
                readings.sort_unstable();
                readings.join("|")
            })
            .collect::<Vec<_>>()
            .join(" -- ")
    }

    #[test]
    fn typographic_apostrophe_surfaces_become_typewriter() {
        assert_eq!(surfaces(&["d’", "aujourd’hui"]), vec!["d'", "aujourd'hui"]);
        assert_eq!(surfaces(&["Fontaine-l’Évêque"]), vec!["Fontaine-l'Évêque"]);
        assert_eq!(surfaces(&["entr’ouvrions"]), vec!["entr'ouvrions"]);
        assert_eq!(surfaces(&["Qu’", "est"]), vec!["Qu'", "est"]);
    }

    #[test]
    fn java_frenchtagger_test_apostrophe_pins() {
        assert_eq!(
            joined(&["d’", "aujourd’hui"]),
            "d'/[de]D e sp|d'/[de]P -- aujourd'hui/[aujourd'hui]A"
        );
        assert_eq!(
            joined(&["d'", "aujourd’hui"]),
            "d'/[de]D e sp|d'/[de]P -- aujourd'hui/[aujourd'hui]A"
        );
        assert_eq!(
            joined(&["d’", "aujourd'hui"]),
            "d'/[de]D e sp|d'/[de]P -- aujourd'hui/[aujourd'hui]A"
        );
        assert_eq!(
            joined(&["Fontaine-l’Évêque"]),
            "Fontaine-l'Évêque/[Fontaine-l'Évêque]Z e sp"
        );
        assert_eq!(
            joined(&["entr’ouvrions"]),
            "entr'ouvrions/[entr'ouvrir]V ind impa 1 p|entr'ouvrions/[entr'ouvrir]V sub pres 1 p"
        );
    }

    #[test]
    fn apostrophe_chunks_follow_java_precedence() {
        let atr = &og_tagger::Tagger::tag(&tagger(), &["aujourd’hui"])[0];
        assert_eq!(atr.chunks(), &["containsTypographicApostrophe".to_string()]);
        let atr = &og_tagger::Tagger::tag(&tagger(), &["aujourd'hui"])[0];
        assert_eq!(atr.chunks(), &["containsTypewriterApostrophe".to_string()]);
        let atr = &og_tagger::Tagger::tag(&tagger(), &["’"])[0];
        assert!(atr.chunks().is_empty());
    }
}
