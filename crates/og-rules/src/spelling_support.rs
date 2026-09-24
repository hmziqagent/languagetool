use og_core::AnalyzedToken;
use og_spell::dictionary::Dictionary;
use og_spell::suggestion::SuggestionEngine;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

impl SpellingSupport {
    pub fn from_dirs(resource_dir: &Path, core_resource_dir: &Path) -> Option<Self> {
        let multitoken_file = if resource_dir.join("multiwords.txt").exists() {
            "multiwords.txt"
        } else {
            "multitoken-suggest.txt"
        };
        Self::from_dirs_with_multitoken(resource_dir, core_resource_dir, multitoken_file)
    }

    pub fn from_dirs_with_multitoken(
        resource_dir: &Path,
        core_resource_dir: &Path,
        multitoken_file: &str,
    ) -> Option<Self> {
        let mut dictionary =
            Dictionary::from_morfologik(&resource_dir.join("dict_decoded.txt")).ok()?;
        if let Ok(words) = std::fs::read_to_string(resource_dir.join("hunspell/spelling.txt")) {
            for line in words.lines() {
                let word = line.trim();
                if !word.is_empty() && !word.starts_with('#') {
                    dictionary.add_word(word);
                }
            }
        }
        let mut multitoken_data =
            std::fs::read_to_string(resource_dir.join(multitoken_file)).ok()?;
        if let Ok(global) = std::fs::read_to_string(core_resource_dir.join("spelling_global.txt")) {
            multitoken_data.push('\n');
            multitoken_data.push_str(&global);
        }
        Some(Self::new(dictionary, &multitoken_data))
    }
}

pub struct SpellingSupport {
    dictionary: Dictionary,
    suggestion_engine: SuggestionEngine,
    multitoken: MultitokenSpeller,
    tagger: Option<Arc<dyn og_tagger::Tagger>>,
}

impl SpellingSupport {
    pub fn new_with(dictionary: Dictionary, core_resource_dir: &Path, resource_dir: &Path) -> Self {
        let mut multitoken_data =
            std::fs::read_to_string(resource_dir.join("multitoken-suggest.txt"))
                .unwrap_or_default();
        if let Ok(global) = std::fs::read_to_string(core_resource_dir.join("spelling_global.txt")) {
            multitoken_data.push('\n');
            multitoken_data.push_str(&global);
        }
        if let Ok(spelling) = std::fs::read_to_string(resource_dir.join("hunspell/spelling.txt")) {
            multitoken_data.push('\n');
            multitoken_data.push_str(&spelling);
        }
        Self::new(dictionary, &multitoken_data)
    }

    pub fn new(dictionary: Dictionary, multitoken_data: &str) -> Self {
        let suggestion_engine = SuggestionEngine::new(&dictionary);
        Self {
            dictionary,
            suggestion_engine,
            multitoken: MultitokenSpeller::from_data(multitoken_data),
            tagger: None,
        }
    }

    pub fn dictionary(&self) -> &Dictionary {
        &self.dictionary
    }

    pub fn with_tagger(mut self, tagger: Arc<dyn og_tagger::Tagger>) -> Self {
        self.tagger = Some(tagger);
        self
    }

    pub fn is_misspelled(&self, word: &str) -> bool {
        if word.is_empty() || !word.chars().any(|c| c.is_alphabetic()) {
            return false;
        }
        !self.dictionary.contains(word)
    }

    pub fn suggestions(&self, word: &str, max: usize) -> Vec<String> {
        self.suggestion_engine.suggest(word, max)
    }

    pub fn tag(&self, word: &str) -> Vec<AnalyzedToken> {
        match &self.tagger {
            Some(tagger) => tagger
                .tag(&[word])
                .into_iter()
                .next()
                .map(|atr| atr.into_readings())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    pub fn multitoken_suggestions(&self, word: &str, accepted_by_speller: bool) -> Vec<String> {
        self.multitoken
            .get_suggestions(word, accepted_by_speller, &self.dictionary)
    }
}

pub struct MultitokenSpeller {
    by_first_char: HashMap<char, HashMap<String, Vec<String>>>,
    no_spaces: HashMap<String, Vec<String>>,
}

const MAX_LENGTH_DIFF: usize = 3;

impl MultitokenSpeller {
    pub fn from_data(data: &str) -> Self {
        let mut by_first_char: HashMap<char, HashMap<String, Vec<String>>> = HashMap::new();
        let mut no_spaces: HashMap<String, Vec<String>> = HashMap::new();
        for line_original in data.lines() {
            if line_original.is_empty() || line_original.starts_with('#') {
                continue;
            }
            let Some(line) = prepare_line_for_speller(line_original) else {
                continue;
            };
            if line.is_empty() {
                continue;
            }
            let normalized = normalize_key(&line);
            if !normalized.contains(' ') {
                continue;
            }
            let first_char = normalized.chars().next().unwrap_or(' ');
            by_first_char
                .entry(first_char)
                .or_default()
                .entry(normalized.clone())
                .or_default()
                .push(line.clone());
            no_spaces
                .entry(normalized.replace(' ', ""))
                .or_default()
                .push(line);
        }
        Self {
            by_first_char,
            no_spaces,
        }
    }

    pub fn get_suggestions(
        &self,
        original_word: &str,
        are_tokens_accepted_by_speller: bool,
        dict: &Dictionary,
    ) -> Vec<String> {
        let original_word = original_word.replace(['\u{00A0}', '\u{2007}', '\u{202F}'], " ");
        let word = original_word.replace("- ", "-").replace(" -", "-");
        if self.discard_run_on_words(&word, dict) {
            return Vec::new();
        }
        let normalized = normalize_key(&word);
        let mut weighted: Vec<(String, i64)> = Vec::new();

        if let Some(candidates) = self.no_spaces.get(&normalized.replace(' ', "")) {
            if stop_searching(candidates, &original_word) {
                return Vec::new();
            }
            for candidate in candidates {
                weighted.push((candidate.clone(), 0));
            }
        }

        if weighted.is_empty()
            && let Some(bucket) = self
                .by_first_char
                .get(&normalized.chars().next().unwrap_or(' '))
        {
            for (normalized_candidate, candidates) in bucket {
                if stop_searching(candidates, &original_word) {
                    return Vec::new();
                }
                if normalized_candidate
                    .chars()
                    .count()
                    .abs_diff(word.chars().count())
                    > MAX_LENGTH_DIFF
                {
                    continue;
                }
                let candidate_parts: Vec<&str> = normalized_candidate.split(' ').collect();
                let word_parts: Vec<&str> = normalized.split(' ').collect();
                let distances = distances_per_word(
                    &candidate_parts,
                    &word_parts,
                    normalized_candidate,
                    &normalized,
                );
                let total_distance: i64 = distances.iter().sum();
                if total_distance < 1 {
                    for candidate in candidates {
                        weighted.push((candidate.clone(), 0));
                    }
                    if weighted.len() == 2 {
                        break;
                    }
                    continue;
                }
                if normalized_candidate.len() < 7 {
                    continue;
                }
                let exceeds = distances
                    .iter()
                    .zip(word_parts.iter())
                    .zip(candidate_parts.iter())
                    .any(|((d, wp), cp)| *d > if wp.len() > 5 && cp.len() > 4 { 2 } else { 1 });
                if exceeds {
                    continue;
                }
                if total_distance <= max_edit_distance(normalized_candidate, &normalized) {
                    for candidate in candidates {
                        weighted.push((candidate.clone(), total_distance));
                    }
                }
            }
        }

        if weighted.is_empty() {
            return Vec::new();
        }
        weighted.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        let first_weight = weighted[0].1;
        if are_tokens_accepted_by_speller && weighted[0].0.to_uppercase() == original_word {
            return Vec::new();
        }
        if are_tokens_accepted_by_speller && first_weight > 1 {
            return Vec::new();
        }
        let mut results: Vec<String> = Vec::new();
        for (word, weight) in weighted {
            if weight - first_weight < 1 && !results.contains(&word) {
                results.push(word);
            }
        }
        results
    }

    fn discard_run_on_words(&self, underlined_error: &str, dict: &Dictionary) -> bool {
        let parts: Vec<&str> = underlined_error
            .split(' ')
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() != 2 {
            return false;
        }
        if is_capitalized_word(parts[1]) {
            return false;
        }
        if parts[0].is_empty() || parts[1].is_empty() {
            return true;
        }
        let head = parts[0].chars().collect::<Vec<_>>();
        let tail = parts[1].chars().collect::<Vec<_>>();
        let sugg1a: String = head[..head.len() - 1].iter().collect();
        let sugg1b: String = head[head.len() - 1..].iter().chain(tail.iter()).collect();
        if !dict.contains(&sugg1a) && !dict.contains(&sugg1b) {
            return true;
        }
        let sugg2a: String = head.iter().chain(tail[..1].iter()).collect();
        let sugg2b: String = tail[1..].iter().collect();
        !dict.contains(&sugg2a) && !dict.contains(&sugg2b)
    }
}

fn prepare_line_for_speller(line_original: &str) -> Option<String> {
    let stripped = line_original.split('#').next().unwrap_or("").trim();
    if stripped.contains('+') {
        return None;
    }
    let mut parts = stripped.splitn(2, '\t');
    let form = parts.next().unwrap_or("").trim();
    match parts.next() {
        Some(tag) => {
            let tag = tag.trim();
            if tag.starts_with("NN") || tag.starts_with("JJ") {
                Some(form.to_string())
            } else {
                None
            }
        }
        None => Some(stripped.to_string()),
    }
}

fn normalize_key(word: &str) -> String {
    remove_diacritics(&word.to_lowercase()).replace('-', " ")
}

fn is_capitalized_word(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.is_uppercase() => chars.all(|c| !c.is_uppercase()),
        _ => false,
    }
}

fn stop_searching(candidates: &[String], original_word: &str) -> bool {
    for candidate in candidates {
        if candidate == original_word {
            return true;
        }
    }
    for candidate in candidates {
        let lower = candidate.to_lowercase();
        if candidate.as_str() == lower && title_case(&lower) == original_word {
            return true;
        }
    }
    false
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for word in s.split(' ') {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.push_str(&first.to_uppercase().to_string());
            out.push_str(&word[first.len_utf8()..].to_lowercase());
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

fn remove_diacritics(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let replacement: &str = match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => "a",
            'é' | 'è' | 'ê' | 'ë' => "e",
            'í' | 'ì' | 'î' | 'ï' => "i",
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => "o",
            'ú' | 'ù' | 'û' | 'ü' => "u",
            'ñ' => "n",
            'ç' => "c",
            'ý' | 'ÿ' => "y",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(replacement);
    }
    out
}

fn distances_per_word<'a>(
    parts1: &[&'a str],
    parts2: &[&'a str],
    s1: &'a str,
    s2: &'a str,
) -> Vec<i64> {
    if parts1.len() == parts2.len() && parts1.len() > 1 {
        parts1
            .iter()
            .zip(parts2.iter())
            .map(|(a, b)| levenshtein_distance(a, b) as i64)
            .collect()
    } else {
        vec![levenshtein_distance(s1, s2) as i64]
    }
}

fn max_edit_distance(normalized_candidate: &str, normalized_word: &str) -> i64 {
    let total_length = normalized_word.chars().count();
    let correct_length =
        total_length - number_of_correct_chars(normalized_candidate, normalized_word);
    let first_char_wrong: f64 = first_character_distances(normalized_candidate, normalized_word)
        .iter()
        .sum();
    if correct_length <= 7 {
        (2.0 - first_char_wrong) as i64
    } else {
        (2.0 + 0.25 * (correct_length as f64 - 7.0) - 0.6 * first_char_wrong) as i64
    }
}

fn first_character_distances(s1: &str, s2: &str) -> Vec<f64> {
    let parts1: Vec<&str> = s1.split(' ').collect();
    let parts2: Vec<&str> = s2.split(' ').collect();
    if parts1.len() == parts2.len() && parts1.len() == 2 {
        parts1
            .iter()
            .zip(parts2.iter())
            .map(|(a, b)| {
                let ca = a.chars().next().unwrap_or(' ');
                let cb = b.chars().next().unwrap_or(' ');
                char_distance(ca, cb)
            })
            .collect()
    } else {
        vec![0.0]
    }
}

fn char_distance(a: char, b: char) -> f64 {
    if a == b {
        return 0.0;
    }
    if (a == 's' && b == 'z') || (a == 'z' && b == 's') {
        return 0.2;
    }
    if (a == 'b' && b == 'v') || (a == 'v' && b == 'b') {
        return 0.2;
    }
    if (a == 'i' && b == 'y') || (a == 'y' && b == 'i') {
        return 0.0;
    }
    1.0
}

fn number_of_correct_chars(s1: &str, s2: &str) -> usize {
    let parts1: Vec<&str> = s1.split(' ').collect();
    let parts2: Vec<&str> = s2.split(' ').collect();
    let mut correct = 0;
    if parts1.len() == parts2.len() && parts1.len() > 1 {
        for (a, b) in parts1.iter().zip(parts2.iter()) {
            if a == b {
                correct += a.chars().count();
            }
        }
    }
    correct
}

fn is_anagram(s1: &str, s2: &str) -> bool {
    let mut a: Vec<char> = s1.chars().collect();
    let mut b: Vec<char> = s2.chars().collect();
    a.sort_unstable();
    b.sort_unstable();
    a == b
}

fn normalize_similar_chars(s: &str) -> String {
    s.replace('y', "i").replace("ko", "co").replace("ka", "ca")
}

fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    if s1.replace(' ', "") == s2.replace(' ', "") {
        return 0;
    }
    let mut distance = plain_levenshtein(s1, s2);
    let ns1 = normalize_similar_chars(s1);
    let ns2 = normalize_similar_chars(s2);
    if s1 != ns1 || s2 != ns2 {
        distance = distance.min(plain_levenshtein(&ns1, &ns2));
    }
    let anagram = is_anagram(s1, s2);
    if distance > 1 && anagram {
        distance -= 1;
    }
    if distance > 0 && s1.chars().count() == s2.chars().count() && anagram {
        distance = 1;
    }
    distance
}

fn plain_levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        curr[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn support() -> SpellingSupport {
        let mut dict = Dictionary::new();
        for w in [
            "good", "god", "the", "dark", "knight", "rises", "rocky", "mountain",
        ] {
            dict.add_word(w);
        }
        let data = "The Dark Knight Rises\tNNP\nRocky Mountain National Park\tNNP # comment\n";
        SpellingSupport::new(dict, data)
    }

    #[test]
    fn number_in_word_helpers_replace_zero_o() {
        let s = support();
        assert!(!s.is_misspelled("good"));
        assert!(s.is_misspelled("go0d"));
    }

    #[test]
    fn multitoken_suggests_known_phrase() {
        let s = support();
        let got = s.multitoken_suggestions("Thé Dákk Kínght Ríses", false);
        assert!(
            got.iter().any(|c| c == "The Dark Knight Rises"),
            "expected the movie title, got {got:?}"
        );
    }

    #[test]
    fn multitoken_returns_input_unchanged_as_none() {
        let s = support();
        assert!(
            s.multitoken_suggestions("The Dark Knight Rises", false)
                .is_empty()
        );
    }

    #[test]
    fn prepare_line_keeps_noun_and_adjective_tags_only() {
        assert_eq!(
            prepare_line_for_speller("Foo Bar\tNNP"),
            Some("Foo Bar".to_string())
        );
        assert_eq!(prepare_line_for_speller("Foo Bar\tRB"), None);
        assert_eq!(
            prepare_line_for_speller("Plain Global Term"),
            Some("Plain Global Term".to_string())
        );
    }

    #[test]
    fn levenshtein_handles_transpositions_and_similar_chars() {
        assert_eq!(levenshtein_distance("abc", "acb"), 1);
        assert_eq!(levenshtein_distance("knight", "knyght"), 0);
    }
}
