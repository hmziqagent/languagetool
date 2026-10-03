use og_tagger::morfologik::MorfologikDictionary;
use std::path::PathBuf;

#[test]
#[ignore]
fn compare_failing_words_across_spanish_dicts() {
    let resource_dir = crate::es::es_resource_dir().expect("es resource dir");
    let on_disk = MorfologikDictionary::load(
        &resource_dir.join("spanish.dict"),
        &resource_dir.join("spanish.info"),
    )
    .expect("on-disk spanish.dict");
    let old = MorfologikDictionary::load(
        &PathBuf::from("/tmp/old_spanish.dict"),
        &PathBuf::from("/tmp/old_spanish.info"),
    );
    let fetched = MorfologikDictionary::load(
        &PathBuf::from("/tmp/current_spanish.dict"),
        &PathBuf::from("/tmp/current_spanish.info"),
    );
    if old.is_none() {
        eprintln!("no /tmp/old_spanish.dict; skipping comparison");
        return;
    }
    let old = old.unwrap();

    let words = std::fs::read_to_string("/tmp/es_fail_words.txt").expect("word list");
    let mut only_old = 0usize;
    let mut only_new = 0usize;
    let mut in_both = 0usize;
    let mut in_neither = 0usize;
    let mut old_better: Vec<String> = Vec::new();
    let mut new_better: Vec<String> = Vec::new();
    let mut neither: Vec<String> = Vec::new();
    for w in words.lines() {
        let w = w.trim();
        if w.is_empty() {
            continue;
        }
        let lower = w.to_lowercase();
        let has = |d: &MorfologikDictionary| {
            let cap = {
                let mut chars = lower.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            };
            !d.lookup(w).is_empty()
                || (!lower.eq(w) && !d.lookup(&lower).is_empty())
                || (w.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
                    && !d.lookup(&cap).is_empty())
        };
        let in_old = has(&old);
        let in_new = has(&on_disk);
        match (in_old, in_new) {
            (true, true) => in_both += 1,
            (true, false) => {
                only_old += 1;
                old_better.push(w.to_string());
            }
            (false, true) => {
                only_new += 1;
                new_better.push(w.to_string());
            }
            (false, false) => {
                in_neither += 1;
                neither.push(w.to_string());
            }
        }
    }
    eprintln!(
        "both: {in_both}, only-old: {only_old}, only-fetched: {only_new}, neither: {in_neither}"
    );
    eprintln!("words only in last-tracked 1MB dict: {old_better:?}");
    eprintln!("words only in fetched dict: {new_better:?}");
    eprintln!("words in neither dict: {neither:?}");
    for w in ["del", "al", "dos", "joven", "casa", "podés", "Jorge"] {
        let fmt = |d: &MorfologikDictionary| {
            d.lookup(w)
                .iter()
                .map(|(l, t)| format!("{l}/{t}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let fetched_fmt = fetched
            .as_ref()
            .map(|d| format!("[{}]", fmt(d)))
            .unwrap_or_else(|| "ABSENT".to_string());
        eprintln!(
            "{w}: last-tracked=[{}] fetched460k={fetched_fmt}",
            fmt(&old)
        );
    }
}
