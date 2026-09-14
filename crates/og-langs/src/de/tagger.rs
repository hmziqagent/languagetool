use og_core::checker::Tagger;
use og_core::{AnalyzedToken, AnalyzedTokenReadings};
use std::collections::HashMap;

/// One analysis of a German full form: (lemma, Morphy POS tag).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GermanReading {
    pub lemma: String,
    pub pos: String,
}

/// A dictionary POS tagger for German, loaded from the upstream
/// `resource/de/added.txt` full-form list (format: `fullform\tbaseform\tpostags`,
/// tab separated, POS in the STTS/Morphy tagset — see `tagset.txt`).
///
/// This is a reduced stand-in for Java's `GermanTagger` (which reads the
/// Morfologik `german.dict` binary plus `added.txt`): it does no
/// unknown-word morphology synthesis (no ge-/ver- prefix stripping or
/// compound splitting like `GermanTagger`'s `postagFixes`/synthesizer
/// fallback), so unknown words keep their untagged token reading.
pub struct GermanTagger {
    /// full form (lowercase key) → readings
    entries: HashMap<String, Vec<GermanReading>>,
}

impl GermanTagger {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Load full-form entries from the `added.txt` format
    /// (`fullform\tbaseform\tpostags`, `#` comments, blank lines skipped).
    pub fn load_added(&mut self, data: &str) -> usize {
        let mut count = 0;
        for line in data.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('\t');
            let (Some(fullform), Some(baseform), Some(postags)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let fullform = fullform.trim();
            if fullform.is_empty() {
                continue;
            }
            for pos in postags.split_whitespace() {
                self.entries
                    .entry(fullform.to_lowercase())
                    .or_default()
                    .push(GermanReading {
                        lemma: baseform.trim().to_string(),
                        pos: pos.to_string(),
                    });
                count += 1;
            }
        }
        count
    }

    /// Build a tagger from the upstream `added.txt` resource. Returns a tagger
    /// with zero entries when the resource root is not configured.
    pub fn from_resources() -> Self {
        let mut tagger = Self::new();
        if let Some(data) = og_spell::read_lt_resource(
            "de/src/main/resources/org/languagetool/resource/de/added.txt",
        ) {
            let count = tagger.load_added(&data);
            eprintln!("Loaded {} German full-form readings", count);
        }
        tagger
    }

    /// Look up the readings for a surface form (exact case, then
    /// all-lowercase, mirroring Java's dictionary lookup order).
    pub fn readings_for(&self, word: &str) -> &[GermanReading] {
        self.entries
            .get(word)
            .or_else(|| self.entries.get(&word.to_lowercase()))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

impl Default for GermanTagger {
    fn default() -> Self {
        Self::new()
    }
}

impl Tagger for GermanTagger {
    fn tag(&self, tokens: &mut [AnalyzedTokenReadings]) {
        // Position 0 is the SENT_START token — leave it untouched.
        for atr in tokens.iter_mut().skip(1) {
            if atr.is_whitespace() {
                continue;
            }
            let text = atr.token().token();
            let readings = self.readings_for(text);
            if readings.is_empty() {
                continue;
            }
            let start = atr.token().start();
            let end = atr.token().end();
            let new_readings: Vec<AnalyzedToken> = readings
                .iter()
                .map(|r| {
                    AnalyzedToken::new(text, start, end)
                        .with_pos_tags(vec![r.pos.clone()])
                        .with_lemma(r.lemma.clone())
                })
                .collect();
            let all_tags: Vec<String> = new_readings
                .iter()
                .flat_map(|r| r.pos_tags().to_vec())
                .collect();
            let primary = AnalyzedToken::new(text, start, end)
                .with_pos_tags(all_tags)
                .with_lemma(readings[0].lemma.clone());
            *atr = AnalyzedTokenReadings::new(primary).with_readings(new_readings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_word(tagger: &GermanTagger, word: &str) -> AnalyzedTokenReadings {
        // Position 0 is SENT_START (skipped by tag()) — put the word second.
        let mut tokens = vec![
            AnalyzedTokenReadings::new(AnalyzedToken::new("<S>", 0, 3)),
            AnalyzedTokenReadings::new(AnalyzedToken::new(word, 4, 4 + word.len())),
        ];
        tagger.tag(&mut tokens);
        tokens.remove(1)
    }

    #[test]
    fn loads_added_format() {
        let mut tagger = GermanTagger::new();
        let n = tagger.load_added("# comment\nHaus\tHaus\tNN\tNN\nhäuser\tHaus\tNN PLU\n");
        assert_eq!(n, 3);
        assert_eq!(tagger.readings_for("Haus").len(), 1); // NN from "NN\tNN"
        assert_eq!(tagger.readings_for("häuser").len(), 2); // NN + PLU
    }

    #[test]
    fn tags_known_word_with_stts_tag_and_lemma() {
        let mut tagger = GermanTagger::new();
        tagger.load_added("Haus\tHaus\tNN\n");
        let atr = tag_word(&tagger, "Haus");
        assert!(atr.has_pos_tag("NN"));
        assert!(atr.readings()[0].lemma().is_some_and(|l| l == "Haus"));
    }

    #[test]
    fn unknown_word_gets_no_readings() {
        let mut tagger = GermanTagger::new();
        tagger.load_added("Haus\tHaus\tNN\n");
        let atr = tag_word(&tagger, "Xyzzyq");
        // No dictionary entry -> the identity reading is left untouched
        // (no POS tags, no lemma).
        assert!(atr.readings().iter().all(|r| r.pos_tags().is_empty()));
        assert!(atr.readings().iter().all(|r| r.lemma().is_none()));
    }

    #[test]
    fn data_backed_tags_real_german_entry() {
        let Some(data) = og_spell::read_lt_resource(
            "de/src/main/resources/org/languagetool/resource/de/added.txt",
        ) else {
            eprintln!("Skipping: LT resource dir not found");
            return;
        };
        let mut tagger = GermanTagger::new();
        let count = tagger.load_added(&data);
        assert!(count > 40_000, "expected large full-form list, got {count}");
        // added.txt supplements the binary german.dict; nouns live in the
        // binary dict, so verify on an entry actually present in added.txt:
        // "vorm" carries PRP:LOK+TMP:DAT.
        let atr = tag_word(&tagger, "vorm");
        assert!(
            atr.has_pos_tag("PRP:LOK+TMP:DAT"),
            "vorm should get its added.txt reading, got {:?}",
            atr.readings()
                .iter()
                .map(|r| r.pos_tags())
                .collect::<Vec<_>>()
        );
        // The main dictionary is NOT loaded: nouns stay untagged (documented
        // divergence — binary Morfologik german.dict is not parsed).
        let haus = tag_word(&tagger, "Haus");
        assert!(haus.readings().iter().all(|r| r.pos_tags().is_empty()));
    }
}
