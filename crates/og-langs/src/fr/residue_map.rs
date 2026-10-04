use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Family {
    TokenizerDecimalRun,
    TokenizerPercentWordChar,
    TokenizerLeadingHyphen,
    RawPosPreDisambiguation,
    SuppressMisspelledSuggestionSpeller,
    CommaSuggestionSentStartOffset,
    EmptyFlagGroupRegex,
    ScopeNextExceptionReadingZero,
    MatchNoZeroTextInterpolation,
    MultilineTokenTextNormalization,
    ApostropheNormalizationAsymmetry,
    Differential,
}

impl Family {
    fn java_anchor(self) -> &'static str {
        match self {
            Self::TokenizerDecimalRun => {
                "FrenchWordTokenizer.java:133-134 DECIMAL_POINT replaceAll joins only the first non-overlapping digit.digit per scan position"
            }
            Self::TokenizerPercentWordChar => {
                "FrenchWordTokenizer.java:37 wordCharacters class contains %"
            }
            Self::TokenizerLeadingHyphen => {
                "FrenchWordTokenizer.java:159-162 strips leading hyphens into standalone '-' tokens before pattern split and dictionary lookup; FrenchWordTokenizerTest.java:92 [-, Oui,  , -, l', homme, .]"
            }
            Self::RawPosPreDisambiguation => {
                "PatternRuleHandler.java:299 raw_pos=yes; PatternRuleMatcher.java:88-90 matches on getPreDisambiguationTokensWithoutWhitespace(); AnalyzedSentence.java:59 keeps the pre-disambiguation copy"
            }
            Self::SuppressMisspelledSuggestionSpeller => {
                "XMLRuleHandler.java:377 + PatternRuleHandler.java:374-383 mark suppress_misspelled matches; MatchState.java:307-318 replace unknown suggestion parts with <mistake/>; PatternRuleMatcher.java:47-51 + :186-190 drop the match"
            }
            Self::CommaSuggestionSentStartOffset => {
                "PatternRuleMatcher.java:177-182 fromPos = prev.getStartPos() + prev.getToken().length() with the Java SENT_START token whose text is empty (JLanguageTool.java:1813)"
            }
            Self::EmptyFlagGroupRegex => {
                "Java accepts the empty flag group (?-) as a no-op; fancy-regex 0.14.0 parse.rs:700-703 rejects it (grammar.xml:108181 and :108368)"
            }
            Self::ScopeNextExceptionReadingZero => {
                "AbstractPatternRulePerformer.java:184-190: the current element's scope-next exception is tested only when prevSkipNext == 0 and only against reading 0 of the next token"
            }
            Self::MatchNoZeroTextInterpolation => {
                "PatternToken.java:502-515 doCompile replaces the \\\\N placeholder inside the token string with the referenced token text (style.xml ECRITURE_INCLUSIVE <token><match no=\"0\"/>e</token>)"
            }
            Self::MultilineTokenTextNormalization => {
                "PatternToken.java:305-307 normalizeTextPattern + StringTools.java:485-496 trimWhitespace drop \\n/\\t/\\r inside token and exception text, so the line-trailing alternative 'frais\\n|froid' still matches 'frais'"
            }
            Self::ApostropheNormalizationAsymmetry => {
                "Java pattern matching normalizes no apostrophes (no U+2018 handling in PatternToken.java or StringMatcher.java); the class [´‘′`] matches the raw token '‘'"
            }
            Self::Differential => {
                "needs a JVM differential trace; probe evidence pinned in fr/trace.rs"
            }
        }
    }

    fn rust_target(self) -> &'static str {
        match self {
            Self::TokenizerDecimalRun
            | Self::TokenizerPercentWordChar
            | Self::TokenizerLeadingHyphen => "og-tokenizer/src/french_word_tokenizer.rs",
            Self::RawPosPreDisambiguation => {
                "og-xml/src/parser.rs (compile raw_pos) + og-core AnalyzedSentence (pre-disambiguation copy) + og-rules/src/pattern_rule.rs (token view selection)"
            }
            Self::SuppressMisspelledSuggestionSpeller => {
                "og-rules/src/pattern_rule.rs raw_match_alternatives/suggestion_alternatives"
            }
            Self::CommaSuggestionSentStartOffset => {
                "og-rules/src/pattern_rule.rs:627 and :639 (use prev.token().end(), not start + text length)"
            }
            Self::EmptyFlagGroupRegex => "og-xml/src/compiler.rs regex translation",
            Self::ScopeNextExceptionReadingZero => "og-rules/src/pattern_rule.rs:1352-1357",
            Self::MatchNoZeroTextInterpolation => {
                "og-xml/src/parser.rs (keep the in-text match placeholder) + og-rules/src/pattern_rule.rs backreference_matches"
            }
            Self::MultilineTokenTextNormalization => {
                "og-xml/src/parser.rs token and exception text accumulation"
            }
            Self::ApostropheNormalizationAsymmetry => {
                "og-xml/src/unifier.rs:8-13 + og-rules/src/pattern_rule.rs:2164-2188"
            }
            Self::Differential => "unresolved",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::TokenizerDecimalRun => "tokenizer decimal run",
            Self::TokenizerPercentWordChar => "tokenizer % word char",
            Self::TokenizerLeadingHyphen => "tokenizer leading hyphen",
            Self::RawPosPreDisambiguation => "raw_pos pre-disambiguation",
            Self::SuppressMisspelledSuggestionSpeller => "suppress_misspelled suggestion speller",
            Self::CommaSuggestionSentStartOffset => "comma-suggestion SENT_START offset",
            Self::EmptyFlagGroupRegex => "(?-) empty flag group",
            Self::ScopeNextExceptionReadingZero => "scope-next exception reading-0",
            Self::MatchNoZeroTextInterpolation => "<match no=\"0\"/> text interpolation",
            Self::MultilineTokenTextNormalization => "multiline token text",
            Self::ApostropheNormalizationAsymmetry => "apostrophe normalization asymmetry",
            Self::Differential => "differential",
        }
    }
}

const GRAMMAR_RESIDUES: &[(&str, &str, Family)] = &[
    (
        "MOTS_MAUX",
        "Antidote, le remède à tous vos mots.",
        Family::ScopeNextExceptionReadingZero,
    ),
    (
        "MOTS_MAUX",
        "Docteur délivrez moi de mes mots.",
        Family::ScopeNextExceptionReadingZero,
    ),
    (
        "LA_LA2",
        "La personne photographiée est à la fois :",
        Family::EmptyFlagGroupRegex,
    ),
    (
        "LA_LA2",
        "La guimbarde de Hulot à la 203 de Schmutz stationnée devant l’hôte.",
        Family::EmptyFlagGroupRegex,
    ),
    (
        "LA_LA2",
        "Mais la plupart travaillaient dans la pêcherie.",
        Family::EmptyFlagGroupRegex,
    ),
    (
        "IL_DEVIENT_QUOI",
        "Il devient de plus en plus.",
        Family::Differential,
    ),
    (
        "ALORS",
        "alors il faut partir",
        Family::CommaSuggestionSentStartOffset,
    ),
    (
        "DANS_CE_CAS",
        "dans ce cas il est certain",
        Family::CommaSuggestionSentStartOffset,
    ),
    (
        "AGREEMENT_PARTICULAR",
        "Sa configuration électronique est par ordre de niveau d'énergie croissant.",
        Family::Differential,
    ),
    (
        "ADJ_ADJ_SENT_END",
        "Quand Ddaykha étudiait à l'université, elle n'avait ni ami ni amie.",
        Family::Differential,
    ),
    (
        "ADJ_ADJ_SENT_END",
        "Ses grandes fleurs doubles sont roses à nuance lilas et très parfumées.",
        Family::Differential,
    ),
    (
        "PEUT_ETRE",
        "Le canon peut tourner sur 360 degrés et peut-être élevé à un angle de 90 degrés.",
        Family::ScopeNextExceptionReadingZero,
    ),
    (
        "PEUT_PEUX_PEU",
        "Quelqu\"un peut -il m'éclairer et si possible en justifiant.",
        Family::TokenizerLeadingHyphen,
    ),
    (
        "OU",
        "Je vous écrirai ou vous téléphonerai la semaine prochaine.",
        Family::Differential,
    ),
    (
        "CE_SE",
        "Il faut tout ce dire.",
        Family::RawPosPreDisambiguation,
    ),
    (
        "CE_SE",
        "C'est lui qui ce cache sous la table.",
        Family::RawPosPreDisambiguation,
    ),
    (
        "CE_SE",
        "Cela ce passe sous la table.",
        Family::RawPosPreDisambiguation,
    ),
    (
        "CE_SE",
        "Qu'attent-il pour ce lancer ?",
        Family::RawPosPreDisambiguation,
    ),
    (
        "CE_SE",
        "Il ne le fait pas pour ce repentir.",
        Family::RawPosPreDisambiguation,
    ),
    (
        "CE_SE",
        "Il a décidé de ce lever tôt.",
        Family::RawPosPreDisambiguation,
    ),
    (
        "VOIX_VOIE",
        "Le candidat a obtenu 51% des voies.",
        Family::TokenizerPercentWordChar,
    ),
    (
        "DU_DU",
        "C'est trop, tu n'aurais pas du savoir !",
        Family::RawPosPreDisambiguation,
    ),
    (
        "MOTS_COLLES_OU_SEPARES",
        "Il le fait poursoi.",
        Family::Differential,
    ),
    (
        "D_N_QUE_R_AVOIR_PPA",
        "Voici la piste qu'aurait négligé les enqêteurs.",
        Family::ScopeNextExceptionReadingZero,
    ),
    (
        "PRONSUJ_NONVERBE",
        "Je ne sé compris.",
        Family::ScopeNextExceptionReadingZero,
    ),
    (
        "ETRE_VPPA_OU_ADJ",
        "De leur rencontre est donc né ce lieu.",
        Family::Differential,
    ),
    (
        "ACCORD_SUJET_VERBE",
        "De quels sites parles -tu?",
        Family::TokenizerLeadingHyphen,
    ),
    (
        "PREFIXES_NON_SEPARES",
        "Pour revenir au post précédent.",
        Family::SuppressMisspelledSuggestionSpeller,
    ),
    (
        "PREFIXES_NON_SEPARES",
        "Miguel Diaz-Canel est porté sur les fonts néo-post-castristes.",
        Family::SuppressMisspelledSuggestionSpeller,
    ),
    (
        "PREFIXES_NON_SEPARES",
        "Elle poursuit les séries et aligne sur la post saison 14,3 points",
        Family::SuppressMisspelledSuggestionSpeller,
    ),
    (
        "FAIRE_PLUS_NOM_INCORRECT",
        "Il fait frais.",
        Family::MultilineTokenTextNormalization,
    ),
    (
        "REFAIRE_PLUS_NOM_INCORRECT",
        "Il refait usage de son bras.",
        Family::MultilineTokenTextNormalization,
    ),
    (
        "ESPACE_POURCENT",
        "Une croissance de 8%.",
        Family::TokenizerPercentWordChar,
    ),
    (
        "ESPACE_POURCENT",
        "Une croissance de 8,5%.",
        Family::TokenizerPercentWordChar,
    ),
    (
        "PAS_DE_TRAIT_UNION",
        "Le néo-impressionnisme",
        Family::SuppressMisspelledSuggestionSpeller,
    ),
    (
        "TRAIT_UNION_ESPACE",
        "Vas -tu les regarder à la télévision ?",
        Family::TokenizerLeadingHyphen,
    ),
    (
        "APOS_INCORRECT",
        "D‘enfants.",
        Family::ApostropheNormalizationAsymmetry,
    ),
    (
        "TIRET",
        "-il est arrivé à l'hôtel hier.",
        Family::TokenizerLeadingHyphen,
    ),
    (
        "DATE",
        "Il est né le 32.1.1985.",
        Family::TokenizerDecimalRun,
    ),
    (
        "DATE",
        "Il est né le 300.1.1985.",
        Family::TokenizerDecimalRun,
    ),
    (
        "DATE",
        "Il est né le 3000.1.1985.",
        Family::TokenizerDecimalRun,
    ),
    (
        "DATE",
        "Elle est née le 31.4.1989.",
        Family::TokenizerDecimalRun,
    ),
    (
        "DATE",
        "Elle est née le 30.2.1900.",
        Family::TokenizerDecimalRun,
    ),
    (
        "DATE",
        "Elle est née le 31.2.1900.",
        Family::TokenizerDecimalRun,
    ),
    (
        "DATE",
        "Elle est née le 29.2.1900.",
        Family::TokenizerDecimalRun,
    ),
    ("DATE", "Le 29.2.2100.", Family::TokenizerDecimalRun),
];

const STYLE_RESIDUES: &[(&str, &str, Family)] = &[(
    "ECRITURE_INCLUSIVE",
    "Pour l'étudiant ou étudiante.",
    Family::MatchNoZeroTextInterpolation,
)];

pub fn families_summary() -> Vec<(&'static str, usize, usize)> {
    let mut counts: HashMap<Family, (usize, usize)> = HashMap::new();
    for (_, _, family) in GRAMMAR_RESIDUES {
        counts.entry(*family).or_default().0 += 1;
    }
    for (_, _, family) in STYLE_RESIDUES {
        counts.entry(*family).or_default().1 += 1;
    }
    let mut out: Vec<_> = counts
        .into_iter()
        .map(|(family, (grammar, style))| (family.label(), grammar, style))
        .collect();
    out.sort_unstable();
    out
}

fn fr_rules_dir() -> Option<PathBuf> {
    crate::engine::find_lt_resource_dir()
        .map(|root| root.join("fr/src/main/resources/org/languagetool/rules/fr"))
}

fn rule_file(rule_id: &str) -> Option<String> {
    for file in ["grammar.xml", "style.xml"] {
        let xml = std::fs::read_to_string(fr_rules_dir()?.join(file)).ok()?;
        let needle = format!("id=\"{rule_id}\"");
        if xml.contains(&needle) {
            return Some(xml);
        }
    }
    None
}

fn compiled_rule_count() -> usize {
    static COUNT: OnceLock<usize> = OnceLock::new();
    *COUNT.get_or_init(|| {
        let mut total = 0usize;
        for file in ["grammar.xml", "style.xml"] {
            let Some(path) = fr_rules_dir().map(|dir| dir.join(file)) else {
                continue;
            };
            let Ok(xml) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(set) =
                og_xml::compiler::XmlCompiler::new().compile_file_with_base(&xml, path.parent())
            else {
                continue;
            };
            total += set.rules.len();
        }
        total
    })
}

pub fn dump() {
    eprintln!(
        "fr residue map: {} grammar + {} style",
        GRAMMAR_RESIDUES.len(),
        STYLE_RESIDUES.len()
    );
    for (label, grammar, style) in families_summary() {
        eprintln!("  {label:<42} grammar {grammar:>2}  style {style:>2}");
    }
    for (rule_id, text, family) in GRAMMAR_RESIDUES.iter().chain(STYLE_RESIDUES.iter()) {
        eprintln!(
            "  [{rule_id}] {text:?} => {} | {} | {}",
            family.label(),
            family.java_anchor(),
            family.rust_target()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn dump_residue_map() {
        super::dump();
    }

    #[test]
    fn residue_counts_match_the_round9_suites() {
        assert_eq!(GRAMMAR_RESIDUES.len(), 46, "grammar.xml residue inventory");
        assert_eq!(STYLE_RESIDUES.len(), 1, "style.xml residue inventory");
        let differential: Vec<_> = GRAMMAR_RESIDUES
            .iter()
            .chain(STYLE_RESIDUES.iter())
            .filter(|(_, _, f)| *f == Family::Differential)
            .map(|(id, text, _)| (*id, *text))
            .collect();
        assert_eq!(
            differential.len(),
            7,
            "unresolved differential residues: {differential:?}"
        );
    }

    #[test]
    fn every_residue_rule_exists_and_example_text_is_real() {
        let dir = fr_rules_dir().expect("fr rules dir");
        for file in ["grammar.xml", "style.xml"] {
            std::fs::read_to_string(dir.join(file)).expect(file);
        }
        for (rule_id, text, _) in GRAMMAR_RESIDUES.iter().chain(STYLE_RESIDUES.iter()) {
            let xml = rule_file(rule_id).unwrap_or_else(|| panic!("rule {rule_id} not found"));
            let flat = xml.replace("<marker>", "").replace("</marker>", "");
            let mut end = text.len().min(24);
            while !text.is_char_boundary(end) {
                end += 1;
            }
            let clean_needle = &text[..end];
            assert!(
                flat.contains(clean_needle),
                "example fragment {clean_needle:?} of {rule_id} not found in rules xml"
            );
        }
    }

    #[test]
    fn residue_rules_are_all_present_in_the_compiled_sets() {
        let dir = fr_rules_dir().expect("fr rules dir");
        let mut ids: Vec<String> = Vec::new();
        for file in ["grammar.xml", "style.xml"] {
            let xml = std::fs::read_to_string(dir.join(file)).expect(file);
            let set = og_xml::compiler::XmlCompiler::new()
                .compile_file_with_base(&xml, Some(&dir))
                .expect(file);
            ids.extend(set.rules.iter().map(|r| r.id.clone()));
        }
        for (rule_id, _, _) in GRAMMAR_RESIDUES.iter().chain(STYLE_RESIDUES.iter()) {
            assert!(
                ids.iter().any(|id| id == rule_id),
                "residue rule {rule_id} missing from compiled sets"
            );
        }
        assert!(compiled_rule_count() > 0);
    }

    #[test]
    fn java_empty_flag_group_is_rejected_by_the_rust_regex_engine() {
        assert!(regex::Regex::new("(?-)[A-Z]").is_err());
        assert!(regex::Regex::new("(?-i)[A-Z]").is_ok());
    }

    #[test]
    fn apos_incorrect_exception_class_covers_all_four_apostrophe_variants() {
        let xml = rule_file("APOS_INCORRECT").expect("APOS_INCORRECT in grammar.xml");
        assert!(xml.contains("[´‘′`]"));
        let class = regex::Regex::new("[´‘′`]").expect("literal class compiles");
        for variant in ['´', '‘', '′', '`'] {
            assert!(class.is_match(&variant.to_string()), "{variant:?} in class");
        }
        let folded = regex::Regex::new("(?i)[´‘′`]").expect("ci class compiles");
        for variant in ['´', '‘', '′', '`'] {
            assert!(
                folded.is_match(&variant.to_string()),
                "case-insensitive class must still match {variant:?}"
            );
        }
        assert_eq!(
            og_xml::unifier::normalize_apostrophes("‘"),
            "'",
            "the engine rewrites U+2018 on the token side while the pattern class keeps it"
        );
    }

    #[test]
    fn faire_plus_exception_regexp_still_matches_its_listed_words() {
        let dir = fr_rules_dir().expect("fr rules dir");
        let xml = std::fs::read_to_string(dir.join("grammar.xml")).expect("grammar.xml");
        let set = og_xml::compiler::XmlCompiler::new()
            .compile_file_with_base(&xml, Some(&dir))
            .expect("compile grammar.xml");
        let rule = set
            .rules
            .iter()
            .find(|r| r.id == "FAIRE_PLUS_NOM_INCORRECT")
            .expect("FAIRE_PLUS_NOM_INCORRECT");
        let exception = rule
            .pattern
            .tokens
            .iter()
            .flat_map(|t| t.exceptions.iter())
            .find(|e| e.text.as_deref().is_some_and(|t| t.contains("abstraction")))
            .expect("word-list exception");
        assert!(
            exception.compiled_regexp.is_some() || exception.compiled_regexp_fancy.is_some(),
            "giant word-list exception regexp failed to compile in both engines"
        );
    }

    #[test]
    #[ignore]
    fn trace_faire_plus_exception_compile_paths() {
        let dir = fr_rules_dir().expect("fr rules dir");
        let xml = std::fs::read_to_string(dir.join("grammar.xml")).expect("grammar.xml");
        let start = xml.find("abstraction").expect("word list");
        let end = xml[start..].find("</exception>").expect("exception end");
        let list = &xml[start..start + end];
        let trimmed = list.trim();
        let anchored = format!("^(?:{trimmed})$");
        let plain = regex::Regex::new(&anchored);
        eprintln!(
            "plain compile: {}",
            plain.as_ref().is_ok_and(|re| re.is_match("frais"))
        );
        let folded = og_xml::java_case_fold::compile_java_ci(&anchored);
        eprintln!(
            "java-ci compile: {}",
            folded.as_ref().is_some_and(|re| re.is_match("frais"))
        );
        let ci = regex::Regex::new(&format!("(?i){anchored}"));
        eprintln!(
            "(?i) compile: {}",
            ci.as_ref().is_ok_and(|re| re.is_match("frais"))
        );
        if let Some(re) = folded {
            let hits: Vec<bool> = ["frais", "abstraction", "confiance", "usage", "violence"]
                .iter()
                .map(|w| re.is_match(w))
                .collect();
            eprintln!("java-ci per-word: {hits:?}");
        }
        eprintln!("list head: {:?}", &list[..80.min(list.len())]);
    }
}
