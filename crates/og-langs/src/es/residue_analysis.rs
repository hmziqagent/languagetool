use crate::es::test_support::SpanishPipeline;
use std::collections::BTreeMap;

#[test]
#[ignore]
fn classify_failures_by_dict_coverage() {
    let artifact =
        std::fs::read_to_string("/tmp/test_failures_es_module.txt").expect("failure artifact");
    let pipeline = SpanishPipeline::shared();
    let mut buckets: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut dict_blocked = 0usize;
    let mut genuine = 0usize;
    let mut genuine_lines: Vec<String> = Vec::new();
    for line in artifact.lines() {
        let Some(rest) = line.strip_prefix("FAIL: [") else {
            continue;
        };
        let Some((rule, rest)) = rest.split_once(']') else {
            continue;
        };
        let text = rest
            .split_once(": '")
            .map(|(_, t)| t.trim_end_matches('\''))
            .unwrap_or("");
        let mut missing: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut words: Vec<String> = Vec::new();
        for c in text.chars() {
            if c.is_alphanumeric() || c == '\'' || c == '-' || c == '’' {
                cur.push(c);
            } else if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            words.push(cur);
        }
        for w in &words {
            if w.chars().any(|c| c.is_alphabetic()) && !pipeline.tagger_ref().is_word_tagged(w) {
                missing.push(w.clone());
            }
        }
        let entry = buckets.entry(rule.to_string()).or_insert((0, 0));
        if missing.is_empty() {
            genuine += 1;
            entry.0 += 1;
            genuine_lines.push(format!("{line} || missing: none"));
        } else {
            dict_blocked += 1;
            entry.1 += 1;
            genuine_lines.push(format!("{line} || missing: {missing:?}"));
        }
    }
    eprintln!("genuine (all words tagged): {genuine}, dict-blocked: {dict_blocked}");
    eprintln!("per-rule (genuine, dict-blocked):");
    for (rule, (g, d)) in &buckets {
        eprintln!("  {rule}: {g}/{d}");
    }
    eprintln!("--- all lines with missing words ---");
    for l in &genuine_lines {
        if l.contains("|| missing: [") {
            eprintln!("{l}");
        }
    }
}

#[test]
#[ignore]
fn compare_failing_word_readings_across_dicts() {
    use og_tagger::morfologik::MorfologikDictionary;
    use std::path::PathBuf;
    let artifact =
        std::fs::read_to_string("/tmp/test_failures_es_module.txt").expect("failure artifact");
    let on_disk_dir = crate::es::es_resource_dir().expect("es dir");
    let on_disk = MorfologikDictionary::load(
        &on_disk_dir.join("spanish.dict"),
        &on_disk_dir.join("spanish.info"),
    )
    .expect("on-disk dict");
    let Some(fetched) = MorfologikDictionary::load(
        &PathBuf::from("/tmp/current_spanish.dict"),
        &PathBuf::from("/tmp/current_spanish.info"),
    ) else {
        eprintln!("no fetched dict; skipping");
        return;
    };
    let mut words: Vec<String> = Vec::new();
    for line in artifact.lines() {
        let Some(rest) = line.strip_prefix("FAIL: [") else {
            continue;
        };
        let Some((_, rest)) = rest.split_once(']') else {
            continue;
        };
        let text = rest
            .split_once(": '")
            .map(|(_, t)| t.trim_end_matches('\''))
            .unwrap_or("");
        let mut cur = String::new();
        for c in text.chars() {
            if c.is_alphanumeric() || c == '\'' || c == '-' || c == '’' {
                cur.push(c);
            } else if !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            words.push(cur);
        }
    }
    words.sort();
    words.dedup();
    let mut same = 0usize;
    let mut differ: Vec<String> = Vec::new();
    for w in &words {
        if !w.chars().any(|c| c.is_alphabetic()) {
            continue;
        }
        let a = on_disk.lookup(w);
        let b = fetched.lookup(w);
        if a == b {
            same += 1;
        } else {
            differ.push(w.clone());
            eprintln!("DIFF {w}: old={:?} fetched={:?}", a, b);
        }
    }
    eprintln!(
        "words: {} same readings, {} differing, ({} total)",
        same,
        differ.len(),
        words.len()
    );
}
