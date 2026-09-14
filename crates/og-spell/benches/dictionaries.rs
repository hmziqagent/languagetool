//! Dictionary lookup / suggestion benchmarks (PLAN Phase 12, plans/08).
//!
//! Run with: `cargo bench -p og-spell` (harness = false; std-only runner —
//! no criterion dependency, works fully offline).
//!
//! Data-backed benches load the real English Morfologik-decoded dictionary
//! and the real German igerman98 Hunspell dictionary through the configurable
//! LT resource root and print a SKIP line (still exit 0) when the checkout is
//! absent, mirroring the data-backed test convention.

use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use og_spell::resource::lt_resource_root;
use og_spell::{Dictionary, HunspellDictionary, SuggestionEngine};

fn bench(name: &str, iters: usize, mut f: impl FnMut()) {
    f(); // warmup
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    let elapsed = start.elapsed();
    println!(
        "{:<58} {:>14.1} ns/op  ({} iters, {:?} total)",
        name,
        elapsed.as_nanos() as f64 / iters as f64,
        iters,
        elapsed
    );
}

fn en_dict_path(root: &PathBuf) -> PathBuf {
    // The generated Morfologik dict_decoded.txt is absent from the frozen
    // checkout (create_dict.sh output); the English engine falls back to
    // the hunspell word list, so bench exactly that.
    root.join("en/src/main/resources/org/languagetool/resource/en/hunspell/spelling.txt")
}

fn de_dic_path(root: &PathBuf) -> PathBuf {
    root.join("de/src/main/resources/org/languagetool/resource/de/hunspell/de_DE.dic")
}

fn de_aff_path(root: &PathBuf) -> PathBuf {
    root.join("de/src/main/resources/org/languagetool/resource/de/hunspell/de_DE.aff")
}

fn main() {
    // English word-list dictionary (HashSet<String> based, plans/08 gap:
    // "no dictionary memory optimization").
    let root = lt_resource_root();
    let dict = match &root {
        Some(r) if en_dict_path(r).exists() => {
            let t = Instant::now();
            let d = Dictionary::from_file(&en_dict_path(r)).expect("open spelling.txt");
            println!(
                "# en Dictionary::from_file (hunspell/spelling.txt) load: {:?} for {} entries",
                t.elapsed(),
                d.len()
            );
            Some(d)
        }
        _ => {
            println!("# SKIP en dictionary benches (no LT resource root)");
            None
        }
    };

    if let Some(dict) = &dict {
        let words = dict.word_list();
        let known = words
            .iter()
            .find(|w| w.len() > 5 && w.chars().all(|c| c.is_ascii_alphabetic()))
            .cloned()
            .unwrap_or_else(|| "language".to_string());
        bench("Dictionary::contains (hit, exact)", 1_000_000, || {
            black_box(dict.contains(black_box(&known)));
        });
        bench("Dictionary::contains (miss)", 1_000_000, || {
            black_box(dict.contains(black_box("zzqqxxyyww")));
        });

        // Suggestion search is a full-dictionary Levenshtein scan bounded by
        // a length filter (plans/08 gap: not frequency-indexed / BK-tree).
        let suggester = SuggestionEngine::new(dict);
        bench("SuggestionEngine::suggest (max 5)", 20, || {
            black_box(suggester.suggest(black_box("languaeg"), 5));
        });
    }

    // German Hunspell dictionary (affix analysis + compounds).
    let hunspell = match &root {
        Some(r) if de_dic_path(r).exists() && de_aff_path(r).exists() => {
            let t = Instant::now();
            let h = HunspellDictionary::open(&de_dic_path(r), &de_aff_path(r)).expect("open de_DE");
            println!(
                "# de HunspellDictionary::open load: {:?} for {} entries",
                t.elapsed(),
                h.len()
            );
            Some(h)
        }
        _ => {
            println!("# SKIP hunspell benches (no LT resource root)");
            None
        }
    };

    if let Some(h) = &hunspell {
        bench("Hunspell is_known (dic stem hit)", 100_000, || {
            black_box(h.is_known(black_box("Haus")));
        });
        bench("Hunspell is_known (affixed form)", 100_000, || {
            black_box(h.is_known(black_box("Haustür")));
        });
        bench("Hunspell is_known (novel compound)", 10_000, || {
            black_box(h.is_known(black_box("Abiturientinnenecke")));
        });
        bench("Hunspell is_known (miss)", 100_000, || {
            black_box(h.is_known(black_box("zzqqxxyyww")));
        });
    }
}
