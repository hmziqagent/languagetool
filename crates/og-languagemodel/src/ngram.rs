use std::collections::HashMap;
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Probability {
    pub prob: f64,
    pub coverage: f32,
    pub total_count: u64,
}

pub trait LanguageModel: Send + Sync {
    fn get_count(&self, tokens: &[&str]) -> u64;
    fn get_total_token_count(&self) -> u64;
    fn get_probability(&self, context: &[&str]) -> Probability;
}

impl LanguageModel for Arc<dyn LanguageModel> {
    fn get_probability(&self, context: &[&str]) -> Probability {
        (**self).get_probability(context)
    }

    fn get_count(&self, tokens: &[&str]) -> u64 {
        (**self).get_count(tokens)
    }

    fn get_total_token_count(&self) -> u64 {
        (**self).get_total_token_count()
    }
}

pub fn get_pseudo_probability_from_ref(model: &dyn LanguageModel, context: &[&str]) -> Probability {
    let total_token_count = model.get_total_token_count();
    let mut max_coverage = 0usize;
    let mut coverage = 0usize;

    let Some(&first) = context.first() else {
        return Probability {
            prob: 0.0,
            coverage: 0.0,
            total_count: 0,
        };
    };
    let first_word_count = model.get_count(&[first]);
    max_coverage += 1;
    if first_word_count > 0 {
        coverage += 1;
    }
    let mut p = (first_word_count as f64 + 1.0) / (total_token_count as f64 + 1.0);

    let mut total_count = 0u64;
    for i in 2..=context.len() {
        let sub = &context[..i];
        let phrase_count = model.get_count(sub);
        if sub.len() == 3 {
            total_count = phrase_count;
        }
        let this_p = (phrase_count as f64 + 1.0) / (first_word_count as f64 + 1.0);
        max_coverage += 1;
        if phrase_count > 0 {
            coverage += 1;
        }
        p *= this_p;
    }
    Probability {
        prob: p,
        coverage: if max_coverage == 0 {
            0.0
        } else {
            coverage as f32 / max_coverage as f32
        },
        total_count,
    }
}

pub struct DiskLanguageModel {
    grams: [GramStore; 4],
    total_token_count: u64,
}

enum GramStore {
    Map(HashMap<Box<str>, u64>),
    SortedTsv(SortedTsvLookup),
}

impl GramStore {
    fn get(&self, key: &str) -> Option<u64> {
        match self {
            GramStore::Map(map) => map.get(key).copied(),
            GramStore::SortedTsv(tsv) => tsv.get(key),
        }
    }
}

pub struct SortedTsvLookup {
    file: fs::File,
    len: u64,
}

impl SortedTsvLookup {
    fn open(path: &Path) -> Option<Self> {
        let file = fs::File::open(path).ok()?;
        let len = file.metadata().ok()?.len();
        if len == 0 {
            return None;
        }
        Some(Self { file, len })
    }

    fn read_line_at(&self, mut pos: u64) -> Option<String> {
        use std::io::{Seek, SeekFrom};
        let mut reader = std::io::BufReader::new(&self.file);
        reader.seek(SeekFrom::Start(pos)).ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        if line.is_empty() && pos < self.len {
            pos += 1;
            reader.seek(SeekFrom::Start(pos)).ok()?;
            line.clear();
            reader.read_line(&mut line).ok()?;
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed.is_empty() {
            return Some(String::new());
        }
        Some(trimmed.to_string())
    }

    pub fn get(&self, key: &str) -> Option<u64> {
        let first = self.read_line_at(0)?;
        if first.is_empty() {
            return None;
        }
        match compare_line(&first, key) {
            Some((std::cmp::Ordering::Equal, count)) => return Some(count),
            Some((std::cmp::Ordering::Greater, _)) => return None,
            _ => {}
        }
        let mut lo = 0u64;
        let mut hi = self.len;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if mid <= lo {
                break;
            }
            let ls = self.line_start_of(mid);
            if ls >= hi {
                break;
            }
            let line = self.read_line_at(ls)?;
            if line.is_empty() {
                break;
            }
            match compare_line(&line, key) {
                Some((std::cmp::Ordering::Equal, count)) => return Some(count),
                Some((std::cmp::Ordering::Less, _)) => {
                    lo = ls + line.len() as u64 + 1;
                }
                Some((std::cmp::Ordering::Greater, _)) => hi = ls,
                None => break,
            }
        }
        None
    }

    fn line_start_of(&self, pos: u64) -> u64 {
        use std::io::{Read, Seek, SeekFrom};
        if pos == 0 {
            return 0;
        }
        let mut reader = std::io::BufReader::with_capacity(8192, &self.file);
        if reader.seek(SeekFrom::Start(pos - 1)).is_err() {
            return pos;
        }
        let window = 512u64;
        let base = pos.saturating_sub(window);
        let mut buf = Vec::new();
        if reader.seek(SeekFrom::Start(base)).is_err() {
            return pos;
        }
        if reader.take(pos - base).read_to_end(&mut buf).is_err() {
            return pos;
        }
        match buf.iter().rposition(|b| *b == b'\n') {
            Some(idx) => base + idx as u64 + 1,
            None => {
                if base == 0 {
                    0
                } else {
                    self.line_start_of(base)
                }
            }
        }
    }
}

fn compare_line(line: &str, key: &str) -> Option<(std::cmp::Ordering, u64)> {
    let (term, count) = line.split_once('\t')?;
    let count = count.trim().parse::<u64>().ok()?;
    Some((term.as_bytes().cmp(key.as_bytes()), count))
}

impl DiskLanguageModel {
    pub fn from_dir(dir: &Path) -> Option<Self> {
        let mut grams: [Option<SortedTsvLookup>; 4] = Default::default();
        let mut total_token_count: Option<u64> = None;
        let mut found_any = false;
        for n in 1..=3usize {
            let path: PathBuf = dir.join(format!("{n}grams.tsv"));
            if !path.exists() {
                continue;
            }
            let Some(lookup) = SortedTsvLookup::open(&path) else {
                continue;
            };
            found_any = true;
            grams[n] = Some(lookup);
            if n == 1 {
                let meta = fs::read_to_string(dir.join(format!("{n}grams.meta")))
                    .ok()
                    .and_then(|m| {
                        m.lines().find_map(|l| {
                            l.strip_prefix("total_token_count\t")
                                .and_then(|v| v.trim().parse::<u64>().ok())
                        })
                    });
                if let Some(total) = meta {
                    total_token_count = Some(total);
                }
            }
        }
        if !found_any {
            return None;
        }
        Some(Self {
            grams: [
                GramStore::Map(HashMap::new()),
                grams[1]
                    .take()
                    .map(GramStore::SortedTsv)
                    .unwrap_or(GramStore::Map(HashMap::new())),
                grams[2]
                    .take()
                    .map(GramStore::SortedTsv)
                    .unwrap_or(GramStore::Map(HashMap::new())),
                grams[3]
                    .take()
                    .map(GramStore::SortedTsv)
                    .unwrap_or(GramStore::Map(HashMap::new())),
            ],
            total_token_count: total_token_count.unwrap_or(1).max(1),
        })
    }

    pub fn shared_for_language(lang_code: &str) -> Option<Arc<Self>> {
        static CACHE: std::sync::OnceLock<
            std::sync::Mutex<HashMap<String, Option<Arc<DiskLanguageModel>>>>,
        > = std::sync::OnceLock::new();
        let cache = CACHE.get_or_init(Default::default);
        let mut guard = cache.lock().ok()?;
        if let Some(hit) = guard.get(lang_code) {
            return hit.clone();
        }
        let loaded = find_ngram_dir().and_then(|root| {
            let candidates = [root.join("extracted").join(lang_code), root.join(lang_code)];
            candidates
                .iter()
                .find_map(|dir| Self::from_dir(dir))
                .map(Arc::new)
        });
        guard.insert(lang_code.to_string(), loaded.clone());
        loaded
    }

    pub fn from_pairs(entries: &[(Vec<&str>, u64)]) -> Self {
        let mut maps: [HashMap<Box<str>, u64>; 4] = Default::default();
        let mut total_token_count = 0u64;
        for (tokens, count) in entries {
            let n = tokens.len().min(4);
            if n == 0 {
                continue;
            }
            let key: Box<str> = tokens.join(" ").into();
            maps[n].insert(key, *count);
            if n == 1 {
                total_token_count += count;
            }
        }
        Self {
            grams: [
                GramStore::Map(maps[0].clone()),
                GramStore::Map(maps[1].clone()),
                GramStore::Map(maps[2].clone()),
                GramStore::Map(maps[3].clone()),
            ],
            total_token_count,
        }
    }
}

impl LanguageModel for DiskLanguageModel {
    fn get_probability(&self, context: &[&str]) -> Probability {
        get_pseudo_probability_from_ref(self, context)
    }

    fn get_count(&self, tokens: &[&str]) -> u64 {
        if tokens.is_empty() || tokens.len() > 4 {
            return 0;
        }
        let key: String = tokens.join(" ");
        self.grams[tokens.len()].get(&key).unwrap_or(0)
    }

    fn get_total_token_count(&self) -> u64 {
        self.total_token_count
    }
}

fn find_ngram_dir() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("ngram-data"),
        PathBuf::from("../ngram-data"),
        PathBuf::from("/home/agent/opengrammar-rs/ngram-data"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

pub type SharedModel = Arc<dyn LanguageModel>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_rule_matches_java_pseudo_probability() {
        let model = DiskLanguageModel::from_pairs(&[
            (vec!["the"], 1000),
            (vec!["quick"], 200),
            (vec!["the", "quick"], 50),
            (vec!["the", "quick", "fox"], 10),
        ]);
        let p = model.get_probability(&["the", "quick", "fox"]);
        assert!((p.prob - (1001.0 / 1201.0) * (51.0 / 1001.0) * (11.0 / 1001.0)).abs() < 1e-12);
        assert_eq!(p.coverage, 1.0);
        assert_eq!(p.total_count, 10);
    }

    #[test]
    fn german_extracted_index_lookups() {
        let dir = PathBuf::from("/home/agent/opengrammar-rs/ngram-data/extracted/de");
        if !dir.join("1grams.tsv").exists() {
            eprintln!("Skipping: extracted German ngram data not present");
            return;
        }
        let Some(model) = DiskLanguageModel::from_dir(&dir) else {
            panic!("model must load from extracted dir");
        };
        assert_eq!(model.get_count(&["der"]), 1446429476);
        assert!(model.get_count(&["der", "Haus"]) > 0, "2gram lookup");
        assert!(
            model.get_count(&["Berlin", "der", "Stadt"]) > 0,
            "3gram lookup"
        );
        assert_eq!(model.get_total_token_count(), 2057002144632);
        let prob = model.get_probability(&["der", "Haus"]);
        assert!(prob.prob > 0.0 && prob.prob < 1.0);
    }

    #[test]
    fn shared_model_resolves_extracted_dirs_per_language() {
        for lang in ["en", "de"] {
            let dir = PathBuf::from("/home/agent/opengrammar-rs/ngram-data/extracted").join(lang);
            if !dir.join("1grams.tsv").exists() {
                eprintln!("Skipping {lang}: extracted ngram data not present");
                continue;
            }
            let model = DiskLanguageModel::shared_for_language(lang).unwrap_or_else(|| {
                panic!("shared model for {lang} must resolve the extracted dir")
            });
            assert!(model.get_total_token_count() > 0);
        }
    }

    #[test]
    fn english_extracted_index_lookups() {
        let dir = PathBuf::from("/home/agent/opengrammar-rs/ngram-data/extracted/en");
        if !dir.join("2grams.tsv").exists() {
            eprintln!("Skipping: extracted English ngram data not present");
            return;
        }
        let Some(model) = DiskLanguageModel::from_dir(&dir) else {
            panic!("model must load from extracted en dir");
        };
        assert_eq!(model.get_count(&["the"]), 19966238696);
        assert_eq!(model.get_count(&["in", "the"]), 1759394592);
        assert_eq!(model.get_count(&["of", "the"]), 3385103919);
        assert!(
            model.get_count(&["in", "the", "morning"]) > 0,
            "3gram lookup"
        );
    }

    #[test]
    fn unknown_words_get_smoothed_probability() {
        let model = DiskLanguageModel::from_pairs(&[(vec!["the"], 100)]);
        let p = model.get_probability(&["zebra"]);
        assert!((p.prob - 1.0 / 101.0).abs() < 1e-12);
        assert_eq!(p.coverage, 0.0);
    }
}
