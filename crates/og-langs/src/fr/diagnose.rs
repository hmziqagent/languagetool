#![cfg(test)]

use crate::fr::example_suite::build_pattern_engine;
use crate::fr::test_support::FrenchPipeline;
use og_tokenizer::WordTokenizer as _;
use og_xml::compiler::XmlCompiler;

fn tokenizer_probe() {
    let pipeline = FrenchPipeline::shared();
    for text in [
        "Il est né le 32.1.1985.",
        "Il est né le 31.4.1989.",
        "Une croissance de 8%.",
        "Une croissance de 8,5%.",
        "-il est arrivé à l'hôtel hier.",
        "D‘enfants.",
        "Vas -tu les regarder à la télévision ?",
        "S’il vous -plaît, dessine-moi un mouton.",
        "S’il-vous plaît, dessine-moi un mouton.",
        "J'utilise google.com.",
        "La surface du mur ne s'abîme.",
        "The pia- nist",
        "alors il faut partir",
        "dans ce cas il est certain",
        "Le néo-impressionnisme",
        "Miguel Diaz-Canel est porté sur les fonts néo-post-castristes.",
        "Pour revenir au post précédent.",
    ] {
        let tokens: Vec<String> = pipeline
            .tokenizer_ref()
            .tokenize(text)
            .iter()
            .map(|t| format!("{:?}", t.text()))
            .collect();
        println!("TOKENS {text:?} => {}", tokens.join(" "));
    }
}

fn tagger_probe() {
    let tagged = og_tagger::Tagger::tag(
        FrenchPipeline::shared().tagger_ref(),
        &[
            "du", "savoir", "dire", "lever", "repentir", "lancer", "voies",
        ],
    );
    for atr in tagged {
        let readings: Vec<String> = atr
            .readings()
            .iter()
            .map(|r| format!("{}/{}", r.lemma().unwrap_or("null"), r.pos_tags().join(" ")))
            .collect();
        println!("TAG {:?} => {}", atr.token().token(), readings.join(" | "));
    }
}

fn readings_probe() {
    let pipeline = FrenchPipeline::shared();
    for text in [
        "Je n’aime guerre le sport.",
        "Il n’y en a guerre.",
        "C'est lui qui ce cache sous la table.",
        "Il a décidé de ce lever tôt.",
        "Il s’en suivit de grands troubles.",
        "Le développement qui s’en est suivi fut remarquable.",
        "J’ai pas compris.",
        "Je ne sé compris.",
        "C’est arrosoir contient 15 litres.",
        "Qu’est ce que tu dis ?",
        "Demander jusqu’à ce qu’il me répond.",
        "J’aimerai bien travailler ici.",
        "C'est trop, tu n'aurais pas du savoir !",
        "Voici la piste qu'aurait négligé les enqêteurs.",
        "Il devient de plus en plus.",
        "La défaite s’en suivit.",
        "Il le fait poursoi.",
        "Il se peut qu’il soi en retard.",
        "Dominique peux venir.",
        "Nous l'avons mangés.",
        "Un orage semble annoncée.",
        "Il parle d’avantage de toi.",
        "Quand j'étais petit, je jouais au football.",
        "Je suis d'abord allé au laboratoire.",
    ] {
        println!("READINGS {text:?}");
        for sentence in pipeline.analyze_text(text) {
            for t in sentence.tokens() {
                if t.is_whitespace() || t.token().token() == "<S>" {
                    continue;
                }
                let readings: Vec<String> = t
                    .readings()
                    .iter()
                    .map(|r| format!("{}/{}", r.lemma().unwrap_or("null"), r.pos_tags().join(" ")))
                    .collect();
                let chunks = t.chunks().join(",");
                println!(
                    "  {:?} [{}..{}] = {} | chunks: {}",
                    t.token().token(),
                    t.token().start(),
                    t.token().end(),
                    readings.join(" | "),
                    chunks
                );
            }
        }
    }
}

fn rule_match_probe() {
    let xml = std::fs::read_to_string(
        crate::fr::test_support::fr_rules_dir()
            .expect("fr rules dir")
            .join("grammar.xml"),
    )
    .expect("grammar.xml");
    let rule_set = XmlCompiler::new()
        .compile_file_with_base(&xml, crate::fr::test_support::fr_rules_dir().as_deref())
        .expect("compile grammar.xml");
    let engine = build_pattern_engine();
    let pipeline = FrenchPipeline::shared();
    og_rules::pattern_rule::set_test_date_hack(true);

    let probes: &[(&str, &str)] = &[
        ("DATE", "Il est né le 32.1.1985."),
        ("DATE", "Elle est née le 29.2.1900."),
        ("ESPACE_POURCENT", "Une croissance de 8%."),
        ("ALORS", "alors il faut partir"),
        ("DANS_CE_CAS", "dans ce cas il est certain"),
        ("FR_SPLIT_WORDS_HYPHEN", "The pia- nist"),
        ("PREFIXES_NON_SEPARES", "Pour revenir au post précédent."),
        ("CE_SE", "C'est lui qui ce cache sous la table."),
        ("GUERE_GUERRE", "Je n’aime guerre le sport."),
        ("LA_LA2", "La personne photographiée est à la fois :"),
        (
            "OU",
            "Je vous écrirai ou vous téléphonerai la semaine prochaine.",
        ),
        ("PAS_DE_TRAIT_UNION", "Le néo-impressionnisme"),
        ("CONF_N_V", "J'utilise google.com."),
        ("APOS_ESPACE", "La surface du mur ne s'abîme."),
        ("MOTS_MAUX", "Antidote, le remède à tous vos mots."),
        ("MOTS_MAUX", "Docteur délivrez moi de mes mots."),
        ("DU_DU", "C'est trop, tu n'aurais pas du savoir !"),
        ("CE_SE", "Il a décidé de ce lever tôt."),
        ("ACCORD_SUJET_VERBE", "Dominique peux venir."),
        ("ACCORD_SUJET_VERBE", "L'enfant dis merci."),
        ("ETRE_VPPA_OU_ADJ", "Un orage semble annoncée."),
        ("AUX_AVOIR", "Nous l'avons mangés."),
    ];
    for (id, text) in probes {
        println!("MATCH {id} on {text:?}");
        let mut any = false;
        for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
            for rule in &rule_set.rules {
                if rule.id != *id {
                    continue;
                }
                let fr_filter = rule.filter.as_ref().filter(|f| {
                    f.class == crate::fr::suppress_misspelled::FRENCH_SUPPRESS_MISSPELLED_FILTER_CLASS
                });
                for m in engine.match_rule(rule, &sentence) {
                    let Some(m) = (match fr_filter {
                        Some(filter) => {
                            crate::fr::suppress_misspelled::accept_rule_match(m, &filter.args, &[])
                        }
                        None => Some(m),
                    }) else {
                        continue;
                    };
                    any = true;
                    println!(
                        "  sub {:?} => {}..{} msg {:?} sugg {:?}",
                        rule.sub_id,
                        m.offset() + sentence_start,
                        m.offset() + sentence_start + m.length(),
                        m.message(),
                        m.replacements()
                            .iter()
                            .map(|r| r.value().to_string())
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
        if !any {
            println!("  (no match)");
        }
    }
}

#[test]
#[ignore]
fn fr_diagnose_tokenizer() {
    tokenizer_probe();
}

#[test]
#[ignore]
fn fr_diagnose_tagger() {
    tagger_probe();
}

#[test]
#[ignore]
fn fr_diagnose_offsets() {
    let pipeline = FrenchPipeline::shared();
    for text in ["alors il faut partir", "dans ce cas il est certain"] {
        println!("SPLIT {text:?} => {:?}", pipeline.sentence_ranges(text));
        for (sentence, sentence_start) in pipeline.analyze_offset_text(text) {
            println!(
                "  sentence {}..{} start {} text {:?}",
                sentence.start(),
                sentence.end(),
                sentence_start,
                sentence.text()
            );
            for (i, t) in sentence.tokens().iter().enumerate() {
                println!(
                    "    {i} {:?} [{}..{}]",
                    t.token().token(),
                    t.token().start(),
                    t.token().end()
                );
            }
        }
    }
}

#[test]
#[ignore]
fn fr_diagnose_readings() {
    readings_probe();
}

#[test]
#[ignore]
fn fr_diagnose_rule_matches() {
    rule_match_probe();
}
