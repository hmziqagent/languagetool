use std::path::Path;
use std::sync::LazyLock;

use og_core::{AnalyzedToken, AnalyzedTokenReadings};

use crate::generic_tagger::GenericTagger;

const TASHKEEL_CHARS: &[char] = &[
    '\u{64b}', '\u{64c}', '\u{64d}', '\u{64e}', '\u{64f}', '\u{650}', '\u{651}', '\u{652}',
    '\u{653}', '\u{654}', '\u{655}', '\u{656}', '\u{640}',
];

static PRONOUN_SUFFIX_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        "(\u{643}|\u{647}\u{627}|\u{647}\u{645}\u{627}|\u{647}\u{645}|\u{647}\u{646}|\u{643}\u{645}\u{627}|\u{643}\u{645}|\u{643}\u{646}|\u{646}\u{627}|\u{64a})$",
    )
    .expect("valid pronoun suffix pattern")
});

fn noun_flag_pos(flag_type: &str) -> usize {
    match flag_type {
        "CATEGORY" => 1,
        "GENDER" => 4,
        "NUMBER" => 5,
        "CASE" => 6,
        "INFLECT_MARK" => 7,
        "CONJ" => 9,
        "JAR" => 10,
        "PRONOUN" => 11,
        _ => 0,
    }
}

fn verb_flag_pos(flag_type: &str) -> usize {
    match flag_type {
        "CATEGORY" => 1,
        "TRANS" => 2,
        "GENDER" => 4,
        "NUMBER" => 5,
        "PERSON" => 6,
        "INFLECT_MARK" => 7,
        "TENSE" => 8,
        "VOICE" => 9,
        "CASE" => 10,
        "CONJ" => 12,
        "ISTIQBAL" => 13,
        "PRONOUN" => 14,
        _ => 0,
    }
}

fn particle_flag_pos(flag_type: &str) -> usize {
    match flag_type {
        "CATEGORY" => 1,
        "OPTION" => 2,
        "GENDER" => 4,
        "NUMBER" => 5,
        "CASE" => 6,
        "CONJ" => 8,
        "JAR" => 9,
        "PRONOUN" => 10,
        _ => 0,
    }
}

fn is_noun(postag: &str) -> bool {
    postag.starts_with('N')
}

fn is_verb(postag: &str) -> bool {
    postag.starts_with('V')
}

fn is_stopword_tag(postag: &str) -> bool {
    postag.starts_with('P')
}

fn flag_pos(postag: &str, flag_type: &str) -> usize {
    if is_noun(postag) {
        noun_flag_pos(flag_type)
    } else if is_verb(postag) {
        verb_flag_pos(flag_type)
    } else if is_stopword_tag(postag) {
        particle_flag_pos(flag_type)
    } else {
        0
    }
}

fn get_flag(postag: &str, flag_type: &str) -> char {
    let chars: Vec<char> = postag.chars().collect();
    let pos = flag_pos(postag, flag_type);
    if pos < chars.len() { chars[pos] } else { '-' }
}

fn set_flag(postag: &str, flag_type: &str, flag: char) -> String {
    let mut chars: Vec<char> = postag.chars().collect();
    let pos = flag_pos(postag, flag_type);
    if pos < chars.len() {
        chars[pos] = flag;
    }
    chars.into_iter().collect()
}

fn is_majrour(postag: &str) -> bool {
    let flag = get_flag(postag, "CASE");
    flag == 'I' || flag == '-'
}

fn is_future_tense(postag: &str) -> bool {
    is_verb(postag) && get_flag(postag, "TENSE") == 'f'
}

fn is_unattached_noun(postag: &str) -> bool {
    is_noun(postag) && get_flag(postag, "PRONOUN") != 'H' && !postag.ends_with('X')
}

fn modify_pos_tag(postag: &str, tags: &[String]) -> Option<String> {
    let mut current = postag.to_string();
    for tg in tags {
        current = add_tag(&current, tg)?;
    }
    Some(current)
}

fn add_tag(postag: &str, flag_string: &str) -> Option<String> {
    let mut parts = flag_string.split(';');
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    let (flag_type, flag) = if second.is_empty() {
        ("", first)
    } else {
        (first, second)
    };

    let mut result = match flag {
        "W" => set_flag(postag, "CONJ", 'W'),
        "K" => {
            if is_noun(postag) && is_majrour(postag) {
                set_flag(postag, "JAR", 'K')
            } else {
                return None;
            }
        }
        "B" => {
            if is_noun(postag) && is_majrour(postag) {
                set_flag(postag, "JAR", 'B')
            } else {
                return None;
            }
        }
        "L" => {
            if is_noun(postag) {
                if is_majrour(postag) {
                    set_flag(postag, "JAR", 'L')
                } else {
                    return None;
                }
            } else {
                set_flag(postag, "ISTIQBAL", 'L')
            }
        }
        "D" => {
            if is_unattached_noun(postag) {
                set_flag(postag, "PRONOUN", 'L')
            } else {
                return None;
            }
        }
        "S" => {
            if is_future_tense(postag) {
                set_flag(postag, "ISTIQBAL", 'S')
            } else {
                return None;
            }
        }
        _ => postag.to_string(),
    };

    if flag_type == "PRONOUN" && !flag.is_empty() && flag != "D" {
        result = set_flag(&result, flag_type, flag.chars().next().unwrap_or('-'));
    }
    Some(result)
}

pub struct ArabicTagger {
    inner: GenericTagger,
}

impl ArabicTagger {
    pub fn from_resource_dir(resource_dir: &Path) -> Self {
        Self {
            inner: GenericTagger::from_resource_dir(resource_dir, "arabic"),
        }
    }

    pub fn has_dictionary(&self) -> bool {
        self.inner.has_dictionary()
    }

    fn remove_tashkeel(word: &str) -> String {
        word.chars()
            .filter(|c| !TASHKEEL_CHARS.contains(c))
            .collect()
    }

    fn tag_token(&self, atr: &AnalyzedTokenReadings) -> AnalyzedTokenReadings {
        let surface = atr.token().token();
        let start = atr.token().start();
        let end = atr.token().end();
        let striped = Self::remove_tashkeel(surface);

        let mut readings: Vec<AnalyzedToken> = self
            .inner
            .lookup(&striped)
            .into_iter()
            .map(|(lemma, tag)| {
                AnalyzedToken::new(surface, start, end)
                    .with_lemma(lemma.to_string())
                    .with_pos_tags(vec![tag.to_string()])
            })
            .collect();

        let is_stop = readings
            .iter()
            .any(|r| r.pos_tags().iter().any(|t| t.starts_with('P')));
        if !is_stop {
            readings.extend(self.additional_tags(surface, &striped, start, end));
        }

        if readings.is_empty() {
            return atr.clone();
        }

        let all_tags: Vec<String> = readings
            .iter()
            .flat_map(|r| r.pos_tags().to_vec())
            .collect();
        let lemma = readings
            .first()
            .and_then(|r| r.lemma().map(str::to_string))
            .unwrap_or_else(|| surface.to_string());
        let primary = AnalyzedToken::new(surface, start, end)
            .with_pos_tags(all_tags)
            .with_lemma(lemma);
        AnalyzedTokenReadings::new(primary).with_readings(readings)
    }

    fn additional_tags(
        &self,
        word: &str,
        striped: &str,
        start: usize,
        end: usize,
    ) -> Vec<AnalyzedToken> {
        let mut additional = Vec::new();
        let chars: Vec<char> = striped.chars().collect();
        for i in prefix_index_list(&chars) {
            for j in suffix_index_list(&chars) {
                if i == 0 && j == chars.len() {
                    continue;
                }
                let tags = get_tags(&chars, i, j);
                for stem in get_stem(&chars, i, j) {
                    for (lemma, postag) in self.inner.lookup(&stem) {
                        if let Some(modified) = modify_pos_tag(postag, &tags) {
                            additional.push(
                                AnalyzedToken::new(word, start, end)
                                    .with_lemma(lemma.to_string())
                                    .with_pos_tags(vec![modified]),
                            );
                        }
                    }
                }
            }
        }
        additional
    }
}

fn prefix_index_list(chars: &[char]) -> Vec<usize> {
    let mut indexes = vec![0usize];
    let word: String = chars.iter().collect();
    let starts_with = |p: &str| word.starts_with(p);

    if starts_with("\u{648}\u{643}\u{627}\u{644}")
        || starts_with("\u{648}\u{628}\u{627}\u{644}")
        || starts_with("\u{641}\u{643}\u{627}\u{644}")
        || starts_with("\u{641}\u{628}\u{627}\u{644}")
    {
        indexes.push(4);
    }

    if starts_with("\u{648}\u{644}\u{644}")
        || starts_with("\u{641}\u{644}\u{644}")
        || starts_with("\u{641}\u{627}\u{644}")
        || starts_with("\u{648}\u{627}\u{644}")
        || starts_with("\u{628}\u{627}\u{644}")
        || starts_with("\u{643}\u{627}\u{644}")
    {
        indexes.push(3);
    }

    if starts_with("\u{644}\u{644}")
        || starts_with("\u{648}\u{643}")
        || starts_with("\u{648}\u{644}")
        || starts_with("\u{648}\u{628}")
        || starts_with("\u{641}\u{643}")
        || starts_with("\u{641}\u{644}")
        || starts_with("\u{641}\u{628}")
        || starts_with("\u{627}\u{644}")
        || starts_with("\u{641}\u{633}\u{623}")
        || starts_with("\u{641}\u{633}\u{646}")
        || starts_with("\u{641}\u{633}\u{64a}")
        || starts_with("\u{641}\u{633}\u{62a}")
        || starts_with("\u{648}\u{633}\u{623}")
        || starts_with("\u{648}\u{633}\u{646}")
        || starts_with("\u{648}\u{633}\u{64a}")
        || starts_with("\u{648}\u{633}\u{62a}")
    {
        indexes.push(2);
    }

    if starts_with("\u{643}")
        || starts_with("\u{644}")
        || starts_with("\u{628}")
        || starts_with("\u{648}")
        || starts_with("\u{641}")
        || starts_with("\u{633}\u{623}")
        || starts_with("\u{633}\u{646}")
        || starts_with("\u{633}\u{64a}")
        || starts_with("\u{633}\u{62a}")
    {
        indexes.push(1);
    }

    indexes
}

fn suffix_index_list(chars: &[char]) -> Vec<usize> {
    let mut indexes = vec![chars.len()];
    let word: String = chars.iter().collect();
    let suffix_len = if word.ends_with('\u{643}') {
        Some(1)
    } else if word.ends_with("\u{647}\u{645}\u{627}") || word.ends_with("\u{643}\u{645}\u{627}") {
        Some(3)
    } else if word.ends_with("\u{647}\u{627}")
        || word.ends_with("\u{647}\u{645}")
        || word.ends_with("\u{647}\u{646}")
        || word.ends_with("\u{643}\u{645}")
        || word.ends_with("\u{643}\u{646}")
        || word.ends_with("\u{646}\u{627}")
    {
        Some(2)
    } else {
        None
    };
    if let Some(len) = suffix_len {
        indexes.push(chars.len() - len);
    }
    indexes
}

fn get_tags(chars: &[char], pos_start: usize, pos_end: usize) -> Vec<String> {
    let mut tags = Vec::new();
    let mut prefix: String = chars[..pos_start].iter().collect();
    let suffix: String = chars[pos_end..].iter().collect();

    if prefix.starts_with('\u{648}') || prefix.starts_with('\u{641}') {
        tags.push("CONJ;W".to_string());
        prefix = prefix
            .trim_start_matches(|c| c == '\u{648}' || c == '\u{641}')
            .to_string();
    }
    if prefix.starts_with('\u{643}') {
        tags.push("JAR;K".to_string());
    } else if prefix.starts_with('\u{644}') {
        tags.push("JAR;L".to_string());
    } else if prefix.starts_with('\u{628}') {
        tags.push("JAR;B".to_string());
    } else if prefix.starts_with('\u{633}') {
        tags.push("ISTIQBAL;S".to_string());
    }
    if prefix.ends_with("\u{627}\u{644}") || prefix.ends_with("\u{644}\u{644}") {
        tags.push("PRONOUN;D".to_string());
    }

    match suffix.as_str() {
        "\u{646}\u{64a}"
        | "\u{646}\u{627}"
        | "\u{643}"
        | "\u{643}\u{645}\u{627}"
        | "\u{643}\u{645}"
        | "\u{643}\u{646}"
        | "\u{647}"
        | "\u{647}\u{627}"
        | "\u{647}\u{645}\u{627}"
        | "\u{647}\u{645}"
        | "\u{647}\u{646}" => {
            tags.push("PRONOUN;H".to_string());
        }
        _ => {}
    }

    tags
}

fn get_stem(chars: &[char], pos_start: usize, pos_end: usize) -> Vec<String> {
    let mut stem: String = chars[pos_start..].iter().collect();
    if pos_end != chars.len() {
        stem = PRONOUN_SUFFIX_RE.replace(&stem, "\u{647}").into_owned();
    }
    let prefix: String = chars[..pos_start].iter().collect();
    let mut stems = Vec::new();
    if prefix.ends_with("\u{644}\u{644}") {
        stems.push(format!("\u{644}{stem}"));
    }
    stems.push(stem);
    stems
}

impl og_core::checker::Tagger for ArabicTagger {
    fn tag(&self, tokens: &mut [AnalyzedTokenReadings]) {
        for atr in tokens.iter_mut().skip(1) {
            *atr = self.tag_token(atr);
        }
    }
}

impl crate::Tagger for ArabicTagger {
    fn tag(&self, tokens: &[&str]) -> Vec<AnalyzedTokenReadings> {
        let mut results = Vec::with_capacity(tokens.len());
        let mut byte_offset = 0;
        for token_text in tokens {
            let start = byte_offset;
            let end = start + token_text.len();
            let striped = Self::remove_tashkeel(token_text);

            let mut readings: Vec<AnalyzedToken> = self
                .inner
                .lookup(&striped)
                .into_iter()
                .map(|(lemma, tag)| {
                    AnalyzedToken::new(*token_text, start, end)
                        .with_lemma(lemma.to_string())
                        .with_pos_tags(vec![tag.to_string()])
                })
                .collect();

            let is_stop = readings
                .iter()
                .any(|r| r.pos_tags().iter().any(|t| t.starts_with('P')));
            if !is_stop {
                readings.extend(self.additional_tags(token_text, &striped, start, end));
            }

            if readings.is_empty() {
                results.push(AnalyzedTokenReadings::new(AnalyzedToken::new(
                    *token_text,
                    start,
                    end,
                )));
            } else {
                let all_tags: Vec<String> = readings
                    .iter()
                    .flat_map(|r| r.pos_tags().to_vec())
                    .collect();
                let lemma = readings
                    .first()
                    .and_then(|r| r.lemma().map(str::to_string))
                    .unwrap_or_else(|| (*token_text).to_string());
                let primary = AnalyzedToken::new(*token_text, start, end)
                    .with_pos_tags(all_tags)
                    .with_lemma(lemma);
                results.push(AnalyzedTokenReadings::new(primary).with_readings(readings));
            }
            byte_offset = end;
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_direct_lookup_count() {
        let dir = std::path::Path::new(
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/resource/ar",
        );
        let t = ArabicTagger::from_resource_dir(dir);
        let toks = vec!["مليونين".to_string()];
        let refs: Vec<&str> = toks.iter().map(|s| s.as_str()).collect();
        let out = crate::Tagger::tag(&t, &refs);
        for atr in &out {
            eprintln!("readings {}", atr.readings().len());
            for r in atr.readings() {
                eprintln!(
                    "  {} [{}]",
                    r.lemma().unwrap_or("-"),
                    r.pos_tags().join(",")
                );
            }
        }
    }

    #[test]
    fn modifies_conjunction_flag_on_prefixed_noun_tag() {
        let base = "NNH;-2A;---";
        let modified = modify_pos_tag(base, &["CONJ;W".to_string()]).expect("compatible");
        let chars: Vec<char> = modified.chars().collect();
        assert_eq!(
            chars[9], 'W',
            "CONJ flag set at noun position 9: {modified}"
        );
    }

    #[test]
    fn rejects_jar_on_non_majrour_noun() {
        let base = "NNH;-2U;---";
        assert!(modify_pos_tag(base, &["JAR;L".to_string()]).is_none());
    }

    #[test]
    fn prefix_and_suffix_index_lists_match_java() {
        let w: Vec<char> = "\u{648}\u{645}\u{626}\u{62a}\u{627}\u{646}"
            .chars()
            .collect();
        assert_eq!(prefix_index_list(&w), vec![0, 1]);
        assert_eq!(suffix_index_list(&w), vec![w.len()]);

        let w2: Vec<char> = "\u{648}\u{627}\u{644}\u{628}\u{64a}\u{62a}"
            .chars()
            .collect();
        assert_eq!(prefix_index_list(&w2), vec![0, 3, 1]);
    }

    #[test]
    fn stem_replaces_attached_pronoun() {
        let chars: Vec<char> = "\u{643}\u{62a}\u{627}\u{628}\u{647}\u{645}"
            .chars()
            .collect();
        let stems = get_stem(&chars, 0, chars.len() - 2);
        assert_eq!(
            stems,
            vec!["\u{643}\u{62a}\u{627}\u{628}\u{647}".to_string()]
        );
    }
}
