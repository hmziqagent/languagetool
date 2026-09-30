use og_tagger::Tagger;

fn main() {
    let dir = std::path::Path::new(
        "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/resource/de",
    );
    let tagger = og_tagger::GermanTagger::from_resource_dir(dir).expect("tagger");
    for w in [
        "reparier",
        "abonnier",
        "bemal",
        "hoch",
        "rüber",
        "Kurz",
        "Anfangen",
        "herumzustehen",
    ] {
        let tagged = Tagger::tag(&tagger, &[w]);
        println!("{w}:");
        for r in tagged[0].readings() {
            println!("   {} {}", r.lemma().unwrap_or("-"), r.pos_tags().join(","));
        }
    }
}
