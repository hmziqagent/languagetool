use crate::Tagger;
use og_core::AnalyzedToken;
use og_core::AnalyzedTokenReadings;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, PartialEq, Eq)]
struct TaggedWord {
    lemma: Box<str>,
    tag: u32,
}

pub struct EnglishTagger {
    tags: Vec<Box<str>>,
    dict: HashMap<Box<str>, Box<[TaggedWord]>>,
    fsa: HashMap<Box<str>, Vec<TaggedWord>>,
    manual: HashMap<Box<str>, Vec<TaggedWord>>,
    removals: HashMap<Box<str>, Vec<(Box<str>, u32)>>,
}

fn is_all_uppercase(s: &str) -> bool {
    !s.chars().any(|c| c.is_lowercase())
}

fn is_not_all_lowercase(s: &str) -> bool {
    s.chars().any(|c| c.is_uppercase())
}

fn is_capitalized_word(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => chars.all(|c| !c.is_uppercase()),
        _ => false,
    }
}

fn is_mixed_case(s: &str) -> bool {
    !is_all_uppercase(s) && !is_capitalized_word(s) && is_not_all_lowercase(s)
}

fn uppercase_first_char(s: &str) -> String {
    let mut out: String = String::with_capacity(s.len());
    let mut chars = s.chars();
    if let Some(first) = chars.next() {
        out.extend(first.to_uppercase());
        out.push_str(chars.as_str());
    }
    out
}

fn split_manual_line<'a>(line: &'a str, separator: &str) -> Option<(&'a str, &'a str, &'a str)> {
    let no_comment = line.split('#').next().unwrap_or("").trim();
    if no_comment.is_empty() {
        return None;
    }
    let mut parts = no_comment.splitn(3, separator);
    let form = parts.next()?.trim();
    let lemma = parts.next()?.trim();
    let tag = parts.next()?.trim();
    if form.is_empty() || lemma.is_empty() || tag.is_empty() {
        return None;
    }
    Some((form, lemma, tag))
}

impl EnglishTagger {
    pub fn new() -> Self {
        Self {
            tags: Vec::new(),
            dict: HashMap::new(),
            fsa: HashMap::new(),
            manual: HashMap::new(),
            removals: HashMap::new(),
        }
    }

    pub fn from_resource_dir(dir: &Path) -> Self {
        let mut tagger = Self::new();
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        if let Some(data) = read("dict_decoded.txt") {
            tagger.load_fsa_data(&data);
        }
        let added = ["added.txt", "added_custom.txt"]
            .iter()
            .filter_map(|f| read(f))
            .collect::<Vec<_>>()
            .join("\n");
        let removed = ["removed.txt", "removed_custom.txt"]
            .iter()
            .filter_map(|f| read(f))
            .collect::<Vec<_>>()
            .join("\n");
        tagger.load_manual_data(&added, &removed);
        tagger.finish();
        tagger
    }

    pub fn load_fsa_data(&mut self, data: &str) -> usize {
        let mut count = 0;
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            let (Some(form), Some(lemma), Some(tag)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let tag = tag.trim();
            if form.is_empty() || tag.is_empty() {
                continue;
            }
            let tag_id = self.intern_tag(tag);
            let lemma: Box<str> = if lemma.is_empty() {
                form.into()
            } else {
                lemma.into()
            };
            self.fsa
                .entry(form.into())
                .or_default()
                .push(TaggedWord { lemma, tag: tag_id });
            count += 1;
        }
        count
    }

    pub fn load_manual_data(&mut self, added: &str, removed: &str) {
        let mut separator = "\t";
        for line in added.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("#separatorRegExp=") {
                separator = rest;
                continue;
            }
            if let Some((form, lemma, tag)) = split_manual_line(line, separator) {
                let tag_id = self.intern_tag(tag);
                self.manual
                    .entry(form.into())
                    .or_default()
                    .push(TaggedWord {
                        lemma: lemma.into(),
                        tag: tag_id,
                    });
            }
        }
        for line in removed.lines() {
            let line = line.trim();
            if let Some((form, lemma, tag)) = split_manual_line(line, separator) {
                let tag_id = self.intern_tag(tag);
                self.removals
                    .entry(form.into())
                    .or_default()
                    .push((lemma.into(), tag_id));
            }
        }
    }

    pub fn finish(&mut self) {
        let fsa = std::mem::take(&mut self.fsa);
        let manual = std::mem::take(&mut self.manual);
        let removals = std::mem::take(&mut self.removals);
        let mut merged: HashMap<Box<str>, Vec<TaggedWord>> = manual;
        for (form, mut readings) in fsa {
            merged.entry(form).or_default().append(&mut readings);
        }
        self.dict = merged
            .into_iter()
            .map(|(form, mut readings)| {
                if let Some(removals) = removals.get(&form) {
                    readings.retain(|tw| {
                        !removals
                            .iter()
                            .any(|(lemma, tag)| tw.tag == *tag && *tw.lemma == *lemma.as_ref())
                    });
                }
                (form, readings.into_boxed_slice())
            })
            .collect();
    }

    pub fn entry_count(&self) -> usize {
        self.dict.len()
    }

    fn intern_tag(&mut self, tag: &str) -> u32 {
        if let Some(pos) = self.tags.iter().position(|t| &**t == tag) {
            return pos as u32;
        }
        self.tags.push(tag.into());
        (self.tags.len() - 1) as u32
    }

    fn lookup(&self, word: &str) -> &[TaggedWord] {
        self.dict.get(word).map(|v| &**v).unwrap_or(&[])
    }

    fn readings_for(
        &self,
        surface: &str,
        word: &str,
        start: usize,
        end: usize,
    ) -> Vec<AnalyzedToken> {
        let entries = self.lookup(word);
        entries
            .iter()
            .map(|tw| {
                AnalyzedToken::new(surface, start, end)
                    .with_lemma(tw.lemma.to_string())
                    .with_pos_tags(vec![self.tags[tw.tag as usize].to_string()])
            })
            .collect()
    }

    pub fn tag_word(&self, surface: &str, start: usize, end: usize) -> Vec<AnalyzedToken> {
        let word = normalize_english_word(surface);

        let lower_word = word.to_lowercase();
        let is_lowercase = word == lower_word;
        let is_mixed = is_mixed_case(&word);
        let is_all_upper = is_all_uppercase(&word);

        let mut readings = self.readings_for(&word, &word, start, end);
        if !is_lowercase && !is_mixed {
            readings.extend(self.readings_for(&word, &lower_word, start, end));
        }
        if readings.is_empty() && is_all_upper {
            let first_upper = uppercase_first_char(&lower_word);
            readings.extend(self.readings_for(&word, &first_upper, start, end));
        }
        if readings.is_empty() && lower_word.ends_with("in'") {
            let mut corrected = word.to_string();
            let corrected_tail = if is_all_upper { 'G' } else { 'g' };
            corrected.pop();
            corrected.push(corrected_tail);
            readings.extend(self.readings_for(&word, &corrected, start, end));
            if !is_lowercase && !is_mixed {
                readings.extend(self.readings_for(&word, &corrected.to_lowercase(), start, end));
            }
        }
        readings
    }
}

fn normalize_english_word(surface: &str) -> std::borrow::Cow<'_, str> {
    if surface.chars().count() > 1 && surface.contains('’') {
        std::borrow::Cow::Owned(surface.replace('’', "'"))
    } else {
        std::borrow::Cow::Borrowed(surface)
    }
}

impl Default for EnglishTagger {
    fn default() -> Self {
        Self::new()
    }
}

impl Tagger for EnglishTagger {
    fn tag(&self, tokens: &[&str]) -> Vec<AnalyzedTokenReadings> {
        let mut results = Vec::with_capacity(tokens.len());
        let mut byte_offset = 0;
        for token_text in tokens {
            let start = byte_offset;
            let end = start + token_text.len();
            let word = normalize_english_word(token_text);
            let readings = self.tag_word(token_text, start, end);
            results.push(build_atr(&word, start, end, readings));
            byte_offset = end;
        }
        results
    }
}

fn build_atr(
    word: &str,
    start: usize,
    end: usize,
    readings: Vec<AnalyzedToken>,
) -> AnalyzedTokenReadings {
    let all_tags: Vec<String> = readings
        .iter()
        .flat_map(|r| r.pos_tags().to_vec())
        .collect();
    let lemma = readings
        .first()
        .and_then(|r| r.lemma().map(str::to_string))
        .unwrap_or_else(|| word.to_string());
    let primary = AnalyzedToken::new(word, start, end)
        .with_pos_tags(all_tags)
        .with_lemma(lemma);
    let mut atr = AnalyzedTokenReadings::new(primary);
    if !readings.is_empty() {
        atr = atr.with_readings(readings);
    }
    atr
}

impl og_core::checker::Tagger for EnglishTagger {
    fn tag(&self, tokens: &mut [og_core::AnalyzedTokenReadings]) {
        for atr in tokens.iter_mut().skip(1) {
            let primary = atr.token();
            let surface = primary.token().to_string();
            let start = primary.start();
            let end = primary.end();
            let word = normalize_english_word(&surface);
            let readings = self.tag_word(&surface, start, end);
            let all_tags: Vec<String> = readings
                .iter()
                .flat_map(|r| r.pos_tags().to_vec())
                .collect();
            let lemma = readings
                .first()
                .and_then(|r| r.lemma().map(str::to_string))
                .unwrap_or_else(|| word.to_string());
            let updated = AnalyzedToken::new(word.as_ref(), start, end)
                .with_pos_tags(all_tags)
                .with_lemma(lemma);
            let mut next = AnalyzedTokenReadings::new(updated);
            if !readings.is_empty() {
                next = next.with_readings(readings);
            }
            *atr = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_tagger() -> EnglishTagger {
        let mut t = EnglishTagger::new();
        t.load_fsa_data(
            "the\tthe\tDT\nrun\trun\tNN\nrun\trun\tVB\nruns\trun\tVBZ\nrunning\trun\tVBG\nFrance\tFrance\tNNP\njohn\tjohn\tNNP\n",
        );
        t.finish();
        t
    }

    #[test]
    fn exact_form_lookup_before_lowercase() {
        let t = loaded_tagger();
        let atr = t.tag(&["the"]).remove(0);
        let tags: Vec<&str> = atr.all_pos_tags();
        assert_eq!(tags, vec!["DT"]);
    }

    #[test]
    fn capitalized_word_gets_lowercase_readings() {
        let t = loaded_tagger();
        let atr = t.tag(&["The"]).remove(0);
        assert!(atr.has_pos_tag("DT"));
    }

    #[test]
    fn all_uppercase_proper_noun_uses_first_upper_lookup() {
        let t = loaded_tagger();
        let atr = t.tag(&["FRANCE"]).remove(0);
        assert!(atr.has_pos_tag("NNP"));
    }

    #[test]
    fn mixed_case_gets_no_lowercase_readings() {
        let t = loaded_tagger();
        let atr = t.tag(&["RuNning"]).remove(0);
        assert!(atr.all_pos_tags().is_empty());
    }

    #[test]
    fn typographic_apostrophe_is_normalized() {
        let mut t = EnglishTagger::new();
        t.load_fsa_data("can't\tcan\tMD\n");
        t.finish();
        let atr = t.tag(&["can’t"]).remove(0);
        assert!(atr.has_pos_tag("MD"));
        assert_eq!(atr.token().token(), "can't");
    }

    #[test]
    fn in_suffix_falls_back_to_ing() {
        let mut t = EnglishTagger::new();
        t.load_fsa_data("running\trun\tVBG\n");
        t.finish();
        let atr = t.tag(&["runnin'"]).remove(0);
        assert!(atr.has_pos_tag("VBG"));
        let atr = t.tag(&["RUNNIN'"]).remove(0);
        assert!(atr.has_pos_tag("VBG"));
    }

    #[test]
    fn manual_additions_come_first_and_removals_apply() {
        let mut t = EnglishTagger::new();
        t.load_fsa_data("word\tword\tNN\n");
        t.load_manual_data("word\tword\tVB\n", "other\tother\tNNP\n");
        t.finish();
        let atr = t.tag(&["word"]).remove(0);
        assert_eq!(atr.all_pos_tags(), vec!["VB", "NN"]);
        assert!(t.tag(&["other"]).remove(0).all_pos_tags().is_empty());
    }

    #[test]
    fn offsets_are_exact_without_separator_guessing() {
        let t = loaded_tagger();
        let result = t.tag(&["the", "runs"]);
        assert_eq!(result[0].token().start(), 0);
        assert_eq!(result[0].token().end(), 3);
        assert_eq!(result[1].token().start(), 3);
        assert_eq!(result[1].token().end(), 7);
    }

    #[test]
    fn manual_lines_support_comments() {
        let mut t = EnglishTagger::new();
        t.load_manual_data("# comment\nword\tword\tNN # trailing\n", "");
        t.finish();
        assert!(t.tag(&["word"]).remove(0).has_pos_tag("NN"));
    }

    #[test]
    fn real_dictionary_loads_and_tags_like_java() {
        let dir = Path::new(
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/resource/en",
        );
        if !dir.exists() {
            return;
        }
        let t = EnglishTagger::from_resource_dir(dir);
        assert!(t.entry_count() > 200_000, "expected full dictionary");
        assert!(t.tag(&["the"]).remove(0).has_pos_tag("DT"));
        assert!(t.tag(&["House"]).remove(0).has_pos_tag("NN"));
        assert!(t.tag(&["running"]).remove(0).has_pos_tag("VBG"));
        assert!(t.tag(&["better"]).remove(0).has_pos_tag("JJR"));
    }
}
