use og_core::checker::Disambiguator;
use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings};
use std::collections::HashMap;
use std::path::Path;

const MAX_TOKENS_IN_MULTIWORD: usize = 20;
const TAG_FOR_NOT_ADDING_TAGS: &str = "_NONE_";

struct MultiwordEntry {
    tag: String,
    lemma: String,
}

pub struct MultiWordChunker {
    m_start_space: HashMap<String, usize>,
    m_start_no_space: HashMap<String, usize>,
    m_full_space: HashMap<String, MultiwordEntry>,
    m_full_no_space: HashMap<String, MultiwordEntry>,
    remove_previous_tags: bool,
}

fn is_camel_case(token: &str) -> bool {
    let bytes: Vec<char> = token.chars().collect();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_lowercase() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() || !bytes[i].is_ascii_uppercase() {
        return false;
    }
    i + 1 < bytes.len() && bytes[i + 1..].iter().all(|c| c.is_ascii_alphabetic())
}

fn uppercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn load_lines(content: &str) -> (String, Vec<String>) {
    let mut separator = "\t".to_string();
    let mut lines = Vec::new();
    for raw in content.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("#separatorRegExp=") {
            separator = rest.to_string();
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let expanded = expand_german_line(line);
        lines.extend(expanded);
    }
    (separator, lines)
}

fn expand_german_line(line: &str) -> Vec<String> {
    let suffix = line.rsplit('/').next().unwrap_or("");
    let has_slash = line.contains('/');
    if has_slash && !suffix.is_empty() && suffix.chars().all(|c| "ESN".contains(c)) {
        let stem = line.split('/').next().unwrap_or("").trim();
        let mut out = vec![stem.to_string()];
        if suffix.contains('E') {
            out.push(format!("{stem}e"));
        }
        if suffix.contains('S') {
            out.push(format!("{stem}s"));
        }
        if suffix.contains('N') {
            out.push(format!("{stem}n"));
        }
        return out;
    }
    vec![line.to_string()]
}

impl MultiWordChunker {
    pub fn from_file(
        path: &Path,
        allow_first_capitalized: bool,
        allow_all_uppercase: bool,
        allow_titlecase: bool,
        default_tag: Option<&str>,
        remove_previous_tags: bool,
    ) -> Option<Self> {
        let content = std::fs::read_to_string(path).ok()?;
        Some(Self::from_content(
            &content,
            allow_first_capitalized,
            allow_all_uppercase,
            allow_titlecase,
            default_tag,
            remove_previous_tags,
        ))
    }

    pub fn from_content(
        content: &str,
        allow_first_capitalized: bool,
        allow_all_uppercase: bool,
        allow_titlecase: bool,
        default_tag: Option<&str>,
        remove_previous_tags: bool,
    ) -> Self {
        let (separator, lines) = load_lines(content);
        let mut m_start_space: HashMap<String, usize> = HashMap::new();
        let mut m_start_no_space: HashMap<String, usize> = HashMap::new();
        let mut m_full_space: HashMap<String, MultiwordEntry> = HashMap::new();
        let mut m_full_no_space: HashMap<String, MultiwordEntry> = HashMap::new();

        for line in lines {
            let parts: Vec<&str> = line.split(&separator).collect();
            let tag = match default_tag {
                Some(dt) => {
                    if parts.len() != 1 {
                        continue;
                    }
                    dt.to_string()
                }
                None => {
                    if parts.len() != 2 {
                        continue;
                    }
                    parts[1].to_string()
                }
            };
            let original = parts[0];
            let contains_space = original.find(' ').map(|i| i > 0).unwrap_or(false);
            let mut variants: Vec<String> = vec![original.to_string()];
            if allow_all_uppercase && !is_camel_case(original) {
                let upper = original.to_uppercase();
                if upper != original
                    && !m_full_no_space.contains_key(&upper)
                    && !m_full_space.contains_key(&upper)
                {
                    variants.push(upper);
                }
            }
            if allow_first_capitalized {
                let first_cap = uppercase_first_char(original);
                if first_cap != original
                    && !m_full_no_space.contains_key(&first_cap)
                    && !m_full_space.contains_key(&first_cap)
                {
                    variants.push(first_cap.clone());
                }
                if allow_titlecase && original.split(' ').count() > 1 {
                    let all_lower = original
                        .split(' ')
                        .all(|w| w.chars().next().map(|c| c.is_lowercase()).unwrap_or(false));
                    if all_lower {
                        let naive = original
                            .split(' ')
                            .map(uppercase_first_char)
                            .collect::<Vec<_>>()
                            .join(" ");
                        if naive != first_cap && naive != original {
                            variants.push(naive);
                        }
                        let _ = &first_cap;
                    }
                }
            }
            for variant in variants {
                if contains_space {
                    let tokens: Vec<&str> = variant.split(' ').collect();
                    let first_token = tokens[0].to_string();
                    let count = tokens.len();
                    let entry = m_start_space.entry(first_token).or_insert(0);
                    if *entry < count {
                        *entry = count;
                    }
                    m_full_space.insert(
                        variant.clone(),
                        MultiwordEntry {
                            tag: tag.clone(),
                            lemma: original.to_string(),
                        },
                    );
                } else {
                    let first_char = variant
                        .chars()
                        .next()
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    if !first_char.is_empty() {
                        let len = variant.len();
                        let entry = m_start_no_space.entry(first_char).or_insert(0);
                        if *entry < len {
                            *entry = len;
                        }
                    }
                    m_full_no_space.insert(
                        variant.clone(),
                        MultiwordEntry {
                            tag: tag.clone(),
                            lemma: original.to_string(),
                        },
                    );
                }
            }
        }

        Self {
            m_start_space,
            m_start_no_space,
            m_full_space,
            m_full_no_space,
            remove_previous_tags,
        }
    }

    pub fn apply(&self, tokens: &mut [AnalyzedTokenReadings]) {
        let n = tokens.len();
        for i in 0..n {
            let mut tok = tokens[i].token().token().to_string();
            let mut k = i + 1;
            while k < n && !tokens[k].is_whitespace() {
                tok.push_str(tokens[k].token().token());
                k += 1;
            }
            if let Some(&max_len) = self.m_start_space.get(&tok) {
                let mut key = String::new();
                let mut final_len = 0usize;
                let mut len_counter = 0usize;
                let mut j = i;
                while j < n && j - i < MAX_TOKENS_IN_MULTIWORD {
                    if !tokens[j].is_whitespace() {
                        key.push_str(tokens[j].token().token());
                        if let Some(at) = self.m_full_space.get(&key)
                            && at.tag != TAG_FOR_NOT_ADDING_TAGS
                        {
                            if final_len == 0 {
                                let tag = at.tag.clone();
                                let lemma = at.lemma.clone();
                                add_plain_reading(&mut tokens[i], &tag, &lemma);
                            } else {
                                let tag = at.tag.clone();
                                let lemma = at.lemma.clone();
                                prepare_new_reading(&mut tokens[i], &tag, &lemma, false);
                                let tag = at.tag.clone();
                                let lemma = at.lemma.clone();
                                prepare_new_reading(&mut tokens[final_len], &tag, &lemma, true);
                            }
                        }
                    } else {
                        if j > 1 && !tokens[j - 1].is_whitespace() {
                            key.push(' ');
                            len_counter += 1;
                        }
                        if len_counter == max_len {
                            break;
                        }
                    }
                    j += 1;
                    final_len = j;
                }
            }
            let first_char = match tokens[i].token().token().chars().next() {
                Some(c) => c.to_string(),
                None => String::new(),
            };
            if !first_char.is_empty() && self.m_start_no_space.contains_key(&first_char) {
                let mut key = String::new();
                let mut j = i;
                while j < n && !tokens[j].is_whitespace() && j - i < MAX_TOKENS_IN_MULTIWORD {
                    key.push_str(tokens[j].token().token());
                    if let Some(at) = self.m_full_no_space.get(&key)
                        && at.tag != TAG_FOR_NOT_ADDING_TAGS
                    {
                        if i == j {
                            let tag = at.tag.clone();
                            let lemma = at.lemma.clone();
                            add_plain_reading(&mut tokens[i], &tag, &lemma);
                        } else {
                            let tag = at.tag.clone();
                            let lemma = at.lemma.clone();
                            prepare_new_reading(&mut tokens[i], &tag, &lemma, false);
                            let tag = at.tag.clone();
                            let lemma = at.lemma.clone();
                            prepare_new_reading(&mut tokens[j], &tag, &lemma, true);
                        }
                    }
                    j += 1;
                }
            }
        }
        if self.remove_previous_tags {
            self.remove_previous_tags(tokens);
        }
    }

    fn remove_previous_tags(&self, tokens: &mut [AnalyzedTokenReadings]) {
        let mut next_pos_tag = String::new();
        let mut lemma = String::new();
        let mut pos_tag = String::new();
        let mut i = 0;
        while i < tokens.len() {
            if !tokens[i].is_whitespace() {
                if !next_pos_tag.is_empty() {
                    let closing = format!("</{pos_tag}>");
                    let is_last = has_pos_tag_and_lemma(&tokens[i], &closing, &lemma);
                    replace_readings(&mut tokens[i], &[(&next_pos_tag, &lemma)]);
                    if is_last {
                        next_pos_tag.clear();
                        lemma.clear();
                    }
                } else if let Some((tag, entry_lemma)) = multiword_start_token(tokens, i) {
                    pos_tag = tag;
                    lemma = entry_lemma;
                    let closing = format!("</{pos_tag}>");
                    if has_pos_tag_and_lemma(&tokens[i], &closing, &lemma) {
                        remove_marker_readings(&mut tokens[i], &pos_tag);
                        add_plain_reading(&mut tokens[i], &pos_tag, &lemma);
                    } else {
                        replace_readings(&mut tokens[i], &[(&pos_tag, &lemma)]);
                        next_pos_tag = get_next_pos_tag(&pos_tag);
                    }
                }
            }
            i += 1;
        }
    }
}

fn get_next_pos_tag(postag: &str) -> String {
    if let Some(rest) = postag.strip_prefix("NC") {
        let mut out = String::from("AQ0");
        let chars: Vec<char> = rest.chars().collect();
        if chars.len() >= 2 {
            out.push(chars[0]);
            out.push(chars[1]);
        }
        out.push('0');
        return out;
    }
    if let Some(rest) = postag.strip_prefix("N ") {
        return format!("J {rest}");
    }
    postag.to_string()
}

fn is_low_priority_tag(tag: &str) -> bool {
    tag == "NPCN000"
}

fn multiword_start_token(tokens: &[AnalyzedTokenReadings], i: usize) -> Option<(String, String)> {
    let mut candidates: Vec<(&str, &str)> = Vec::new();
    for reading in tokens[i].readings() {
        if let Some(pos) = reading.pos_tags().first()
            && pos.starts_with('<')
            && pos.ends_with('>')
            && !pos.starts_with("</")
        {
            candidates.push((pos, reading.lemma().unwrap_or("")));
        }
    }
    if candidates.is_empty() {
        return None;
    }
    let mut selected: Option<(String, String)> = None;
    let mut max_distance = 0usize;
    for (pos, lemma) in candidates {
        let closing = format!("</{}", &pos[1..]);
        let inner_len = pos.chars().count();
        let clean_tag: String = pos
            .chars()
            .skip(1)
            .take(inner_len.saturating_sub(2))
            .collect();
        let mut distance = 1usize;
        while i + distance < tokens.len() {
            if has_pos_tag_and_lemma(&tokens[i + distance], &closing, lemma) {
                if distance > max_distance
                    || (distance == max_distance && !is_low_priority_tag(&clean_tag))
                {
                    max_distance = distance;
                    selected = Some((clean_tag_full(pos), lemma.to_string()));
                }
                break;
            }
            distance += 1;
        }
    }
    selected
}

fn clean_tag_full(pos: &str) -> String {
    let chars: Vec<char> = pos.chars().collect();
    chars[1..chars.len() - 1].iter().collect()
}

fn has_pos_tag_and_lemma(atr: &AnalyzedTokenReadings, pos_tag: &str, lemma: &str) -> bool {
    atr.readings().iter().any(|r| {
        r.pos_tags().first().map(|p| p.as_str()) == Some(pos_tag)
            && r.lemma().map(|l| l == lemma).unwrap_or(false)
    })
}

fn make_reading(atr: &AnalyzedTokenReadings, pos_tag: &str, lemma: &str) -> AnalyzedToken {
    let t = atr.token();
    AnalyzedToken::new(t.token(), t.start(), t.end())
        .with_pos_tags(vec![pos_tag.to_string()])
        .with_lemma(lemma)
}

fn prepare_new_reading(atr: &mut AnalyzedTokenReadings, tag: &str, lemma: &str, is_last: bool) {
    let marker = if is_last {
        format!("</{tag}>")
    } else {
        format!("<{tag}>")
    };
    let reading = make_reading(atr, &marker, lemma);
    push_reading(atr, reading);
}

fn add_plain_reading(atr: &mut AnalyzedTokenReadings, tag: &str, lemma: &str) {
    let reading = make_reading(atr, tag, lemma);
    push_reading(atr, reading);
}

fn push_reading(atr: &mut AnalyzedTokenReadings, reading: AnalyzedToken) {
    let readings = atr.readings_mut();
    let last_is_null = readings
        .last()
        .map(|r| r.pos_tags().is_empty())
        .unwrap_or(false);
    if last_is_null {
        readings.pop();
    }
    let exists = readings.iter().any(|r| {
        r.token() == reading.token()
            && r.pos_tags() == reading.pos_tags()
            && r.lemma() == reading.lemma()
    });
    if !exists {
        readings.push(reading);
    }
}

fn replace_readings(atr: &mut AnalyzedTokenReadings, new_readings: &[(&str, &str)]) {
    let chunks = atr.chunks().to_vec();
    let readings: Vec<AnalyzedToken> = new_readings
        .iter()
        .map(|(pos, lemma)| make_reading(atr, pos, lemma))
        .collect();
    let mut updated = match new_readings.first() {
        Some((pos, lemma)) => {
            let primary = make_reading(atr, pos, lemma);
            AnalyzedTokenReadings::new(primary).with_readings(readings)
        }
        None => AnalyzedTokenReadings::new(atr.token().clone()),
    };
    updated.set_chunks(chunks);
    *atr = updated;
}

fn remove_marker_readings(atr: &mut AnalyzedTokenReadings, pos_tag: &str) {
    let open = format!("<{pos_tag}>");
    let close = format!("</{pos_tag}>");
    atr.readings_mut().retain(|r| {
        !r.pos_tags()
            .first()
            .map(|p| p == &open || p == &close)
            .unwrap_or(false)
    });
}

pub struct MultiWordChunkerDisambiguator {
    chunker: MultiWordChunker,
}

impl MultiWordChunkerDisambiguator {
    pub fn from_resource_dir(resource_dir: &Path) -> Option<Self> {
        let path = resource_dir.join("multiwords.txt");
        if !path.exists() {
            return None;
        }
        let chunker = MultiWordChunker::from_file(&path, true, true, false, None, true)?;
        Some(Self { chunker })
    }
}

impl Disambiguator for MultiWordChunkerDisambiguator {
    fn disambiguate(&self, sentence: &mut AnalyzedSentence) {
        let tokens = sentence.tokens_mut();
        self.chunker.apply(tokens);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atr(text: &str, start: usize) -> AnalyzedTokenReadings {
        AnalyzedTokenReadings::new(AnalyzedToken::new(text, start, start + text.len()))
    }

    fn atr_with_space(text: &str, start: usize) -> AnalyzedTokenReadings {
        AnalyzedTokenReadings::new(AnalyzedToken::new(text, start, start + text.len()))
    }

    fn sentence_tokens(text: &str) -> Vec<AnalyzedTokenReadings> {
        let mut out = Vec::new();
        let mut pos = 0;
        let mut expect_space = false;
        for word in text.split(' ') {
            if expect_space {
                out.push(atr_with_space(" ", pos));
                pos += 1;
                expect_space = false;
            }
            if word.is_empty() {
                continue;
            }
            out.push(atr(word, pos));
            pos += word.len();
            expect_space = true;
        }
        out
    }

    #[test]
    fn multiword_phrase_replaces_readings() {
        let content = "status quo\tNN:UN\nquid pro quo\tNN\n";
        let chunker = MultiWordChunker::from_content(content, true, true, false, None, true);
        let mut tokens = sentence_tokens("The status quo is odd");
        chunker.apply(&mut tokens);
        let status = tokens
            .iter()
            .find(|t| t.token().token() == "status")
            .unwrap();
        let quo = tokens.iter().find(|t| t.token().token() == "quo").unwrap();
        assert_eq!(
            status
                .readings()
                .iter()
                .map(|r| (r.token(), r.pos_tags().concat(), r.lemma().unwrap_or("")))
                .map(|(a, b, c)| (a.to_string(), b, c.to_string()))
                .collect::<Vec<_>>(),
            vec![(
                "status".to_string(),
                "NN:UN".to_string(),
                "status quo".to_string()
            )]
        );
        assert_eq!(
            quo.readings()
                .iter()
                .map(|r| (r.token(), r.pos_tags().concat(), r.lemma().unwrap_or("")))
                .map(|(a, b, c)| (a.to_string(), b, c.to_string()))
                .collect::<Vec<_>>(),
            vec![(
                "quo".to_string(),
                "NN:UN".to_string(),
                "status quo".to_string()
            )]
        );
    }

    #[test]
    fn single_token_entry_keeps_existing_readings() {
        let content = "coffee\tNN:UN\n";
        let chunker = MultiWordChunker::from_content(content, true, true, false, None, true);
        let mut tokens = sentence_tokens("coffee");
        tokens[0].readings_mut().push(
            AnalyzedToken::new("coffee", 0, 6)
                .with_pos_tags(vec!["NN".to_string()])
                .with_lemma("coffee"),
        );
        chunker.apply(&mut tokens);
        assert!(tokens[0].readings().len() >= 2);
        assert!(tokens[0].has_pos_tag("NN:UN"));
    }

    #[test]
    fn no_space_entry_spans_split_tokens() {
        let content = "don't\tVB\n";
        let chunker = MultiWordChunker::from_content(content, true, true, false, None, true);
        let mut tokens = vec![atr("do", 0), atr("n't", 2), atr("!", 5)];
        chunker.apply(&mut tokens);
        assert_eq!(
            tokens[0]
                .readings()
                .iter()
                .map(|r| (r.pos_tags().concat(), r.lemma().unwrap_or("").to_string()))
                .collect::<Vec<_>>(),
            vec![("VB".to_string(), "don't".to_string())]
        );
        assert_eq!(
            tokens[1]
                .readings()
                .iter()
                .map(|r| (r.pos_tags().concat(), r.lemma().unwrap_or("").to_string()))
                .collect::<Vec<_>>(),
            vec![("VB".to_string(), "don't".to_string())]
        );
    }

    #[test]
    fn casing_variants_match() {
        let content = "big sur\tNNP\n";
        let chunker = MultiWordChunker::from_content(content, true, true, false, None, true);
        let mut tokens = sentence_tokens("We visited Big sur today");
        chunker.apply(&mut tokens);
        let big = tokens.iter().find(|t| t.token().token() == "Big").unwrap();
        assert_eq!(
            big.readings()
                .iter()
                .map(|r| (r.pos_tags().concat(), r.lemma().unwrap_or("").to_string()))
                .collect::<Vec<_>>(),
            vec![("NNP".to_string(), "big sur".to_string())]
        );
        let mut title = sentence_tokens("We visited Big Sur today");
        chunker.apply(&mut title);
        let big = title.iter().find(|t| t.token().token() == "Big").unwrap();
        assert!(big.readings().iter().all(|r| r.pos_tags().is_empty()));
    }
}
