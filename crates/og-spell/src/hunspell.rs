//! Hunspell `.dic` / `.aff` dictionary support (Phase 6 gap).
//!
//! This is a pragmatic Hunspell subset, not a full reimplementation of
//! `hunspell`'s morphological engine:
//!
//! * `.dic` entries with `/FLAG` suffixes and tab-separated morphological
//!   descriptors (ignored beyond the flags).
//! * `.aff` `FLAG` modes (`ASCII` default, `LONG`, `NUM`), `COMPOUNDMIN`,
//!   `COMPOUNDFLAG`, `COMPOUNDBEGIN`/`COMPOUNDMIDDLE`/`COMPOUNDEND`,
//!   `ONLYINCOMPOUND`, `FORBIDDENWORD`, `NEEDAFFIX`, `KEEPCASE`, `PFX`/`SFX`
//!   rules with simple conditions (literal chars, `.`, `[...]`, `[^...]` with
//!   `-` ranges) and one level of `/` continuation flags on the added affix.
//! * Lookup performs depth-2 affix checking (one prefix AND/OR one suffix
//!   applied on top of a dictionary stem, verifying BOTH rule flags on the
//!   stem and honoring the header cross-product permission), which covers the
//!   common inflections. Deeper recursive continuation expansion is not done.
//! * Compound acceptance supports 2- and 3-part splits where every part is at
//!   least `COMPOUNDMIN` characters and carries the required compound flag —
//!   either directly on the dic stem or via the continuation flags of the
//!   affix that derived the part (how igerman98/de_AT encodes compound
//!   markers). Affixed parts without a compound flag are rejected.
//! * `COMPOUNDPERMITFLAG` is parsed but not enforced; `CIRCUMFIX`,
//!   `COMPOUNDRULE`, `CHECKCOMPOUNDREP` and friends are not implemented.

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
    /// Continuation flags carried by forms derived via this rule (the part
    /// after `/` in the `add` field, e.g. `innen/xyoc` -> x, y, o, c). These
    /// can carry compound markers (COMPOUNDBEGIN/MIDDLE/END) or further
    /// affix classes (only consulted at one level).
    continuation: Vec<String>,
    /// Whether the owning group header allowed cross-products (`Y`).
    cross_product: bool,
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
    forbidden_word: Option<String>,
    need_affix: Option<String>,
    keep_case: Option<String>,
    /// affix group flag -> cross-product permission from the group header.
    group_cross_product: HashMap<String, bool>,
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
                "FORBIDDENWORD" => {
                    aff.forbidden_word = tokens.next().map(|f| aff.canonical_flag_token(f));
                }
                "NEEDAFFIX" => {
                    aff.need_affix = tokens.next().map(|f| aff.canonical_flag_token(f));
                }
                "KEEPCASE" => {
                    aff.keep_case = tokens.next().map(|f| aff.canonical_flag_token(f));
                }
                "COMPOUNDPERMITFLAG" => {
                    // Parsed for documentation; not enforced by this subset.
                    let _ = tokens.next();
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
                        aff.group_cross_product
                            .insert(flag.clone(), strip_tok == "Y");
                        continue;
                    }
                    let Some(add_tok) = tokens.next() else {
                        return Err(HunspellError::MalformedAff(line.to_string()));
                    };
                    let (add, continuation) = match add_tok.split_once('/') {
                        Some((a, c)) => (a, aff.parse_flags(c)),
                        None => (add_tok, Vec::new()),
                    };
                    let cond = tokens.next().unwrap_or(".");
                    let cross_product = aff.group_cross_product.get(&flag).copied().unwrap_or(true);
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
                        continuation,
                        cross_product,
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
        // FORBIDDENWORD wins over every other acceptance path.
        if self.is_forbidden(word) {
            return false;
        }
        if self.is_known_stem(word, false) {
            return true;
        }
        // NEEDAFFIX surfaces are NOT rejected here: igerman98-style
        // dictionaries mark valid standalone words with h plus a zero-add
        // rule (e.g. de_AT `SFX j 0 0/xoc .`), so the affixed path below
        // must still run for them; is_known_stem already rejects the bare
        // analysis, and a stem with h and no applicable affix simply falls
        // through to false.
        let lower = word.to_lowercase();
        if lower != word {
            if self.is_forbidden(&lower) {
                return false;
            }
            if self.is_known_stem(&lower, true) {
                return true;
            }
        }
        if self.is_known_affixed(word) {
            return true;
        }
        if lower != word && self.is_known_affixed(&lower) {
            return true;
        }
        self.is_known_compound(word)
    }

    /// A surface form listed with the FORBIDDENWORD flag must be rejected
    /// outright (these are words hunspell marks as wrong even though they
    /// look derivable).
    fn is_forbidden(&self, word: &str) -> bool {
        let Some(forbidden) = &self.aff.forbidden_word else {
            return false;
        };
        self.entries
            .get(word)
            .is_some_and(|flags| flags.iter().any(|f| f == forbidden))
    }

    /// Direct stem lookup. Standalone stems are rejected when they carry
    /// ONLYINCOMPOUND or NEEDAFFIX (both mean "not legal on its own"); when
    /// the lookup is a lowercased fallback, KEEPCASE entries are rejected.
    fn is_known_stem(&self, word: &str, lowercased: bool) -> bool {
        self.entries.get(word).is_some_and(|flags| {
            if flags.iter().any(|f| self.aff.only_in_compound.contains(f)) {
                return false;
            }
            if let Some(need) = &self.aff.need_affix
                && flags.iter().any(|f| f == need)
            {
                return false;
            }
            if lowercased
                && let Some(keep) = &self.aff.keep_case
                && flags.iter().any(|f| f == keep)
            {
                return false;
            }
            true
        })
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
        // Depth-1: one suffix or one prefix on a dic stem. Depth-2: word =
        // prefix + stem + suffix — reverse the suffix to get the stem+strip
        // form, reverse the prefix on that, and require the reconstructed
        // stem to carry BOTH affix flags (and both groups to permit
        // cross-products).
        for sfx in &self.aff.rules {
            if sfx.kind != AffixKind::Suffix {
                continue;
            }
            if let Some(stem) = self.apply_rule(&word_chars, sfx) {
                if self.stem_accepts(&stem, &sfx.flag) {
                    return true;
                }
                if self.pair_applies(&stem, sfx, AffixKind::Prefix) {
                    return true;
                }
            }
        }
        for pfx in &self.aff.rules {
            if pfx.kind != AffixKind::Prefix {
                continue;
            }
            if let Some(stem) = self.apply_rule(&word_chars, pfx) {
                if self.stem_accepts(&stem, &pfx.flag) {
                    return true;
                }
                if self.pair_applies(&stem, pfx, AffixKind::Suffix) {
                    return true;
                }
            }
        }
        false
    }

    /// Depth-2 helper: `first` was reverse-applied to the surface; check
    /// whether an affix of `second_kind` applies on top, verifying both
    /// flags on the reconstructed stem and the cross-product permission of
    /// both groups.
    fn pair_applies(&self, stem1: &str, first: &AffixRule, second_kind: AffixKind) -> bool {
        if !first.cross_product {
            return false;
        }
        let stem1_chars: Vec<char> = stem1.chars().collect();
        self.aff.rules.iter().any(|second| {
            second.kind == second_kind
                && second.cross_product
                && self
                    .apply_rule(&stem1_chars, second)
                    .is_some_and(|stem0| self.stem_accepts_two(&stem0, &first.flag, &second.flag))
        })
    }

    /// Like [`Self::stem_accepts`] but requiring two flags on the same stem.
    fn stem_accepts_two(&self, stem: &str, flag_a: &str, flag_b: &str) -> bool {
        self.entries.get(stem).is_some_and(|flags| {
            flags.iter().any(|f| f == flag_a) && flags.iter().any(|f| f == flag_b)
        })
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

    /// Compound check: 2- or 3-part split, all parts >= COMPOUNDMIN, each
    /// part accepted as a compound member via COMPOUNDFLAG or the positional
    /// COMPOUNDBEGIN/MIDDLE/END flag — present either on the dic stem or in
    /// the continuation flags of the affix that derived the part.
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

    fn required_compound_flag(&self, index: usize, total: usize) -> Option<&String> {
        if index == 0 {
            return self.aff.compound_begin.as_ref();
        }
        if index + 1 == total {
            return self.aff.compound_end.as_ref();
        }
        self.aff.compound_middle.as_ref()
    }

    fn is_compound_member(&self, part: &str, index: usize, total: usize) -> bool {
        if self.is_forbidden(part) {
            return false;
        }
        let required = self.required_compound_flag(index, total);

        // 1. The part is a dic stem: it must carry the required compound
        //    flag (or COMPOUNDFLAG) itself.
        if let Some(flags) = self.entries.get(part) {
            if self.has_any_compound_flag(flags, required) {
                return true;
            }
            // No early NEEDAFFIX rejection: a stem with NEEDAFFIX can still
            // qualify as a member via branch 2 (a zero-add affix whose
            // continuation flags carry the compound marker), which is how
            // igerman98 encodes compoundable h-flagged stems.
        }

        // 2. The part is affixed: the derived form's flags are the stem's
        //    flags plus the affix's continuation flags (this is where
        //    igerman98-style dictionaries keep COMPOUNDBEGIN/MIDDLE markers).
        //    A derived form with no compound flag is NOT a valid member.
        let part_chars: Vec<char> = part.chars().collect();
        for rule in &self.aff.rules {
            let Some(stem) = self.apply_rule(&part_chars, rule) else {
                continue;
            };
            if !self.stem_accepts(&stem, &rule.flag) {
                continue;
            }
            let Some(stem_flags) = self.entries.get(&stem) else {
                continue;
            };
            let mut derived: Vec<&String> = stem_flags.iter().collect();
            derived.extend(rule.continuation.iter());
            if self.has_any_compound_flag_ref(&derived, required) {
                return true;
            }
        }
        false
    }

    fn has_any_compound_flag(&self, flags: &[String], required: Option<&String>) -> bool {
        self.has_any_compound_flag_ref(&flags.iter().collect::<Vec<_>>(), required)
    }

    fn has_any_compound_flag_ref(&self, flags: &[&String], required: Option<&String>) -> bool {
        if let Some(cf) = &self.aff.compound_flag
            && flags.contains(&cf)
        {
            return true;
        }
        required.is_some_and(|w| flags.contains(&w))
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

    const AFF2: &str = "COMPOUNDMIN 3
COMPOUNDBEGIN B
COMPOUNDMIDDLE M
COMPOUNDEND E
FORBIDDENWORD F
NEEDAFFIX N
KEEPCASE K
SFX T Y 1
SFX T 0 ting/BE .
PFX Q Y 1
PFX Q 0 re .
SFX R N 1
SFX R 0 ly .
";

    const DIC2: &str = "5
star/TN
badword/F
acme/K
door/E
happy/S
form/QR
";

    fn test_dict2() -> HunspellDictionary {
        let aff = HunspellAff::parse(AFF2).expect("aff2 parses");
        HunspellDictionary::parse_dic(DIC2, aff).expect("dic2 parses")
    }

    #[test]
    fn cross_product_n_groups_do_not_combine() {
        let dict = test_dict2();
        // Prefix Q and suffix R both apply to "form", but the R group header
        // is N (no cross-products), so prefix+suffix must NOT combine.
        assert!(dict.is_known("reform"));
        assert!(dict.is_known("formly"));
        assert!(
            !dict.is_known("reformly"),
            "N cross-product group must not combine with a prefix"
        );
    }

    #[test]
    fn forbidden_word_is_rejected() {
        let dict = test_dict2();
        // "badword" is a dic stem, but its FORBIDDENWORD flag means it must
        // be rejected even though a plain stem lookup would accept it.
        assert!(!dict.is_known("badword"));
    }

    #[test]
    fn need_affix_stem_rejected_but_affixed_form_accepted() {
        let dict = test_dict2();
        // "star" carries NEEDAFFIX: illegal standalone.
        assert!(!dict.is_known("star"));
        // "starting" = star + SFX T ("ting", continuation B+E): legal.
        assert!(dict.is_known("starting"));
    }

    #[test]
    fn keep_case_rejects_lowercased_fallback() {
        let dict = test_dict2();
        assert!(dict.is_known("acme"));
        assert!(!dict.is_known("ACME"), "KEEPCASE must not accept ACME");
        assert!(
            !dict.is_known("Acme"),
            "lowercased fallback of a KEEPCASE entry must be rejected"
        );
    }

    #[test]
    fn compound_via_continuation_flags() {
        let dict = test_dict2();
        // "starting" (derived, continuation B=COMPOUNDBEGIN) + "door" (E=
        // COMPOUNDEEND) is a valid 2-part compound.
        assert!(dict.is_known("startingdoor"));
        // 3-part: starting + starting (B..E? middle needs M) — use
        // continuation B+E on both parts; middle position needs M so this
        // must fail.
        assert!(!dict.is_known("startingstartingdoor"));
    }

    #[test]
    fn flag_free_affixed_compound_parts_are_rejected() {
        let dict = test_dict();
        // "runs" (affixed, but "run" has no compound flag C beyond its own
        // C — actually run/CS carries C, so use a non-compound stem):
        // "reads" is affixed from read/PSC (C present) — instead use happy/S:
        // "happies" is affixed and carries no compound flag, so it must not
        // be a compound member even next to a flagged part.
        assert!(
            !dict.is_known("happiesdog"),
            "affixed part without compound flag"
        );
        // And a non-flagged plain stem is not a member either.
        assert!(!dict.is_known("happydog"));
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

        // FORBIDDENWORD (flag d, 395 de_AT entries): must-reject.
        assert!(
            !dict.is_known("Abendwand"),
            "FORBIDDENWORD dic entry must be rejected"
        );

        // NEEDAFFIX (flag h): "Abiturienten" carries h but igerman98 pairs
        // h with the zero-add SFX j (continuation xoc), so real hunspell
        // accepts the standalone surface — and so must we. The h+bare-stem
        // analysis stays rejected (is_known_stem); the zero-add affix
        // analysis accepts it.
        assert!(
            dict.is_known("Abiturienten"),
            "h-flagged stem with zero-add rule must be accepted standalone"
        );
        assert!(dict.is_known("Abiturientinnen"));
        // And as a compound first part via the SFX j continuation flags.
        assert!(dict.is_known("Abiturientenecke"));

        // Positive compound through continuation flags: "Abiturientinnen"
        // derives with continuation xyoc (x = COMPOUNDBEGIN via SFX g) and
        // "ecke" carries z (COMPOUNDEEND) — the compound itself is NOT a dic
        // stem, so this exercises the real compound path.
        assert!(
            dict.is_known("Abiturientinnenecke"),
            "compound with continuation-flagged first part must be accepted"
        );
        // Over-acceptance regression: "schuh" (EPSTozm) has COMPOUNDEEND z
        // but no BEGIN flag, and no affix of "schuh" carries one, so
        // "schuhecke" is not a legal compound.
        assert!(
            !dict.is_known("schuhecke"),
            "part without COMPOUNDBEGIN must be rejected"
        );
    }
}
