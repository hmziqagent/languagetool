//! Hunspell `.dic` / `.aff` dictionary support (Phase 6 gap).
//!
//! This is a pragmatic Hunspell subset, not a full reimplementation of
//! `hunspell`'s morphological engine:
//!
//! * `.dic` entries with `/FLAG` suffixes and tab-separated morphological
//!   descriptors (ignored beyond the flags).
//! * `.aff` `FLAG` modes (`ASCII` default, `LONG`, `NUM`), `COMPOUNDMIN`,
//!   `COMPOUNDFLAG`, `COMPOUNDBEGIN`/`COMPOUNDMIDDLE`/`COMPOUNDEND`,
//!   `ONLYINCOMPOUND`, `PFX`/`SFX` rules with simple conditions
//!   (literal chars, `.`, `[...]`, `[^...]` with `-` ranges).
//! * Lookup performs depth-2 affix checking (one prefix and/or one suffix
//!   applied on top of a dictionary stem), which covers the common
//!   inflections. Recursive continuation classes (`/` flags on affix `add`
//!   fields) are not expanded.
//! * Compound acceptance supports 2- and 3-part splits where every part is at
//!   least `COMPOUNDMIN` characters and carries the compound flags.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum HunspellError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Malformed aff file: {0}")]
    MalformedAff(String),
    #[error("Malformed dic file: {0}")]
    MalformedDic(String),
}

/// How flags are encoded in the `.aff`/`.dic` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum FlagMode {
    /// One-byte flags (the hunspell default, `FLAG` unset or `FLAG ASCII`).
    #[default]
    Ascii,
    /// Two-byte flags (`FLAG LONG`).
    Long,
    /// Comma-separated numeric flags (`FLAG NUM`).
    Num,
}

/// A parsed affix condition. Hunspell conditions are a small regex dialect;
/// we support the overwhelmingly common subset: concatenated atoms of a
/// literal char, `.` (any), `[...]`, and `[^...]`, where classes may contain
/// `-` ranges. Anything we fail to parse degrades to `Always`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Condition {
    Always,
    Atoms(Vec<CondAtom>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CondAtom {
    Any,
    Literal(char),
    Class {
        negated: bool,
        chars: Vec<(char, char)>,
    },
}

impl Condition {
    fn parse(spec: &str) -> Condition {
        if spec == "." || spec.is_empty() {
            return Condition::Always;
        }
        let chars: Vec<char> = spec.chars().collect();
        let mut atoms = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '.' => atoms.push(CondAtom::Any),
                '[' => {
                    let mut j = i + 1;
                    let negated = j < chars.len() && chars[j] == '^';
                    if negated {
                        j += 1;
                    }
                    let mut ranges = Vec::new();
                    let mut start: Option<char> = None;
                    let mut closed = false;
                    while j < chars.len() {
                        match chars[j] {
                            ']' => {
                                if let Some(c) = start.take() {
                                    ranges.push((c, c));
                                }
                                closed = true;
                                break;
                            }
                            '-' if start.is_some() && j + 1 < chars.len() => {
                                let from = start.take().expect("checked is_some");
                                j += 1;
                                let to = chars[j];
                                ranges.push((from, to));
                            }
                            c => {
                                if let Some(prev) = start.take() {
                                    ranges.push((prev, prev));
                                }
                                start = Some(c);
                            }
                        }
                        j += 1;
                    }
                    if !closed {
                        return Condition::Always;
                    }
                    atoms.push(CondAtom::Class {
                        negated,
                        chars: ranges,
                    });
                    i = j;
                }
                c => atoms.push(CondAtom::Literal(c)),
            }
            i += 1;
        }
        Condition::Atoms(atoms)
    }

    /// Match the condition against a char slice. Hunspell conditions are
    /// anchored: prefix rules match at the start of the stem, suffix rules
    /// at its end, and the condition must consume the whole matched window.
    fn matches_anchored(&self, text: &[char], at_start: bool) -> bool {
        match self {
            Condition::Always => true,
            Condition::Atoms(atoms) => {
                if text.len() < atoms.len() {
                    return false;
                }
                let window: Vec<char> = if at_start {
                    text[..atoms.len()].to_vec()
                } else {
                    text[text.len() - atoms.len()..].to_vec()
                };
                atoms
                    .iter()
                    .zip(window.iter())
                    .all(|(atom, &c)| atom.matches(c))
            }
        }
    }
}

impl CondAtom {
    fn matches(&self, c: char) -> bool {
        match self {
            CondAtom::Any => true,
            CondAtom::Literal(l) => *l == c,
            CondAtom::Class { negated, chars } => {
                let inside = chars.iter().any(|&(a, b)| c >= a && c <= b);
                inside != *negated
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AffixKind {
    Prefix,
    Suffix,
}

/// One PFX/SFX rule line.
#[derive(Debug, Clone)]
struct AffixRule {
    kind: AffixKind,
    /// Canonical (parsed) flag owning this rule.
    flag: String,
    /// Chars to strip from the stem ("0" = none).
    strip: String,
    /// Chars to append (before any `/` continuation flags).
    add: String,
    condition: Condition,
}

/// Parsed `.aff` data.
#[derive(Debug, Default)]
pub struct HunspellAff {
    flag_mode: FlagMode,
    compound_min: usize,
    compound_flag: Option<String>,
    compound_begin: Option<String>,
    compound_middle: Option<String>,
    compound_end: Option<String>,
    only_in_compound: HashSet<String>,
    rules: Vec<AffixRule>,
}

impl HunspellAff {
    /// Parse an `.aff` file from its text.
    pub fn parse(text: &str) -> Result<Self, HunspellError> {
        let mut aff = HunspellAff {
            compound_min: 3, // hunspell default
            ..HunspellAff::default()
        };

        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim_end();
            if line.trim().is_empty() {
                continue;
            }
            let mut tokens = line.split_whitespace();
            let Some(key) = tokens.next() else { continue };
            match key {
                "FLAG" => {
                    if let Some(mode) = tokens.next() {
                        aff.flag_mode = match mode {
                            "LONG" => FlagMode::Long,
                            "NUM" => FlagMode::Num,
                            _ => FlagMode::Ascii,
                        };
                    }
                }
                "COMPOUNDMIN" => {
                    if let Some(v) = tokens.next().and_then(|t| t.parse().ok()) {
                        aff.compound_min = v;
                    }
                }
                "COMPOUNDFLAG" => {
                    aff.compound_flag = tokens.next().map(|f| aff.canonical_flag_token(f));
                }
                "COMPOUNDBEGIN" => {
                    aff.compound_begin = tokens.next().map(|f| aff.canonical_flag_token(f));
                }
                "COMPOUNDMIDDLE" => {
                    aff.compound_middle = tokens.next().map(|f| aff.canonical_flag_token(f));
                }
                "COMPOUNDEND" => {
                    aff.compound_end = tokens.next().map(|f| aff.canonical_flag_token(f));
                }
                "ONLYINCOMPOUND" => {
                    if let Some(f) = tokens.next() {
                        aff.only_in_compound.extend(aff.parse_flags(f));
                    }
                }
                "PFX" | "SFX" => {
                    // Rule line (headers "PFX F Y 35" are recognized by having
                    // a non-strip 3rd token; rule lines are "PFX F strip add cond").
                    let kind = if key == "PFX" {
                        AffixKind::Prefix
                    } else {
                        AffixKind::Suffix
                    };
                    let flag = match tokens.next() {
                        Some(f) => aff.canonical_flag_token(f),
                        None => return Err(HunspellError::MalformedAff(line.to_string())),
                    };
                    let Some(strip_tok) = tokens.next() else {
                        return Err(HunspellError::MalformedAff(line.to_string()));
                    };
                    // Header lines: strip token is Y/N and a count follows.
                    if matches!(strip_tok, "Y" | "N") && tokens.next().is_some() {
                        continue;
                    }
                    let Some(add_tok) = tokens.next() else {
                        return Err(HunspellError::MalformedAff(line.to_string()));
                    };
                    let add = add_tok.split('/').next().unwrap_or("");
                    let cond = tokens.next().unwrap_or(".");
                    aff.rules.push(AffixRule {
                        kind,
                        flag,
                        strip: if strip_tok == "0" {
                            String::new()
                        } else {
                            strip_tok.to_string()
                        },
                        add: if add == "0" {
                            String::new()
                        } else {
                            add.to_string()
                        },
                        condition: Condition::parse(cond),
                    });
                }
                _ => {}
            }
        }
        Ok(aff)
    }

    /// Load an `.aff` file from disk.
    pub fn from_file(path: &Path) -> Result<Self, HunspellError> {
        Self::parse(&fs::read_to_string(path)?)
    }

    /// Convert a raw flag token from the .aff file into its canonical
    /// representation (used both for rule ownership and dictionary flags).
    fn canonical_flag_token(&self, token: &str) -> String {
        self.parse_flags(token).join(",")
    }

    /// Parse a raw flag string (from `.dic` word/FLAGS or an `.aff` flag
    /// field) into canonical flag strings according to the FLAG mode.
    fn parse_flags(&self, raw: &str) -> Vec<String> {
        match self.flag_mode {
            FlagMode::Ascii => raw.chars().map(|c| c.to_string()).collect(),
            FlagMode::Long => raw
                .as_bytes()
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| String::from_utf8_lossy(c).into_owned())
                .collect(),
            FlagMode::Num => raw.split(',').map(str::to_string).collect(),
        }
    }
}

/// A Hunspell dictionary: stems with their flags plus the affix data needed
/// for lookup.
#[derive(Debug)]
pub struct HunspellDictionary {
    /// stem -> canonical flag set
    entries: HashMap<String, Vec<String>>,
    aff: HunspellAff,
}

impl HunspellDictionary {
    /// Load a dictionary from `.dic` and `.aff` files.
    pub fn open(dic_path: &Path, aff_path: &Path) -> Result<Self, HunspellError> {
        let aff = HunspellAff::from_file(aff_path)?;
        let dic_text = fs::read_to_string(dic_path)?;
        Self::parse_dic(&dic_text, aff)
    }

    fn parse_dic(text: &str, aff: HunspellAff) -> Result<Self, HunspellError> {
        let mut entries: HashMap<String, Vec<String>> = HashMap::new();
        for (idx, raw) in text.lines().enumerate() {
            if idx == 0 && raw.trim().chars().all(|c| c.is_ascii_digit()) {
                continue; // declared entry count
            }
            let line = raw.split('\t').next().unwrap_or("");
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (word, flags) = match line.split_once('/') {
                Some((w, f)) => (w, aff.parse_flags(f)),
                None => (line, Vec::new()),
            };
            if word.is_empty() {
                continue;
            }
            entries.entry(word.to_string()).or_default().extend(flags);
        }
        Ok(Self { entries, aff })
    }

    /// Number of dictionary stems.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// True when `word` is accepted by the dictionary: a stem, an affixed
    /// form of a stem (one prefix and/or one suffix), or a valid compound.
    pub fn is_known(&self, word: &str) -> bool {
        if word.is_empty() {
            return false;
        }
        if self.is_known_stem(word) {
            return true;
        }
        let lower = word.to_lowercase();
        if lower != word && self.is_known_stem(&lower) {
            return true;
        }
        if self.is_known_affixed(word) || (lower != word && self.is_known_affixed(&lower)) {
            return true;
        }
        self.is_known_compound(word)
    }

    /// Direct stem lookup, rejecting ONLYINCOMPOUND-only stems.
    fn is_known_stem(&self, word: &str) -> bool {
        self.entries
            .get(word)
            .is_some_and(|flags| !self.standalone_forbidden(flags))
    }

    fn standalone_forbidden(&self, flags: &[String]) -> bool {
        !flags.is_empty() && flags.iter().any(|f| self.aff.only_in_compound.contains(f))
    }

    /// Check stem + flags combination for affix application (compound-only
    /// stems are legal here when they carry the affix flag).
    fn stem_accepts(&self, stem: &str, flag: &str) -> bool {
        self.entries
            .get(stem)
            .is_some_and(|flags| flags.iter().any(|f| f == flag))
    }

    fn is_known_affixed(&self, word: &str) -> bool {
        let word_chars: Vec<char> = word.chars().collect();
        // Collect suffix-derivable stems, then check each for a bare match or
        // an additional prefix (depth-2 affix checking).
        for rule in &self.aff.rules {
            if rule.kind != AffixKind::Suffix {
                continue;
            }
            if let Some(stem) = self.apply_rule(&word_chars, rule) {
                if self.stem_accepts(&stem, &rule.flag) {
                    return true;
                }
                if self.any_prefix_applies(&stem) {
                    return true;
                }
            }
        }
        for rule in &self.aff.rules {
            if rule.kind != AffixKind::Prefix {
                continue;
            }
            if let Some(stem) = self.apply_rule(&word_chars, rule) {
                if self.stem_accepts(&stem, &rule.flag) {
                    return true;
                }
                if self.any_suffix_applies(&stem) {
                    return true;
                }
            }
        }
        false
    }

    /// Reverse-apply `rule` to `word`: if `word` could have been produced by
    /// applying the rule to a stem, return that stem.
    fn apply_rule(&self, word: &[char], rule: &AffixRule) -> Option<String> {
        let add: Vec<char> = rule.add.chars().collect();
        match rule.kind {
            AffixKind::Suffix => {
                if word.len() < add.len() || !word.ends_with(&add) {
                    return None;
                }
                let mut stem: Vec<char> = word[..word.len() - add.len()].to_vec();
                // Applying the rule means removing what it added and restoring
                // what it stripped; for suffix rules the strip goes back on
                // the end of the stem.
                let strip: Vec<char> = rule.strip.chars().collect();
                stem.extend(strip);
                if !self.condition_ok(rule, &stem) {
                    return None;
                }
                Some(stem.into_iter().collect())
            }
            AffixKind::Prefix => {
                if word.len() < add.len() || !word.starts_with(&add) {
                    return None;
                }
                let mut stem: Vec<char> = word[add.len()..].to_vec();
                let strip: Vec<char> = rule.strip.chars().collect();
                let mut restored: Vec<char> = strip;
                restored.extend(stem);
                stem = restored;
                if !self.condition_ok(rule, &stem) {
                    return None;
                }
                Some(stem.into_iter().collect())
            }
        }
    }

    /// Conditions apply to the stem after stripping (before the strip is
    /// restored), matched from the relevant edge.
    /// Hunspell affix conditions are matched against the stem *before* the
    /// strip is removed — i.e. against the reconstructed stem — anchored at
    /// the start for prefixes and the end for suffixes.
    fn condition_ok(&self, rule: &AffixRule, stem: &[char]) -> bool {
        rule.condition
            .matches_anchored(stem, rule.kind == AffixKind::Prefix)
    }

    fn any_prefix_applies(&self, stem: &str) -> bool {
        let stem_chars: Vec<char> = stem.chars().collect();
        self.aff.rules.iter().any(|rule| {
            rule.kind == AffixKind::Prefix
                && self
                    .apply_rule(&stem_chars, rule)
                    .is_some_and(|s| self.stem_accepts(&s, &rule.flag))
        })
    }

    fn any_suffix_applies(&self, stem: &str) -> bool {
        let stem_chars: Vec<char> = stem.chars().collect();
        self.aff.rules.iter().any(|rule| {
            rule.kind == AffixKind::Suffix
                && self
                    .apply_rule(&stem_chars, rule)
                    .is_some_and(|s| self.stem_accepts(&s, &rule.flag))
        })
    }

    /// Compound check: 2- or 3-part split, all parts >= COMPOUNDMIN, each
    /// part accepted as a compound member via COMPOUNDFLAG or the
    /// positional COMPOUNDBEGIN/MIDDLE/END flags.
    fn is_known_compound(&self, word: &str) -> bool {
        if self.aff.compound_flag.is_none()
            && self.aff.compound_begin.is_none()
            && self.aff.compound_middle.is_none()
            && self.aff.compound_end.is_none()
        {
            return false;
        }
        let chars: Vec<char> = word.chars().collect();
        let min = self.aff.compound_min;
        if chars.len() < min * 2 {
            return false;
        }
        let two_part = (min..=chars.len() - min).map(|i| (i, None::<usize>));
        let three_part = (min..=chars.len().saturating_sub(2 * min))
            .flat_map(|i| (i + min..=chars.len() - min).map(move |j| (i, Some(j))));
        for (i, j) in two_part.chain(three_part) {
            let parts: Vec<&[char]> = match j {
                Some(j) => vec![&chars[..i], &chars[i..j], &chars[j..]],
                None => vec![&chars[..i], &chars[i..]],
            };
            if parts.iter().enumerate().all(|(idx, part)| {
                let s: String = part.iter().collect();
                self.is_compound_member(&s, idx, parts.len())
            }) {
                return true;
            }
        }
        false
    }

    fn is_compound_member(&self, part: &str, index: usize, total: usize) -> bool {
        let check = |flags: &[String], wanted: &Option<String>| -> bool {
            wanted
                .as_ref()
                .is_some_and(|w| flags.iter().any(|f| f == w))
        };
        let standalone_ok = self.is_known_affixed(part);
        let Some(flags) = self.entries.get(part) else {
            // Not a stem itself; an affixed form of a compound-flagged stem
            // is beyond this subset.
            return standalone_ok;
        };
        if check(flags, &self.aff.compound_flag) {
            return true;
        }
        match (index, total) {
            (0, 2) => check(flags, &self.aff.compound_begin),
            (1, 2) => check(flags, &self.aff.compound_end),
            (0, 3) => check(flags, &self.aff.compound_begin),
            (1, 3) => check(flags, &self.aff.compound_middle),
            (_, 3) => check(flags, &self.aff.compound_end),
            _ => false,
        }
    }
}

impl fmt::Display for HunspellDictionary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HunspellDictionary({} stems)", self.entries.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AFF: &str = "\
# test affix file
COMPOUNDMIN 3
COMPOUNDFLAG C
ONLYINCOMPOUND O
PFX P Y 1
PFX P 0 un .
SFX S Y 2
SFX S y ies [^aeiou]y
SFX S 0 s .
";

    const DIC: &str = "\
5
happy/S
run/CS
read/PSC
dog/CS
blue/OS
";

    fn test_dict() -> HunspellDictionary {
        let aff = HunspellAff::parse(AFF).expect("aff parses");
        HunspellDictionary::parse_dic(DIC, aff).expect("dic parses")
    }

    #[test]
    fn stems_are_known() {
        let dict = test_dict();
        assert!(dict.is_known("happy"));
        assert!(dict.is_known("run"));
        assert!(dict.is_known("read"));
    }

    #[test]
    fn suffixed_forms_are_known() {
        let dict = test_dict();
        // SFX S 0 s . => runs, reads, dogs
        assert!(dict.is_known("runs"));
        assert!(dict.is_known("reads"));
        // SFX S y ies [^aeiou]y => happiest stem "happy" via ies->y
        assert!(dict.is_known("happies"));
    }

    #[test]
    fn prefixed_forms_are_known() {
        let dict = test_dict();
        // PFX P 0 un . on "read" => "unread"
        assert!(dict.is_known("unread"));
    }

    #[test]
    fn prefix_plus_suffix_is_known() {
        let dict = test_dict();
        assert!(dict.is_known("unreads"));
    }

    #[test]
    fn compounds_are_known() {
        let dict = test_dict();
        // dog/C + run/C => "dogrun"
        assert!(dict.is_known("dogrun"));
        // run/C + dog/C
        assert!(dict.is_known("rundog"));
    }

    #[test]
    fn only_in_compound_stems_are_rejected_standalone() {
        let dict = test_dict();
        assert!(
            !dict.is_known("blue"),
            "ONLYINCOMPOUND stem must not be standalone"
        );
    }

    #[test]
    fn unknown_words_are_rejected() {
        let dict = test_dict();
        assert!(!dict.is_known("qqqzzz"));
        assert!(!dict.is_known(""));
        assert!(!dict.is_known("zz"));
    }

    #[test]
    fn flag_mode_long_parses_two_byte_flags() {
        let aff = HunspellAff::parse("FLAG LONG\nSFX AA Y 1\nSFX AA 0 s .\n").unwrap();
        let flags = aff.parse_flags("AB");
        assert_eq!(flags, vec!["AB".to_string()]);
    }

    #[test]
    fn flag_mode_num_parses_comma_flags() {
        let aff = HunspellAff::parse("FLAG NUM\nCOMPOUNDFLAG 7\n").unwrap();
        assert_eq!(aff.compound_flag.as_deref(), Some("7"));
    }

    #[test]
    fn condition_negated_class() {
        let aff = HunspellAff::parse("SFX S Y 1\nSFX S y ies [^aeiou]y\n").unwrap();
        let rule = &aff.rules[0];
        // Condition is checked on the stem (before stripping "y").
        let happy: Vec<char> = "happy".chars().collect();
        assert!(rule.condition.matches_anchored(&happy, false));
        let day: Vec<char> = "day".chars().collect();
        assert!(
            !rule.condition.matches_anchored(&day, false),
            "ay ends in vowel y"
        );
    }

    #[test]
    fn loads_real_german_hunspell_when_root_present() {
        let Some(root) = crate::resource::lt_resource_root() else {
            eprintln!("Skipping: LT resource root not found");
            return;
        };
        let dir = root.join("de/src/main/resources/org/languagetool/resource/de/hunspell");
        let dic = dir.join("de_AT.dic");
        let aff = dir.join("de_AT.aff");
        if !dic.is_file() || !aff.is_file() {
            eprintln!("Skipping: de_AT hunspell files not found");
            return;
        }
        let dict = HunspellDictionary::open(&dic, &aff).expect("de_AT parses");
        assert!(dict.len() > 100_000, "expected a large dictionary");
        for known in ["und", "Haus", "Straße"] {
            assert!(dict.is_known(known), "{known} should be known");
        }
        assert!(!dict.is_known("qqqzzzvvv"));
    }
}
