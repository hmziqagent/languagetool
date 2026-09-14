use crate::dictionary::Dictionary;

pub struct SuggestionEngine {
    words: Vec<String>,
}

impl SuggestionEngine {
    pub fn new(dictionary: &Dictionary) -> Self {
        let words: Vec<String> = dictionary.word_list();
        Self { words }
    }

    pub fn suggest(&self, word: &str, max: usize) -> Vec<String> {
        let lower = word.to_lowercase();
        let is_capitalized = word
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
            && word
                .chars()
                .skip(1)
                .all(|c| !c.is_uppercase() || !c.is_alphabetic());
        let is_all_upper = word
            .chars()
            .filter(|c| c.is_alphabetic())
            .all(|c| c.is_uppercase());

        let word_len = lower.len();
        let max_dist = if word_len <= 4 { 2 } else { 3 };

        let first = lower.chars().next();
        let mut scored: Vec<(String, usize)> = self
            .words
            .iter()
            .filter(|w| {
                let wlen = w.len();
                // Only check words within plausible edit distance range
                wlen > 1 && wlen.abs_diff(word_len) <= max_dist
            })
            .filter_map(|w| {
                let dist = levenshtein_distance(&lower, w, max_dist);
                if dist <= max_dist && dist > 0 {
                    Some((w.clone(), dist))
                } else {
                    None
                }
            })
            .collect();

        // Deterministic ranking: nearest first, then same-initial-letter,
        // then closest length, then alphabetical. (There is no frequency
        // corpus in the upstream checkout, so frequency ranking is out of
        // scope; see docs/plans/compatibility-matrix.md.)
        scored.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| {
                    let a_match = a.0.chars().next() == first;
                    let b_match = b.0.chars().next() == first;
                    b_match.cmp(&a_match)
                })
                .then_with(|| a.0.len().cmp(&b.0.len()))
                .then_with(|| a.0.cmp(&b.0))
        });

        scored
            .into_iter()
            .take(max)
            .map(|(w, _)| {
                if is_all_upper {
                    w.to_uppercase()
                } else if is_capitalized {
                    let mut chars = w.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                        None => w,
                    }
                } else {
                    w
                }
            })
            .collect()
    }
}

/// Levenshtein distance with a rolling two-row DP and early exit: returns
/// `max + 1` as soon as the distance provably exceeds `max`.
fn levenshtein_distance(a: &str, b: &str, max: usize) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }
    if a_len.abs_diff(b_len) > max {
        return max + 1;
    }

    let mut prev: Vec<usize> = (0..=b_len).collect();
    let mut curr = vec![0usize; b_len + 1];

    for i in 1..=a_len {
        curr[0] = i;
        let mut row_min = curr[0];
        for j in 1..=b_len {
            let cost = usize::from(a_chars[i - 1] != b_chars[j - 1]);
            curr[j] = std::cmp::min(
                std::cmp::min(prev[j] + 1, curr[j - 1] + 1),
                prev[j - 1] + cost,
            );
            row_min = row_min.min(curr[j]);
        }
        if row_min > max {
            return max + 1;
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[b_len]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein_distance("hello", "hello", 4), 0);
        assert_eq!(levenshtein_distance("hello", "helo", 4), 1);
        assert_eq!(levenshtein_distance("hello", "hallo", 4), 1);
        assert_eq!(levenshtein_distance("hello", "world", 4), 4);
    }

    #[test]
    fn test_suggest() {
        let dict = Dictionary::from_words(&["hello", "world", "help", "held", "hell", "helm"]);
        let engine = SuggestionEngine::new(&dict);
        let suggestions = engine.suggest("helo", 5);
        // All words with distance <= 1 should be suggested: hello, help, held, hell
        assert!(
            suggestions.contains(&"hello".to_string()) || suggestions.contains(&"help".to_string()),
            "Expected at least one close match, got: {:?}",
            suggestions
        );
        // Verify close matches have priority
        assert!(
            suggestions
                .iter()
                .take(3)
                .all(|s| levenshtein_distance("helo", s, 3) <= 1)
        );
    }

    #[test]
    fn test_suggestions_are_deterministic() {
        let dict = Dictionary::from_words(&["hell", "helm", "help", "held"]);
        let engine = SuggestionEngine::new(&dict);
        let a = engine.suggest("helo", 4);
        let b = engine.suggest("helo", 4);
        assert_eq!(a, b);
        // All candidates are distance 1; ranking must be total.
        assert_eq!(a.len(), 4);
    }

    #[test]
    fn test_levenshtein_early_exit() {
        assert!(levenshtein_distance("aaaaaaaaaa", "bbbbbbbbbb", 3) > 3);
        assert_eq!(levenshtein_distance("cat", "caat", 1), 1);
    }
}
