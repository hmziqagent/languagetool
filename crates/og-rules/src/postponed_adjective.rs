use og_core::{AnalyzedToken, AnalyzedTokenReadings, RuleMatch, SuggestedReplacement};
use og_tagger::morfologik::MorfologikDictionary;
use regex::Regex;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

const MAX_LEVELS: usize = 4;

fn full_match(pattern: &str) -> Regex {
    Regex::new(&format!("^(?:{pattern})$")).expect("valid postag pattern")
}

fn reading_matches(reading: &AnalyzedToken, re: &Regex) -> bool {
    let tags = reading.pos_tags();
    if tags.is_empty() {
        re.is_match("UNKNOWN")
    } else {
        tags.iter().any(|t| re.is_match(t))
    }
}

fn mp(token: Option<&AnalyzedTokenReadings>, re: &Regex) -> bool {
    token.is_some_and(|t| t.readings().iter().any(|r| reading_matches(r, re)))
}

fn mr(text: &str, re: &Regex) -> bool {
    re.is_match(text)
}

fn matching_reading<'a>(token: &'a AnalyzedTokenReadings, re: &Regex) -> Option<&'a AnalyzedToken> {
    token.readings().iter().find(|r| reading_matches(r, re))
}

#[derive(Default)]
struct Apparitions {
    adverb: bool,
    conjunction: bool,
    punctuation: bool,
    infinitive: bool,
}

#[derive(Default)]
struct Levels {
    nt: [u32; MAX_LEVELS],
    nms: [u32; MAX_LEVELS],
    nfs: [u32; MAX_LEVELS],
    nmp: [u32; MAX_LEVELS],
    nmn: [u32; MAX_LEVELS],
    nfp: [u32; MAX_LEVELS],
    ncs: [u32; MAX_LEVELS],
    ncp: [u32; MAX_LEVELS],
    dms: [u32; MAX_LEVELS],
    dfs: [u32; MAX_LEVELS],
    dmp: [u32; MAX_LEVELS],
    dfp: [u32; MAX_LEVELS],
}

struct TagSet {
    nom: Regex,
    nom_ms: Regex,
    nom_fs: Regex,
    nom_mp: Regex,
    nom_mn: Regex,
    nom_fp: Regex,
    nom_cs: Regex,
    nom_cp: Regex,
    nom_det: Regex,
    gn_any: Regex,
    gn_ms_tag: Regex,
    gn_fs_tag: Regex,
    gn_mp_tag: Regex,
    gn_fp_tag: Regex,
    gn_cs_tag: Regex,
    gn_cp_tag: Regex,
    det: Regex,
    det_cs: Regex,
    det_ms: Regex,
    det_fs: Regex,
    det_mp: Regex,
    det_fp: Regex,
    gn_ms: Regex,
    gn_fs: Regex,
    gn_mp: Regex,
    gn_fp: Regex,
    gn_cp: Regex,
    gn_cs: Regex,
    adjectiu: Regex,
    adjectiu_ms: Regex,
    adjectiu_fs: Regex,
    adjectiu_mp: Regex,
    adjectiu_fp: Regex,
    adjectiu_cp: Regex,
    adjectiu_cs: Regex,
    adjectiu_s: Regex,
    adjectiu_p: Regex,
    adverbi: Regex,
    conjuncio: Regex,
    puntuacio: Regex,
    loc_adv: Regex,
    adverbis_acceptats: Regex,
    coord_ioni: Regex,
    keep_count: Regex,
    keep_count2: Regex,
    stop_count: Regex,
    preposicions: Regex,
    prepo_nivell: Regex,
    verb: Regex,
    gv: Regex,
    synth_common_plural: Regex,
    synth_cs: Regex,
    synth_cp: Regex,
    synth_ms: Regex,
    synth_fs: Regex,
    synth_mp: Regex,
    synth_fp: Regex,
}

fn es_tags() -> &'static TagSet {
    static TAGS: OnceLock<TagSet> = OnceLock::new();
    TAGS.get_or_init(|| TagSet {
        nom: full_match("N.*"),
        nom_ms: full_match("N.MS.*|PI0MS000"),
        nom_fs: full_match("N.FS.*|PI0FS000"),
        nom_mp: full_match("N.MP.*"),
        nom_mn: full_match("N.MN.*"),
        nom_fp: full_match("N.FP.*"),
        nom_cs: full_match("N.CS.*"),
        nom_cp: full_match("N.CP.*"),
        nom_det: full_match("N.*|D[NDA0I].*|PI0[MF]S000"),
        gn_any: full_match("_GN_.*"),
        gn_ms_tag: full_match("_GN_MS"),
        gn_fs_tag: full_match("_GN_FS"),
        gn_mp_tag: full_match("_GN_MP"),
        gn_fp_tag: full_match("_GN_FP"),
        gn_cs_tag: full_match("_GN_[MF]S"),
        gn_cp_tag: full_match("_GN_[MF]P"),
        det: full_match("D[NDA0IP].*"),
        det_cs: full_match("D[NDA0IP]0CS0"),
        det_ms: full_match("D[NDA0IP]0MS0"),
        det_fs: full_match("D[NDA0IP]0FS0"),
        det_mp: full_match("D[NDA0IP]0MP0"),
        det_fp: full_match("D[NDA0IP]0FP0"),
        gn_ms: full_match(
            "N.[MC][SN].*|A..[MC][SN].*|V.P..SM.?|PX.MS.*|D[NDA0I]0MS0|PI0MS000",
        ),
        gn_fs: full_match(
            "N.[FC][SN].*|A..[FC][SN].*|V.P..SF.?|PX.FS.*|D[NDA0I]0FS0|PI0FS000",
        ),
        gn_mp: full_match(
            "N.[MC][PN].*|A..[MC][PN].*|V.P..PM.?|PX.MP.*|D[NDA0I]0MP0",
        ),
        gn_fp: full_match(
            "N.[FC][PN].*|A..[FC][PN].*|V.P..PF.?|PX.FP.*|D[NDA0I]0FP0",
        ),
        gn_cp: full_match("N.[FMC][PN].*|A..[FMC][PN].*|D[NDA0I]0[FM]P0"),
        gn_cs: full_match("N.[FMC][SN].*|A..[FMC][SN].*|D[NDA0I]0[FM]S0||PI0[MFC]S000"),
        adjectiu: full_match("AQ.*|V.P.*|PX.*|.*LOC_ADJ.*"),
        adjectiu_ms: full_match("A..[MC][SN].*|V.P..SM.?|PX.MS.*"),
        adjectiu_fs: full_match("A..[FC][SN].*|V.P..SF.?|PX.FS.*"),
        adjectiu_mp: full_match("A..[MC][PN].*|V.P..PM.?|PX.MP.*"),
        adjectiu_fp: full_match("A..[FC][PN].*|V.P..PF.?|PX.FP.*"),
        adjectiu_cp: full_match("A..C[PN].*"),
        adjectiu_cs: full_match("A..C[SN].*"),
        adjectiu_s: full_match("A...[SN].*|V.P..S..?|PX..S.*"),
        adjectiu_p: full_match("A...[PN].*|V.P..P..?|PX..P.*"),
        adverbi: full_match("R.|.*LOC_ADV.*"),
        conjuncio: full_match("C.|.*LOC_CONJ.*"),
        puntuacio: full_match("_PUNCT"),
        loc_adv: full_match(".*LOC_ADV.*"),
        adverbis_acceptats: full_match("RG_before"),
        coord_ioni: full_match("y|e|o|u|ni"),
        keep_count: full_match(
            "A.*|N.*|D[NAIDP].*|SPS.*|SP:DA|.*LOC_ADV.*|V.P.*|_PUNCT.*|.*LOC_ADJ.*|PX.*|PI0.S000|UNKNOWN|V.N.{4}",
        ),
        keep_count2: full_match(",|y|e|o|ni|u"),
        stop_count: full_match(";|lo"),
        preposicions: full_match("SP.*"),
        prepo_nivell: full_match("de|del|en|sobre|a|entre|por|con|sin|contra|para"),
        verb: full_match("V.[^P].*|_GV_"),
        gv: full_match("_GV_"),
        synth_cs: full_match("A..CP."),
        synth_cp: full_match("A..CS."),
        synth_common_plural: full_match("A...P.|V.P..P.|PX..P.*"),
        synth_ms: full_match("A..MS.|V.P..SM|PX.MS.*"),
        synth_fs: full_match("A..FS.|V.P..SF|PX.FS.*"),
        synth_mp: full_match("A..MP.|V.P..PM|PX.MP.*"),
        synth_fp: full_match("A..FP.|V.P..PF|PX.FP.*"),
    })
}

struct FrTagSet {
    base: TagSet,
    det_cp: Regex,
    gn_mn: Regex,
    gn_fn: Regex,
    gn_mn_tag: Regex,
    gn_fn_tag: Regex,
    adjectiu_mn: Regex,
    adjectiu_fn: Regex,
    adjectiu_m: Regex,
    infinitive: Regex,
}

fn fr_tags() -> &'static FrTagSet {
    static TAGS: OnceLock<FrTagSet> = OnceLock::new();
    TAGS.get_or_init(|| FrTagSet {
        base: TagSet {
            nom: full_match("[NZ] .*"),
            nom_ms: full_match("[NZ] m s"),
            nom_fs: full_match("[NZ] f s"),
            nom_mp: full_match("[NZ] m p"),
            nom_mn: full_match("[NZ] m sp"),
            nom_fp: full_match("[NZ] f p"),
            nom_cs: full_match("[NZ] e s"),
            nom_cp: full_match("[NZ] e sp"),
            nom_det: full_match("[NZ] .*|(P\\+)?D .*"),
            gn_any: full_match("_GN_.*"),
            gn_ms_tag: full_match("_GN_MS"),
            gn_fs_tag: full_match("_GN_FS"),
            gn_mp_tag: full_match("_GN_MP"),
            gn_fp_tag: full_match("_GN_FP"),
            gn_cs_tag: full_match("_GN_[MF]S"),
            gn_cp_tag: full_match("_GN_[MF]P"),
            det: full_match("(P\\+)?D .*"),
            det_cs: full_match("(P\\+)?D e s"),
            det_ms: full_match("(P\\+)?D m s"),
            det_fs: full_match("(P\\+)?D f s"),
            det_mp: full_match("(P\\+)?D m p"),
            det_fp: full_match("(P\\+)?D f p"),
            gn_ms: full_match("[NZ] [me] (s|sp)|J [me] (s|sp)|V ppa m s|(P\\+)?D m (s|sp)"),
            gn_fs: full_match("[NZ] [fe] (s|sp)|J [fe] (s|sp)|V ppa f s|(P\\+)?D f (s|sp)"),
            gn_mp: full_match("[NZ] [me] (p|sp)|J [me] (p|sp)|V ppa m p|(P\\+)?D m (p|sp)"),
            gn_fp: full_match("[NZ] [fe] (p|sp)|J [fe] (p|sp)|V ppa f p|(P\\+)?D f (p|sp)"),
            gn_cp: full_match("[NZ] [fme] (p|sp)|J [fme] (p|sp)|(P\\+)?D [fme] (p|sp)"),
            gn_cs: full_match("[NZ] [fme] (s|sp)|J [fme] (s|sp)|(P\\+)?D [fme] (s|sp)"),
            adjectiu: full_match("J .*|V ppa .*|PX.*"),
            adjectiu_ms: full_match("J [me] (s|sp)|V ppa m s"),
            adjectiu_fs: full_match("J [fe] (s|sp)|V ppa f s"),
            adjectiu_mp: full_match("J [me] (p|sp)|V ppa m p"),
            adjectiu_fp: full_match("J [fe] (p|sp)|V ppa f p"),
            adjectiu_cp: full_match("J e (p|sp)"),
            adjectiu_cs: full_match("J e (s|sp)"),
            adjectiu_s: full_match("J .* (s|sp)|V ppa . s"),
            adjectiu_p: full_match("J .* (p|sp)|V ppa . p"),
            adverbi: full_match("A"),
            conjuncio: full_match("C .*"),
            puntuacio: full_match("_PUNCT"),
            loc_adv: full_match("A"),
            adverbis_acceptats: full_match("A"),
            coord_ioni: full_match("et|ou|ni"),
            keep_count: full_match(
                "Y|J .*|N .*|D .*|P.*|V ppa .*|M nonfin|UNKNOWN|Z.*|V.* inf|V ppr",
            ),
            keep_count2: full_match(",|et|ou|ni"),
            stop_count: full_match("[;:\\(\\)\\[\\]–—―‒]"),
            preposicions: full_match("P.*"),
            prepo_nivell: full_match(
                "d'|de|des|du|à|au|aux|en|dans|sur|entre|par|pour|avec|sans|contre|comme",
            ),
            verb: full_match("V.* (inf|ind|sub|con|ppr|imp).*"),
            gv: full_match("_GV_"),
            synth_cs: full_match("J e p"),
            synth_cp: full_match("J e s"),
            synth_common_plural: full_match("J . p|V ppa . p"),
            synth_ms: full_match("J [me] sp?|V ppa m s"),
            synth_fs: full_match("J [fe] sp?|V ppa f s"),
            synth_mp: full_match("J [me] s?p|V ppa m p"),
            synth_fp: full_match("J [fe] s?p|V ppa f p"),
        },
        det_cp: full_match("(P\\+)?D e p"),
        gn_mn: full_match("[NZ] [me] (s|p|sp)|J [me] (s|p|sp)|(P\\+)?D [me] (s|p|sp)"),
        gn_fn: full_match("[NZ] [fe] (s|p|sp)|J [fe] (s|p|sp)|(P\\+)?D [fe] (s|p|sp)"),
        gn_mn_tag: full_match("_GN_M[SP]"),
        gn_fn_tag: full_match("_GN_F[SP]"),
        adjectiu_mn: full_match("J m sp"),
        adjectiu_fn: full_match("J f sp"),
        adjectiu_m: full_match("J [me] .*|V ppa [me] .*"),
        infinitive: full_match("V.* inf"),
    })
}

const FR_EXCEPTIONS_EGRAVE: [&str; 3] = ["burkinabè", "koinè", "épistémè"];

fn is_exception_fr(word: &str) -> bool {
    if word.starts_with("qq") {
        return true;
    }
    if word.ends_with('è') {
        let lower = word.to_lowercase();
        return !FR_EXCEPTIONS_EGRAVE.contains(&lower.as_str());
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SynthLang {
    German,
    Spanish,
    French,
    English,
    Portuguese,
}

pub struct LangSynthesizer {
    dictionary: Option<Arc<MorfologikDictionary>>,
    manual: Vec<(Box<str>, Box<str>, Box<str>)>,
    removals: Vec<(Box<str>, Box<str>, Box<str>)>,
    removals2: Vec<(Box<str>, Box<str>, Box<str>)>,
    lang: SynthLang,
    article: crate::en_article::ArticleData,
    lemma_forms: OnceLock<HashMap<Box<str>, Vec<(Box<str>, Box<str>)>>>,
}

const GERMAN_REMOVE: &[&str] = &[
    "unsren",
    "unsrem",
    "unsres",
    "unsre",
    "unsern",
    "unserm",
    "unsrer",
    "angepaßt",
    "beschloß",
    "biß",
    "entschloß",
    "ergoß",
    "faßt",
    "genoß",
    "paßt",
    "paßte",
    "preßt",
    "preßte",
    "riß",
    "schloß",
    "streßtest",
    "vergißt",
    "verlaß",
    "verläßt",
    "vermiß",
    "vermißt",
    "wißt",
    "wußtest",
    "wüßtest",
];

impl LangSynthesizer {
    pub fn for_language(resource_dir: &Path, code: &str) -> Option<Self> {
        let (dictionary, lang): (Option<Arc<MorfologikDictionary>>, SynthLang) = match code {
            "de" => (
                og_tagger::german_tagger::shared_german_dictionary(resource_dir),
                SynthLang::German,
            ),
            "es" => (
                og_tagger::generic_tagger::shared_dictionary(resource_dir, "spanish"),
                SynthLang::Spanish,
            ),
            "fr" => (
                og_tagger::generic_tagger::shared_dictionary(resource_dir, "french"),
                SynthLang::French,
            ),
            "pt" => (
                og_tagger::generic_tagger::shared_dictionary(resource_dir, "portuguese"),
                SynthLang::Portuguese,
            ),
            "en" => {
                let dict = std::fs::read_to_string(resource_dir.join("dict_decoded.txt"))
                    .ok()
                    .map(|data| Arc::new(MorfologikDictionary::from_decoded_text(&data)));
                (dict, SynthLang::English)
            }
            _ => return None,
        };
        let manual = std::fs::read_to_string(resource_dir.join("added.txt"))
            .map(|t| parse_manual(&t))
            .unwrap_or_default();
        let removals = std::fs::read_to_string(resource_dir.join("removed.txt"))
            .map(|t| parse_manual(&t))
            .unwrap_or_default();
        let removals2 = std::fs::read_to_string(resource_dir.join("do-not-synthesize.txt"))
            .map(|t| parse_manual(&t))
            .unwrap_or_default();
        let article = if lang == SynthLang::English {
            let rules_dir = resource_dir.join("../../rules/en");
            if rules_dir.join("det_a.txt").is_file() {
                crate::en_article::ArticleData::from_rules_dir(&rules_dir)
            } else {
                crate::en_article::ArticleData::empty()
            }
        } else {
            crate::en_article::ArticleData::empty()
        };
        Some(Self {
            dictionary,
            manual,
            removals,
            removals2,
            lang,
            article,
            lemma_forms: OnceLock::new(),
        })
    }

    fn is_german(&self) -> bool {
        self.lang == SynthLang::German
    }

    fn is_french(&self) -> bool {
        self.lang == SynthLang::French
    }

    fn is_english(&self) -> bool {
        self.lang == SynthLang::English
    }

    fn is_spanish(&self) -> bool {
        self.lang == SynthLang::Spanish
    }

    fn forms(&self) -> &HashMap<Box<str>, Vec<(Box<str>, Box<str>)>> {
        self.lemma_forms.get_or_init(|| {
            let mut map: HashMap<Box<str>, Vec<(Box<str>, Box<str>)>> = HashMap::new();
            if let Some(dict) = &self.dictionary {
                for (lemma, entries) in dict.forms_by_lemma() {
                    let list = map.entry(Box::from(*lemma)).or_default();
                    for (tag, form) in entries {
                        list.push((Box::from(*tag), Box::from(*form)));
                    }
                }
            }
            for list in map.values_mut() {
                list.sort();
            }
            map
        })
    }

    pub fn synthesize(&self, lemma: &str, tag: &Regex) -> Vec<String> {
        let mut results = self.lookup_forms(lemma, tag);
        if self.is_german() {
            results.retain(|r| !GERMAN_REMOVE.contains(&r.as_str()));
        }
        results
    }

    fn german_lookup_ok(&self, lemma: &str, form: &str) -> bool {
        if !self.is_german() {
            return true;
        }
        let lc_lemma = lemma.chars().next().is_some_and(|c| c.is_lowercase());
        let lc_form = form.chars().next().is_some_and(|c| c.is_lowercase());
        lc_lemma == lc_form || lemma == "mein" || lemma == "ich" && !GERMAN_REMOVE.contains(&form)
    }

    fn keep_form(&self, form: &str) -> bool {
        if self.is_french() {
            return !is_exception_fr(form);
        }
        if self.is_english() {
            return !is_exception_en(form);
        }
        true
    }

    fn split_es_lemma<'a>(&self, lemma: &'a str, postag: &str) -> (&'a str, &'a str) {
        if self.is_spanish()
            && postag.starts_with('V')
            && let Some((head, tail)) = lemma.split_once(' ')
        {
            (head, tail)
        } else {
            (lemma, "")
        }
    }

    pub fn is_determiner_tag(&self, postag: &str) -> bool {
        self.is_english() && (postag == "+DT" || postag == "+INDT")
    }

    pub fn determiner_forms(&self, token: &str, postag: &str) -> Vec<String> {
        if postag == "+DT" {
            return vec![
                self.article.suggest_a_or_an(token),
                format!(
                    "the {}",
                    crate::en_article::lowercase_first_char_if_capitalized(token)
                ),
            ];
        }
        vec![self.article.suggest_a_or_an(token)]
    }

    pub fn synthesize_exact(&self, lemma: &str, postag: &str) -> Vec<String> {
        let (head, tail) = self.split_es_lemma(lemma, postag);
        let mut results = self.lookup_exact(head, postag);
        if results.is_empty() && self.is_german() {
            results = self.compound_forms_matching(head, |t| t == postag);
        } else if self.is_german() {
            results.retain(|r| !GERMAN_REMOVE.contains(&r.as_str()));
        }
        if !tail.is_empty() {
            for form in &mut results {
                *form = format!("{form} {tail}");
            }
        }
        results
    }

    pub fn target_postag<'a>(&self, pos_tags: &[&'a str]) -> Option<&'a str> {
        if pos_tags.is_empty() {
            return None;
        }
        if !self.is_spanish() {
            return pos_tags.last().copied();
        }
        let mut sorted: Vec<&str> = pos_tags.to_vec();
        sorted.sort_by(|a, b| {
            if *a == "VMIP3S0" && *b == "VMM02S0" {
                std::cmp::Ordering::Greater
            } else if *a == "VMM02S0" && *b == "VMIP3S0" {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        });
        sorted.pop()
    }

    pub fn synthesize_full(&self, lemma: &str, target: &str) -> Vec<String> {
        let Ok(re) = Regex::new(&format!("^(?:{target})$")) else {
            return Vec::new();
        };
        let (head, tail) = self.split_es_lemma(lemma, target);
        let mut results = self.synthesize(head, &re);
        if results.is_empty() && self.is_german() {
            results = self.compound_forms_matching(head, |t| re.is_match(t));
        }
        if !tail.is_empty() {
            for form in &mut results {
                *form = format!("{form} {tail}");
            }
        }
        results.sort();
        results.dedup();
        results
    }

    fn compound_parts(&self, lemma: &str) -> Option<(String, String, &'static str, bool)> {
        let mut parts = og_tagger::jwordsplitter::tokenize(lemma);
        if parts.is_empty() {
            return None;
        }
        let mut maybe_hyphen = "";
        if parts.len() == 1 {
            let hyphen_split: Vec<String> = lemma.split('-').map(str::to_string).collect();
            if hyphen_split.len() > 1 {
                maybe_hyphen = "-";
                parts = hyphen_split;
            }
        }
        let (last_raw, first_parts) = parts.split_last()?;
        let first_part = first_parts.join(maybe_hyphen);
        let last_part = uppercase_first_char(last_raw);
        let uppercase_last =
            !maybe_hyphen.is_empty() && last_raw.chars().next().is_some_and(char::is_uppercase);
        Some((last_part, first_part, maybe_hyphen, uppercase_last))
    }

    fn compound_forms_matching(
        &self,
        lemma: &str,
        matches_tag: impl Fn(&str) -> bool,
    ) -> Vec<String> {
        let Some((last_part, first_part, maybe_hyphen, uppercase_last)) =
            self.compound_parts(lemma)
        else {
            return Vec::new();
        };
        let mut results = Vec::new();
        for form in self.lookup_matching(&last_part, &matches_tag) {
            let tail = if uppercase_last {
                form
            } else {
                lowercase_first_char(&form)
            };
            results.push(format!("{first_part}{maybe_hyphen}{tail}"));
        }
        results
    }

    fn lookup_matching(&self, lemma: &str, matches_tag: &impl Fn(&str) -> bool) -> Vec<String> {
        let mut results = Vec::new();
        if let Some(entries) = self.forms().get(lemma) {
            for (t, form) in entries {
                if matches_tag(t) && self.german_lookup_ok(lemma, form) {
                    results.push(form.to_string());
                }
            }
        }
        self.extend_manual(lemma, &mut results, matches_tag);
        self.apply_removals(lemma, &mut results, matches_tag);
        results.retain(|r| self.keep_form(r));
        results
    }

    fn lookup_forms(&self, lemma: &str, tag: &Regex) -> Vec<String> {
        self.lookup_matching(lemma, &|t| tag.is_match(t))
    }

    fn lookup_exact(&self, lemma: &str, postag: &str) -> Vec<String> {
        self.lookup_matching(lemma, &|t| t == postag)
    }

    fn extend_manual(
        &self,
        lemma: &str,
        results: &mut Vec<String>,
        matches_tag: &impl Fn(&str) -> bool,
    ) {
        for (l, t, form) in &self.manual {
            if l.as_ref() == lemma && matches_tag(t) && self.german_lookup_ok(lemma, form) {
                results.push(form.to_string());
            }
        }
    }

    fn apply_removals(
        &self,
        lemma: &str,
        results: &mut Vec<String>,
        matches_tag: &impl Fn(&str) -> bool,
    ) {
        for (l, t, form) in self.removals.iter().chain(self.removals2.iter()) {
            if l.as_ref() == lemma && matches_tag(t) {
                results.retain(|r| r.as_str() != form.as_ref());
            }
        }
    }
}

const EN_EXCEPTIONS: &[&str] = &[
    "ne'er",
    "e'er",
    "o'er",
    "ol'",
    "ma'am",
    "n't",
    "informations",
];

fn is_exception_en(word: &str) -> bool {
    word.starts_with('\'') || EN_EXCEPTIONS.contains(&word)
}

fn uppercase_first_char(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn lowercase_first_char(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn parse_manual(text: &str) -> Vec<(Box<str>, Box<str>, Box<str>)> {
    let mut entries = Vec::new();
    let mut separator = "\t";
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if let Some(sep) = line.strip_prefix("#separatorRegExp=") {
            separator = sep;
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.split('#').next().unwrap_or("").trim();
        let mut parts = line.splitn(3, separator);
        if let (Some(form), Some(lemma), Some(tag)) = (parts.next(), parts.next(), parts.next()) {
            entries.push((lemma.into(), tag.trim().into(), form.into()));
        }
    }
    entries
}

fn arg_is_true(args: &str, key: &str) -> bool {
    args.split_whitespace().any(|part| {
        part.strip_prefix(&format!("{key}:"))
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    })
}

fn dedup_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::with_capacity(values.len());
    values
        .into_iter()
        .filter(|v| seen.insert(v.clone()))
        .collect()
}

pub(crate) fn accept_es(
    synthesizer: Option<&LangSynthesizer>,
    args: &str,
    matched_positions: &[usize],
    tokens: &[&AnalyzedTokenReadings],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let t = es_tags();
    let add_comma = arg_is_true(args, "addComma");
    let i = *matched_positions.first()?;
    let token_at = |idx: usize| tokens.get(idx).copied();
    let adjective = token_at(i);

    let mut is_plural = true;
    let mut is_prev_noun = false;
    let mut can_be_ms = false;
    let mut can_be_fs = false;
    let mut can_be_mp = false;
    let mut can_be_fp = false;
    let mut can_be_p = false;
    let mut lv = Levels::default();
    let mut apparitions = Apparitions::default();
    let mut level = 0usize;
    let mut j = 1usize;

    while i > j && keep_counting_es(token_at(i - j), &apparitions, t) && level < MAX_LEVELS {
        let cur = i - j;
        let current = token_at(cur);
        if !is_prev_noun {
            if mp(current, &t.nom)
                || (cur > 1
                    && !mp(current, &t.nom)
                    && mp(current, &t.adjectiu)
                    && mp(token_at(cur - 1), &t.det))
            {
                if mp(current, &t.gn_ms_tag) {
                    lv.nms[level] += 1;
                    can_be_ms = true;
                }
                if mp(current, &t.gn_fs_tag) {
                    lv.nfs[level] += 1;
                    can_be_fs = true;
                }
                if mp(current, &t.gn_mp_tag) {
                    lv.nmp[level] += 1;
                    can_be_mp = true;
                }
                if mp(current, &t.gn_fp_tag) {
                    lv.nfp[level] += 1;
                    can_be_fp = true;
                }
            }
            if !mp(current, &t.gn_any) {
                if mp(current, &t.nom_ms) {
                    lv.nms[level] += 1;
                    can_be_ms = true;
                } else if mp(current, &t.nom_fs) {
                    lv.nfs[level] += 1;
                    can_be_fs = true;
                } else if mp(current, &t.nom_mp) {
                    lv.nmp[level] += 1;
                    can_be_mp = true;
                } else if mp(current, &t.nom_mn) {
                    lv.nmn[level] += 1;
                    can_be_ms = true;
                    can_be_mp = true;
                } else if mp(current, &t.nom_fp) {
                    lv.nfp[level] += 1;
                    can_be_fp = true;
                } else if mp(current, &t.nom_cs) {
                    lv.ncs[level] += 1;
                    can_be_ms = true;
                    can_be_fs = true;
                } else if mp(current, &t.nom_cp) {
                    lv.ncp[level] += 1;
                    can_be_fp = true;
                    can_be_mp = true;
                }
            }
        }
        if mp(current, &t.nom) {
            lv.nt[level] += 1;
            is_prev_noun = true;
        } else {
            is_prev_noun = false;
        }
        if mp(current, &t.det_cs) {
            if mp(token_at(cur + 1), &t.nom_ms) {
                lv.dms[level] += 1;
                can_be_ms = true;
            }
            if mp(token_at(cur + 1), &t.nom_fs) {
                lv.dfs[level] += 1;
                can_be_fs = true;
            }
        }
        if !mp(current, &t.adverbi) {
            if mp(current, &t.det_ms) {
                lv.dms[level] += 1;
                can_be_ms = true;
            }
            if mp(current, &t.det_fs) {
                lv.dfs[level] += 1;
                can_be_fs = true;
            }
            if mp(current, &t.det_mp) {
                lv.dmp[level] += 1;
                can_be_mp = true;
            }
            if mp(current, &t.det_fp) {
                lv.dfp[level] += 1;
                can_be_fp = true;
            }
        }
        if i > j {
            let text = current.map(|c| c.token().token()).unwrap_or_default();
            if mr(text, &t.prepo_nivell)
                && !mr(
                    token_at(cur - 1)
                        .map(|c| c.token().token())
                        .unwrap_or_default(),
                    &t.coord_ioni,
                )
                && !mp(token_at(cur + 1), &t.adverbi)
            {
                level += 1;
            }
        }
        if level > 0
            && mr(
                token_at(cur).map(|c| c.token().token()).unwrap_or_default(),
                &t.coord_ioni,
            )
        {
            let mut k = 1usize;
            while k < 4 && i > j + k {
                let back = token_at(i - j - k);
                let back_text = back.map(|c| c.token().token()).unwrap_or_default();
                if !((mp(back, &t.keep_count)
                    || mr(back_text, &t.keep_count2)
                    || mp(back, &t.adverbis_acceptats))
                    && !mr(back_text, &t.stop_count))
                {
                    break;
                }
                if mp(back, &t.preposicions) {
                    j += k;
                    break;
                }
                k += 1;
            }
        }
        if let Some(current) = token_at(i - j) {
            update_apparitions_es(current, &mut apparitions, t);
        }
        j += 1;
    }

    level += 1;
    if level > MAX_LEVELS {
        level = MAX_LEVELS;
    }
    let mut c_n = [0u32; MAX_LEVELS];
    let mut c_d = [0u32; MAX_LEVELS];
    let mut c_ntotal = 0u32;
    let mut c_dtotal = 0u32;
    for jj in 0..level {
        c_n[jj] = lv.nms[jj]
            + lv.nfs[jj]
            + lv.nmp[jj]
            + lv.nfp[jj]
            + lv.ncs[jj]
            + lv.ncp[jj]
            + lv.nmn[jj];
        c_d[jj] = lv.dms[jj] + lv.dfs[jj] + lv.dmp[jj] + lv.dfp[jj];
        c_ntotal += c_n[jj];
        c_dtotal += c_d[jj];
        if mp(adjective, &t.adjectiu_mp)
            && (c_n[jj] > 1 || c_d[jj] > 1)
            && (lv.nms[jj]
                + lv.nmn[jj]
                + lv.nmp[jj]
                + lv.ncs[jj]
                + lv.ncp[jj]
                + lv.dms[jj]
                + lv.dmp[jj])
                > 0
            && lv.nfs[jj] + lv.nfp[jj] <= lv.nt[jj]
        {
            return None;
        }
        if mp(adjective, &t.adjectiu_fp)
            && (c_n[jj] > 1 || c_d[jj] > 1)
            && ((lv.nms[jj] + lv.nmp[jj] + lv.nmn[jj] + lv.dms[jj] + lv.dmp[jj]) == 0
                || (lv.nt[jj] > 0 && lv.nfs[jj] + lv.nfp[jj] >= lv.nt[jj]))
        {
            return None;
        }
        if c_n[jj] + c_d[jj] > 0 {
            is_plural = is_plural && c_d[jj] > 1;
            can_be_p = can_be_p || c_n[jj] > 1;
        }
    }
    is_plural = is_plural
        || (i > 2
            && lv.nmp[0] + lv.nfp[0] + lv.ncp[0] > 0
            && token_at(i - 2).map(|c| c.token().token()) == Some(","));

    if c_ntotal == 0 && c_dtotal == 0 {
        return None;
    }

    let (subst, adj, gn): (&Regex, &Regex, &Regex) = if mp(adjective, &t.adjectiu_cs) {
        (&t.gn_cs, &t.adjectiu_s, &t.gn_cs_tag)
    } else if mp(adjective, &t.adjectiu_cp) {
        (&t.gn_cp, &t.adjectiu_p, &t.gn_cp_tag)
    } else if mp(adjective, &t.adjectiu_ms) {
        (&t.gn_ms, &t.adjectiu_ms, &t.gn_ms_tag)
    } else if mp(adjective, &t.adjectiu_fs) {
        (&t.gn_fs, &t.adjectiu_fs, &t.gn_fs_tag)
    } else if mp(adjective, &t.adjectiu_mp) {
        (&t.gn_mp, &t.adjectiu_mp, &t.gn_mp_tag)
    } else if mp(adjective, &t.adjectiu_fp) {
        (&t.gn_fp, &t.adjectiu_fp, &t.gn_fp_tag)
    } else {
        return None;
    };

    let mut j = 1usize;
    let mut keep = true;
    while i > j && keep {
        let cur = i - j;
        let current = token_at(cur);
        if mp(current, &t.nom_det) && mp(current, gn) {
            return None;
        } else if !mp(current, &t.gn_any) && mp(current, subst) {
            return None;
        }
        keep = !mp(current, &t.nom_det);
        j += 1;
    }

    let prev = token_at(i - 1);
    let necessary = (mp(prev, &t.nom) && !mp(prev, subst))
        || (mp(prev, &t.adjectiu) && !mp(prev, gn))
        || (mp(prev, &t.adjectiu) && !mp(prev, adj))
        || (i > 2
            && mp(prev, &t.adverbis_acceptats)
            && !mp(token_at(i - 2), &t.verb)
            && !mp(token_at(i - 2), &t.preposicions))
        || (i > 3
            && mp(prev, &t.loc_adv)
            && mp(token_at(i - 2), &t.loc_adv)
            && !mp(token_at(i - 3), &t.verb)
            && !mp(token_at(i - 3), &t.preposicions));
    if !necessary {
        return None;
    }

    if !(is_plural && mp(adjective, &t.adjectiu_s)) {
        let mut j = 1usize;
        apparitions = Apparitions::default();
        while i > j && keep_counting_es(token_at(i - j), &apparitions, t) && (level > 1 || j < 4) {
            let cur = i - j;
            let current = token_at(cur);
            if !mp(current, &t.gn_any) && mp(current, &t.nom_det) && mp(current, subst) {
                return None;
            } else if mp(current, gn) {
                return None;
            }
            if let Some(current) = current {
                update_apparitions_es(current, &mut apparitions, t);
            }
            j += 1;
        }
    }

    let Some(synth) = synthesizer else {
        return Some(rm);
    };
    let Some(adjective) = adjective else {
        return Some(rm);
    };
    let mut suggestions: Vec<String> = Vec::new();
    if let Some(lemma) = matching_reading(adjective, &t.adjectiu_cs).and_then(|r| r.lemma()) {
        suggestions.extend(synth.synthesize(lemma, &t.synth_cs));
    }
    if suggestions.is_empty()
        && let Some(lemma) = matching_reading(adjective, &t.adjectiu_cp).and_then(|r| r.lemma())
    {
        suggestions.extend(synth.synthesize(lemma, &t.synth_cp));
    }
    if suggestions.is_empty()
        && is_plural
        && let Some(lemma) = matching_reading(adjective, &t.adjectiu_p).and_then(|r| r.lemma())
    {
        suggestions.extend(synth.synthesize(lemma, &t.synth_common_plural));
    }
    if suggestions.is_empty()
        && let Some(at) = matching_reading(adjective, &t.adjectiu)
        && let Some(lemma) = at.lemma()
    {
        if can_be_ms && !is_plural {
            suggestions.extend(synth.synthesize(lemma, &t.synth_ms));
        }
        if can_be_fs && !is_plural {
            suggestions.extend(synth.synthesize(lemma, &t.synth_fs));
        }
        if can_be_mp {
            suggestions.extend(synth.synthesize(lemma, &t.synth_mp));
        }
        if can_be_fp {
            suggestions.extend(synth.synthesize(lemma, &t.synth_fp));
        }
        if can_be_ms && (is_plural || can_be_p) {
            suggestions.extend(synth.synthesize(lemma, &t.synth_mp));
        }
        if can_be_fs && !can_be_ms && (is_plural || can_be_p) {
            suggestions.extend(synth.synthesize(lemma, &t.synth_fp));
        }
    }
    let surface = adjective.token().token();
    let lower = surface.to_lowercase();
    if let Some(pos) = suggestions.iter().position(|s| *s == lower) {
        suggestions.remove(pos);
    }
    if suggestions.is_empty() {
        return None;
    }
    let definitive = if add_comma {
        rm.set_offset_length(rm.offset().saturating_sub(1), rm.length() + 1);
        let mut out = Vec::with_capacity(suggestions.len() + 1);
        out.push(format!(", {surface}"));
        out.extend(suggestions.iter().map(|s| format!(" {s}")));
        out
    } else {
        suggestions
    };
    rm.set_replacements(
        dedup_preserving_order(definitive)
            .into_iter()
            .map(SuggestedReplacement::new)
            .collect(),
    );
    Some(rm)
}

fn keep_counting_es(
    token: Option<&AnalyzedTokenReadings>,
    apparitions: &Apparitions,
    t: &TagSet,
) -> bool {
    let Some(a) = token else {
        return false;
    };
    let text = a.token().token();
    if (apparitions.adverb && apparitions.conjunction)
        || (apparitions.adverb && apparitions.punctuation)
        || (apparitions.conjunction && apparitions.punctuation)
        || (apparitions.punctuation && mp(Some(a), &t.puntuacio))
    {
        return false;
    }
    (mp(Some(a), &t.keep_count) || mr(text, &t.keep_count2) || mp(Some(a), &t.adverbis_acceptats))
        && !mr(text, &t.stop_count)
        && (!mp(Some(a), &t.gv) || mp(Some(a), &t.gn_any))
}

fn update_apparitions_es(token: &AnalyzedTokenReadings, apparitions: &mut Apparitions, t: &TagSet) {
    if mp(Some(token), &t.nom) || mp(Some(token), &t.adjectiu) {
        *apparitions = Apparitions::default();
        return;
    }
    apparitions.adverb |= mp(Some(token), &t.adverbi);
    apparitions.conjunction |= mp(Some(token), &t.conjuncio);
    apparitions.punctuation |= mp(Some(token), &t.puntuacio) || token.token().token() == ",";
}

pub(crate) fn accept_fr(
    synthesizer: Option<&LangSynthesizer>,
    _args: &str,
    matched_positions: &[usize],
    tokens: &[&AnalyzedTokenReadings],
    mut rm: RuleMatch,
) -> Option<RuleMatch> {
    let t = fr_tags();
    let b = &t.base;
    let i = *matched_positions.first()?;
    let token_at = |idx: usize| tokens.get(idx).copied();
    let adjective = token_at(i);

    let mut is_plural = true;
    let mut is_prev_noun = false;
    let mut can_be_ms = false;
    let mut can_be_fs = false;
    let mut can_be_mp = false;
    let mut can_be_fp = false;
    let mut can_be_p = false;
    let mut lv = Levels::default();
    let mut apparitions = Apparitions::default();
    let mut level = 0usize;
    let mut j = 1usize;

    while i > j && keep_counting_fr(token_at(i - j), &apparitions, t) && level < MAX_LEVELS {
        let cur = i - j;
        let current = token_at(cur);
        if !is_prev_noun {
            if mp(current, &b.nom)
                || (cur > 1
                    && !mp(current, &b.nom)
                    && mp(current, &b.adjectiu)
                    && mp(token_at(cur - 1), &b.det))
            {
                if mp(current, &b.gn_ms_tag) {
                    lv.nms[level] += 1;
                    can_be_ms = true;
                }
                if mp(current, &b.gn_fs_tag) {
                    lv.nfs[level] += 1;
                    can_be_fs = true;
                }
                if mp(current, &b.gn_mp_tag) {
                    lv.nmp[level] += 1;
                    can_be_mp = true;
                }
                if mp(current, &b.gn_fp_tag) {
                    lv.nfp[level] += 1;
                    can_be_fp = true;
                }
            }
            if !mp(current, &b.gn_any) {
                if mp(current, &b.nom_ms) {
                    lv.nms[level] += 1;
                    can_be_ms = true;
                } else if mp(current, &b.nom_fs) {
                    lv.nfs[level] += 1;
                    can_be_fs = true;
                } else if mp(current, &b.nom_mp) {
                    lv.nmp[level] += 1;
                    can_be_mp = true;
                } else if mp(current, &b.nom_mn) {
                    lv.nmn[level] += 1;
                    can_be_ms = true;
                    can_be_mp = true;
                } else if mp(current, &b.nom_fp) {
                    lv.nfp[level] += 1;
                    can_be_fp = true;
                } else if mp(current, &b.nom_cs) {
                    lv.ncs[level] += 1;
                    can_be_ms = true;
                    can_be_fs = true;
                } else if mp(current, &b.nom_cp) {
                    lv.ncp[level] += 1;
                    can_be_fp = true;
                    can_be_mp = true;
                }
            }
        }
        if mp(current, &b.nom) {
            lv.nt[level] += 1;
            is_prev_noun = true;
        } else {
            is_prev_noun = false;
        }
        if mp(current, &b.det_cs) {
            if mp(token_at(cur + 1), &b.nom_ms) {
                lv.dms[level] += 1;
                can_be_ms = true;
            }
            if mp(token_at(cur + 1), &b.nom_fs) {
                lv.dfs[level] += 1;
                can_be_fs = true;
            }
        }
        if mp(current, &t.det_cp) {
            if mp(token_at(cur + 1), &b.nom_mp) {
                lv.dms[level] += 1;
                can_be_mp = true;
            }
            if mp(token_at(cur + 1), &b.nom_fp) {
                lv.dfs[level] += 1;
                can_be_fp = true;
            }
        }
        if !mp(current, &b.adverbi) {
            if mp(current, &b.det_ms) {
                lv.dms[level] += 1;
                can_be_ms = true;
            }
            if mp(current, &b.det_fs) {
                lv.dfs[level] += 1;
                can_be_fs = true;
            }
            if mp(current, &b.det_mp) {
                lv.dmp[level] += 1;
                can_be_mp = true;
            }
            if mp(current, &b.det_fp) {
                lv.dfp[level] += 1;
                can_be_fp = true;
            }
        }
        if i > j + 1 {
            let text = current.map(|c| c.token().token()).unwrap_or_default();
            if mr(text, &b.prepo_nivell)
                && mp(current, &b.preposicions)
                && !mp(current, &b.conjuncio)
                && !mr(
                    token_at(cur - 1)
                        .map(|c| c.token().token())
                        .unwrap_or_default(),
                    &b.coord_ioni,
                )
                && !mp(token_at(cur + 1), &b.adverbi)
            {
                level += 1;
            } else if text.eq_ignore_ascii_case("d'")
                && token_at(cur + 1)
                    .map(|c| c.token().token().eq_ignore_ascii_case("environ"))
                    .unwrap_or(false)
            {
                level += 1;
            }
        }
        j = update_j_value_fr(tokens, i, j, t);
        if let Some(current) = token_at(i - j) {
            update_apparitions_fr(current, &mut apparitions, t);
        }
        j += 1;
    }

    level += 1;
    if level > MAX_LEVELS {
        level = MAX_LEVELS;
    }
    let mut c_n = [0u32; MAX_LEVELS];
    let mut c_d = [0u32; MAX_LEVELS];
    let mut c_ntotal = 0u32;
    let mut c_dtotal = 0u32;
    for jj in 0..level {
        c_n[jj] = lv.nms[jj]
            + lv.nfs[jj]
            + lv.nmp[jj]
            + lv.nfp[jj]
            + lv.ncs[jj]
            + lv.ncp[jj]
            + lv.nmn[jj];
        c_d[jj] = lv.dms[jj] + lv.dfs[jj] + lv.dmp[jj] + lv.dfp[jj];
        c_ntotal += c_n[jj];
        c_dtotal += c_d[jj];
        if mp(adjective, &b.adjectiu_mp)
            && (c_n[jj] > 1 || c_d[jj] > 1)
            && (lv.nms[jj]
                + lv.nmn[jj]
                + lv.nmp[jj]
                + lv.ncs[jj]
                + lv.ncp[jj]
                + lv.dms[jj]
                + lv.dmp[jj])
                > 0
            && lv.nfs[jj] + lv.nfp[jj] <= lv.nt[jj]
        {
            return None;
        }
        if mp(adjective, &b.adjectiu_fp)
            && (c_n[jj] > 1 || c_d[jj] > 1)
            && ((lv.nms[jj] + lv.nmp[jj] + lv.nmn[jj] + lv.dms[jj] + lv.dmp[jj]) == 0
                || (lv.nt[jj] > 0 && lv.nfs[jj] + lv.nfp[jj] >= lv.nt[jj]))
        {
            return None;
        }
        if c_n[jj] + c_d[jj] > 0 {
            is_plural = is_plural && c_d[jj] > 1 && level > 1;
            can_be_p = can_be_p || c_n[jj] > 1;
        }
    }
    is_plural = is_plural
        || (i > 2
            && lv.nmp[0] + lv.nfp[0] + lv.ncp[0] > 0
            && token_at(i - 2).map(|c| c.token().token()) == Some(","));

    if c_ntotal == 0 && c_dtotal == 0 {
        return None;
    }

    let (subst, adj, gn): (&Regex, &Regex, &Regex) = if mp(adjective, &b.adjectiu_cs) {
        (&b.gn_cs, &b.adjectiu_s, &b.gn_cs_tag)
    } else if mp(adjective, &b.adjectiu_cp) {
        (&b.gn_cp, &b.adjectiu_p, &b.gn_cp_tag)
    } else if mp(adjective, &t.adjectiu_mn) {
        (&t.gn_mn, &t.adjectiu_m, &t.gn_mn_tag)
    } else if mp(adjective, &t.adjectiu_fn) {
        (&t.gn_fn, &t.adjectiu_fn, &t.gn_fn_tag)
    } else if mp(adjective, &b.adjectiu_ms) {
        (&b.gn_ms, &b.adjectiu_ms, &b.gn_ms_tag)
    } else if mp(adjective, &b.adjectiu_fs) {
        (&b.gn_fs, &b.adjectiu_fs, &b.gn_fs_tag)
    } else if mp(adjective, &b.adjectiu_mp) {
        (&b.gn_mp, &b.adjectiu_mp, &b.gn_mp_tag)
    } else if mp(adjective, &b.adjectiu_fp) {
        (&b.gn_fp, &b.adjectiu_fp, &b.gn_fp_tag)
    } else {
        return None;
    };

    let mut j = 1usize;
    let mut keep = true;
    while i > j && keep {
        let cur = i - j;
        let current = token_at(cur);
        if mp(current, &b.nom_det) && mp(current, gn) {
            return None;
        } else if !mp(current, &b.gn_any) && mp(current, subst) {
            return None;
        }
        keep = !mp(current, &b.nom_det);
        j += 1;
    }

    let prev = token_at(i - 1);
    let necessary = (mp(prev, &b.nom) && !mp(prev, subst))
        || (mp(prev, &b.gn_any) && !mp(prev, gn))
        || (mp(prev, &b.adjectiu) && !mp(prev, adj))
        || (i > 2
            && mp(prev, &b.adverbis_acceptats)
            && !mp(token_at(i - 2), &b.verb)
            && !mp(token_at(i - 2), &b.preposicions))
        || (i > 3
            && mp(prev, &b.loc_adv)
            && mp(token_at(i - 2), &b.loc_adv)
            && !mp(token_at(i - 3), &b.verb)
            && !mp(token_at(i - 3), &b.preposicions));
    if !necessary {
        return None;
    }

    if !(is_plural && mp(adjective, &b.adjectiu_s)) {
        let mut j = 1usize;
        apparitions = Apparitions::default();
        while i > j && keep_counting_fr(token_at(i - j), &apparitions, t) && (level > 1 || j < 4) {
            let cur = i - j;
            let current = token_at(cur);
            if !mp(current, &b.gn_any) && mp(current, &b.nom_det) && mp(current, subst) {
                return None;
            } else if mp(current, gn) {
                return None;
            }
            j = update_j_value_fr(tokens, i, j, t);
            if let Some(current) = token_at(i - j) {
                update_apparitions_fr(current, &mut apparitions, t);
            }
            j += 1;
        }
    }

    let Some(synth) = synthesizer else {
        return Some(rm);
    };
    let adjective = adjective?;
    let mut suggestions: Vec<String> = Vec::new();
    if let Some(lemma) = matching_reading(adjective, &b.adjectiu_cs).and_then(|r| r.lemma()) {
        suggestions.extend(synth.synthesize(lemma, &b.synth_cs));
    }
    if suggestions.is_empty()
        && let Some(lemma) = matching_reading(adjective, &b.adjectiu_cp).and_then(|r| r.lemma())
    {
        suggestions.extend(synth.synthesize(lemma, &b.synth_cp));
    }
    if suggestions.is_empty()
        && is_plural
        && let Some(lemma) = matching_reading(adjective, &b.adjectiu_p).and_then(|r| r.lemma())
    {
        suggestions.extend(synth.synthesize(lemma, &b.synth_common_plural));
    }
    if suggestions.is_empty()
        && let Some(at) = matching_reading(adjective, &b.adjectiu)
        && let Some(lemma) = at.lemma()
    {
        if can_be_ms && !is_plural {
            suggestions.extend(synth.synthesize(lemma, &b.synth_ms));
        }
        if can_be_fs && !is_plural {
            suggestions.extend(synth.synthesize(lemma, &b.synth_fs));
        }
        if can_be_mp {
            suggestions.extend(synth.synthesize(lemma, &b.synth_mp));
        }
        if can_be_fp {
            suggestions.extend(synth.synthesize(lemma, &b.synth_fp));
        }
        if can_be_ms && (is_plural || can_be_p) {
            suggestions.extend(synth.synthesize(lemma, &b.synth_mp));
        }
        if can_be_fs && !can_be_ms && (is_plural || can_be_p) {
            suggestions.extend(synth.synthesize(lemma, &b.synth_fp));
        }
    }
    suggestions = dedup_preserving_order(suggestions);
    let lower = adjective.token().token().to_lowercase();
    if let Some(pos) = suggestions.iter().position(|s| *s == lower) {
        suggestions.remove(pos);
    }
    if suggestions.is_empty() {
        return None;
    }
    rm.set_replacements(
        suggestions
            .into_iter()
            .map(SuggestedReplacement::new)
            .collect(),
    );
    Some(rm)
}

fn update_j_value_fr(tokens: &[&AnalyzedTokenReadings], i: usize, j: usize, t: &FrTagSet) -> usize {
    let cur = i - j;
    let Some(current) = tokens.get(cur).copied() else {
        return j;
    };
    if mr(current.token().token(), &t.base.coord_ioni)
        && cur > 1
        && cur + 1 < tokens.len()
        && mp(tokens.get(cur - 1).copied(), &t.base.det)
        && tokens[cur + 1].token().token() == "plus"
    {
        return j + 1;
    }
    j
}

fn keep_counting_fr(
    token: Option<&AnalyzedTokenReadings>,
    apparitions: &Apparitions,
    t: &FrTagSet,
) -> bool {
    let Some(a) = token else {
        return false;
    };
    let b = &t.base;
    let text = a.token().token();
    if mr(text, &b.prepo_nivell) {
        return true;
    }
    if text == "." {
        return true;
    }
    if (apparitions.adverb && apparitions.conjunction)
        || (apparitions.adverb && apparitions.punctuation)
        || (apparitions.conjunction && apparitions.punctuation)
        || (apparitions.punctuation && mp(Some(a), &b.puntuacio))
        || (apparitions.infinitive && mr(text, &b.coord_ioni))
        || (apparitions.infinitive && apparitions.adverb)
    {
        return false;
    }
    (mp(Some(a), &b.keep_count) || mr(text, &b.keep_count2) || mp(Some(a), &b.adverbis_acceptats))
        && !mr(text, &b.stop_count)
        && (!mp(Some(a), &b.gv) || mp(Some(a), &b.gn_any))
}

fn update_apparitions_fr(
    token: &AnalyzedTokenReadings,
    apparitions: &mut Apparitions,
    t: &FrTagSet,
) {
    let b = &t.base;
    apparitions.conjunction |= mp(Some(token), &b.conjuncio);
    if token.token().token() == "com" {
        return;
    }
    if mp(Some(token), &b.nom) || mp(Some(token), &b.adjectiu) {
        *apparitions = Apparitions::default();
        return;
    }
    apparitions.adverb |= mp(Some(token), &b.adverbi);
    apparitions.punctuation |= mp(Some(token), &b.puntuacio) || token.token().token() == ",";
    apparitions.infinitive |= mp(Some(token), &t.infinitive);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(text: &str, tags: &[&str]) -> AnalyzedTokenReadings {
        let readings = if tags.is_empty() {
            vec![AnalyzedToken::new(text, 0, text.len())]
        } else {
            tags.iter()
                .map(|t| AnalyzedToken::new(text, 0, text.len()).with_pos_tags(vec![t.to_string()]))
                .collect()
        };
        AnalyzedTokenReadings::new(AnalyzedToken::new(text, 0, text.len())).with_readings(readings)
    }

    #[test]
    fn es_patterns_compile_and_match() {
        let t = es_tags();
        assert!(mp(Some(&tag("casa", &["NCFS000"])), &t.nom));
        assert!(mp(Some(&tag("casa", &["NCFS000"])), &t.nom_fs));
        assert!(mp(Some(&tag("blancas", &["AQ0FP0"])), &t.adjectiu_fp));
        assert!(mp(Some(&tag("del", &["SP+DA"])), &t.preposicions));
        assert!(mp(Some(&tag("x", &[])), &t.keep_count));
        assert!(mr("del", &t.prepo_nivell));
        assert!(!mr("el", &t.prepo_nivell));
        assert!(mr("ni", &t.coord_ioni));
        assert!(mp(Some(&tag("x", &["_GN_MS"])), &t.gn_any));
    }

    #[test]
    fn fr_patterns_compile_and_match() {
        let t = fr_tags();
        let b = &t.base;
        assert!(mp(Some(&tag("maison", &["N f s"])), &b.nom));
        assert!(mp(Some(&tag("jolies", &["J f p"])), &b.adjectiu));
        assert!(mp(Some(&tag("jolies", &["J f p"])), &b.adjectiu_fp));
        assert!(mp(Some(&tag("joli", &["J m sp"])), &t.adjectiu_mn));
        assert!(mp(Some(&tag("les", &["D e p"])), &t.det_cp));
        assert!(mp(Some(&tag("manger", &["V inf"])), &t.infinitive));
        assert!(mp(Some(&tag("manger", &["V inf"])), &b.verb));
        assert!(!mp(Some(&tag("joli", &["V ppa m s"])), &b.verb));
        assert!(mr("et", &b.coord_ioni));
        assert!(mr("d'", &b.prepo_nivell));
        assert!(mr(";", &b.stop_count));
        assert!(mr(":", &b.stop_count));
        assert!(mr("–", &b.stop_count));
        assert!(!mr(".", &b.stop_count));
    }

    #[test]
    fn fr_exception_filtering() {
        assert!(is_exception_fr("qqch"));
        assert!(is_exception_fr("manifestè"));
        assert!(!is_exception_fr("burkinabè"));
        assert!(!is_exception_fr("épistémè"));
        assert!(!is_exception_fr("jolis"));
    }

    #[test]
    fn manual_parsing_supports_declared_separator() {
        let entries = parse_manual("#separatorRegExp=;\nQuick;Quick;Z m sp\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.as_ref(), "Quick");
        assert_eq!(entries[0].1.as_ref(), "Z m sp");
        assert_eq!(entries[0].2.as_ref(), "Quick");
    }

    #[test]
    fn arg_parsing_requires_exact_key() {
        assert!(arg_is_true("addComma:true", "addComma"));
        assert!(arg_is_true("key:value addComma:TRUE", "addComma"));
        assert!(!arg_is_true("key:value", "addComma"));
        assert!(!arg_is_true("addComma:false", "addComma"));
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let out = dedup_preserving_order(vec!["a".into(), "b".into(), "a".into()]);
        assert_eq!(out, vec!["a".to_string(), "b".to_string()]);
    }
}
