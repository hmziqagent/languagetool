//! Process-wide cache of compiled regexes.
//!
//! Compiling the real LT grammar produces thousands of `regex::Regex` NFAs,
//! many of them duplicated across rules (and across every engine build in the
//! same process). Building each pattern independently costs seconds of CPU and
//! multiple GB of resident memory in debug builds — enough to OOM test
//! runners that construct several engines.
//!
//! This module dedups compilation: `(pattern, case_insensitive)` is compiled
//! at most once per process and shared as an `Arc`. Failed compilations are
//! cached too (as `None`) so a broken pattern is not retried per token.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use regex::{Regex, RegexBuilder};

type CacheKey = (Box<str>, bool);

static CACHE: LazyLock<Mutex<HashMap<CacheKey, Option<Arc<Regex>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Compile `pattern`, returning a shared handle from the process-wide cache.
///
/// `case_insensitive` mirrors `regex::RegexBuilder::case_insensitive`.
/// Returns `None` when the pattern fails to compile (same as the previous
/// eager `Regex::new(..).ok()` behavior).
pub fn compile(pattern: &str, case_insensitive: bool) -> Option<Arc<Regex>> {
    let key: CacheKey = (pattern.into(), case_insensitive);
    if let Some(hit) = CACHE.lock().unwrap().get(&key) {
        return hit.clone();
    }
    let built = RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .ok()
        .map(Arc::new);
    CACHE.lock().unwrap().insert(key, built.clone());
    built
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dedups_and_shares() {
        let a = compile("^a+$", false).unwrap();
        let b = compile("^a+$", false).unwrap();
        assert!(Arc::ptr_eq(&a, &b));

        let ci = compile("^a+$", true).unwrap();
        assert!(!Arc::ptr_eq(&a, &ci));
        assert!(ci.is_match("AAA"));
        assert!(!a.is_match("AAA"));
    }

    #[test]
    fn invalid_pattern_is_cached_as_none() {
        assert!(compile("(unclosed", false).is_none());
        assert!(compile("(unclosed", false).is_none());
    }
}
