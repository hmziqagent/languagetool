use og_core::{AnalyzedSentence, AnalyzedTokenReadings};

pub fn uppercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn lowercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

pub fn starts_with_uppercase(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_uppercase())
}

pub fn is_all_uppercase(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| !c.is_lowercase())
}

pub fn is_capitalized_word(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_uppercase())
        && chars.all(|c| !c.is_uppercase())
        && s.chars().any(|c| c.is_alphabetic())
}

pub fn has_pos_tag_starting_with(token: &AnalyzedTokenReadings, prefix: &str) -> bool {
    token
        .readings()
        .iter()
        .any(|r| r.pos_tags().iter().any(|t| t.starts_with(prefix)))
}

pub fn matches_pos_tag_regex(token: &AnalyzedTokenReadings, pattern: &str) -> bool {
    token.has_pos_tag_matching(pattern)
}

pub fn is_tagged(token: &AnalyzedTokenReadings) -> bool {
    token
        .readings()
        .iter()
        .any(|r| r.pos_tags().iter().any(|t| !t.is_empty()))
}

pub fn is_pos_tag_unknown(token: &AnalyzedTokenReadings) -> bool {
    !is_tagged(token)
}

pub fn has_same_lemmas(token: &AnalyzedTokenReadings) -> bool {
    let mut lemmas = token.readings().iter().map(|r| r.lemma());
    let first = lemmas.next().flatten();
    first.is_some_and(|first| lemmas.all(|l| l == Some(first)))
}

pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    let mut a_chars = a.chars().flat_map(char::to_lowercase);
    let mut b_chars = b.chars().flat_map(char::to_lowercase);
    loop {
        match (a_chars.next(), b_chars.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x == y => continue,
            _ => return false,
        }
    }
}

pub fn levenshtein_at_most_one(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let mut i = 0;
    let mut j = 0;
    let mut diff = 0;
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else {
            diff += 1;
            if diff > 1 {
                return false;
            }
            if a.len() < b.len() {
                j += 1;
            } else if a.len() > b.len() {
                i += 1;
            } else {
                i += 1;
                j += 1;
            }
        }
    }
    diff + (a.len() - i) + (b.len() - j) <= 1
}

pub fn is_paragraph_end(sentences: &[AnalyzedSentence], n: usize) -> bool {
    if sentences.is_empty() || n >= sentences.len() - 1 {
        return true;
    }
    let text = sentences[n].text();
    if text.ends_with("\n\n")
        || text.ends_with("\n\r\n\r")
        || text.ends_with("\r\n\r\n")
        || sentences[n + 1].text().starts_with('\n')
        || sentences[n + 1].text().starts_with("\r\n")
    {
        return true;
    }
    false
}

pub fn has_no_whitespace_before(
    sentence: &AnalyzedSentence,
    token: &AnalyzedTokenReadings,
) -> bool {
    let rel = token.token().start().saturating_sub(sentence.start());
    match sentence
        .text()
        .get(..rel)
        .and_then(|s| s.chars().next_back())
    {
        Some(c) => !c.is_whitespace(),
        None => false,
    }
}
