fn main() {
    let dir = std::path::PathBuf::from(
        "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/resource/de",
    );
    let dict = og_tagger::morfologik::MorfologikDictionary::load(
        &dir.join("german.dict"),
        &dir.join("german.info"),
    )
    .expect("dict");
    for w in ["reparier", "abonnier", "bemal", "hoch"] {
        println!("{w}:");
        for r in dict.lookup(w) {
            println!("   {:?}", r);
        }
    }
    for name in ["added.txt", "removed.txt"] {
        if let Ok(content) = std::fs::read_to_string(dir.join(name)) {
            for line in content.lines() {
                if line.contains("reparier") || line.contains("abonnier") {
                    println!("{name}: {line}");
                }
            }
        }
    }
}
