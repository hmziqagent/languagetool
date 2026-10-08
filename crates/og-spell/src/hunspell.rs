use std::collections::HashMap;
use std::path::Path;

const MAX_WORD_UTF8_LEN: usize = 100;
const MAX_SHARPS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CapType {
    No,
    Init,
    All,
    Huh,
    HuhInit,
}

#[derive(Clone, Debug)]
struct CondClass {
    negated: bool,
    chars: Vec<char>,
}

#[derive(Clone, Debug)]
struct AffixLine {
    name: char,
    is_prefix: bool,
    cross: bool,
    strip: String,
    add: String,
    cont: Vec<char>,
    cond: Vec<CondClass>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InCompound {
    Not,
    Begin,
    End,
}

enum RootHit {
    Flags(&'static str),
    Forbidden,
}

enum CheckResult {
    NotFound,
    Found { keepcase: bool },
    Forbidden,
}

pub struct HunspellChecker {
    entries: HashMap<&'static str, Vec<&'static str>>,
    affixes: Vec<AffixLine>,
    zero_prefixes: Vec<usize>,
    zero_suffixes: Vec<usize>,
    prefixes_by_first: HashMap<char, Vec<usize>>,
    suffixes_by_last: HashMap<char, Vec<usize>>,
    compound_begin: Option<char>,
    compound_middle: Option<char>,
    compound_end: Option<char>,
    forbiddenword: Option<char>,
    permit: Option<char>,
    only_in_compound: Option<char>,
    need_affix: Option<char>,
    circumfix: Option<char>,
    keepcase: Option<char>,
    compound_min: usize,
    check_sharps: bool,
}

fn lower_char(c: char) -> char {
    let mut it = c.to_lowercase();
    let first = it.next().unwrap_or(c);
    if it.next().is_some() { c } else { first }
}

fn upper_char(c: char) -> char {
    let mut it = c.to_uppercase();
    let first = it.next().unwrap_or(c);
    if it.next().is_some() { c } else { first }
}

fn has_flag(flags: &str, flag: Option<char>) -> bool {
    match flag {
        Some(f) => flags.contains(f),
        None => false,
    }
}

fn parse_condition(cond: &str) -> Vec<CondClass> {
    let mut out = Vec::new();
    let chars: Vec<char> = cond.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '.' => out.push(CondClass {
                negated: false,
                chars: Vec::new(),
            }),
            '[' => {
                i += 1;
                let negated = i < chars.len() && chars[i] == '^';
                if negated {
                    i += 1;
                }
                let mut set = Vec::new();
                while i < chars.len() && chars[i] != ']' {
                    set.push(chars[i]);
                    i += 1;
                }
                out.push(CondClass {
                    negated,
                    chars: set,
                });
            }
            c => out.push(CondClass {
                negated: false,
                chars: vec![c],
            }),
        }
        i += 1;
    }
    out
}

fn cond_match(cond: &[CondClass], s: &[char], at_start: bool) -> bool {
    if cond.len() > s.len() {
        return false;
    }
    let offset = if at_start { 0 } else { s.len() - cond.len() };
    for (i, class) in cond.iter().enumerate() {
        if class.chars.is_empty() {
            continue;
        }
        let inside = class.chars.contains(&s[offset + i]);
        if inside == class.negated {
            return false;
        }
    }
    true
}

fn cap_type(chars: &[char]) -> CapType {
    let mut ncap = 0;
    let mut nneutral = 0;
    for &c in chars {
        if c != lower_char(c) {
            ncap += 1;
        }
        if upper_char(c) == lower_char(c) {
            nneutral += 1;
        }
    }
    let firstcap = !chars.is_empty() && chars[0] != lower_char(chars[0]);
    if ncap == 0 {
        CapType::No
    } else if ncap == 1 && firstcap {
        CapType::Init
    } else if ncap == chars.len() || ncap + nneutral == chars.len() {
        CapType::All
    } else if ncap > 1 && firstcap {
        CapType::HuhInit
    } else {
        CapType::Huh
    }
}

fn mk_all_small(chars: &[char]) -> Vec<char> {
    chars.iter().map(|&c| lower_char(c)).collect()
}

fn mk_init_cap(chars: &[char]) -> Vec<char> {
    let mut out = mk_all_small(chars);
    if !out.is_empty() {
        out[0] = upper_char(out[0]);
    }
    out
}

impl HunspellChecker {
    pub fn load(aff_path: &Path, dic_path: &Path) -> Option<Self> {
        let aff = std::fs::read_to_string(aff_path).ok()?;
        let dic: &'static str = Box::leak(std::fs::read_to_string(dic_path).ok()?.into_boxed_str());
        Self::parse(&aff, dic)
    }

    fn parse(aff: &str, dic: &'static str) -> Option<Self> {
        let mut checker = HunspellChecker {
            entries: HashMap::new(),
            affixes: Vec::new(),
            zero_prefixes: Vec::new(),
            zero_suffixes: Vec::new(),
            prefixes_by_first: HashMap::new(),
            suffixes_by_last: HashMap::new(),
            compound_begin: None,
            compound_middle: None,
            compound_end: None,
            forbiddenword: None,
            permit: None,
            only_in_compound: None,
            need_affix: None,
            circumfix: None,
            keepcase: None,
            compound_min: 3,
            check_sharps: false,
        };
        let mut headers: HashMap<char, bool> = HashMap::new();
        let lines = aff.lines().peekable();
        for line in lines {
            let line = match line.find('#') {
                Some(0) => continue,
                Some(pos) => &line[..pos],
                None => line,
            };
            if line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            match fields[0] {
                "PFX" | "SFX" => {
                    let Some(name) = fields[1].chars().next() else {
                        continue;
                    };
                    let is_prefix = fields[0] == "PFX";
                    if fields.len() == 4 && fields[3].parse::<i32>().is_ok() {
                        headers.insert(name, fields[2] == "Y");
                        continue;
                    }
                    if fields.len() < 4 {
                        continue;
                    }
                    let cross = headers.get(&name).copied().unwrap_or(true);
                    let strip = if fields[2] == "0" { "" } else { fields[2] };
                    let (add, cont) = match fields[3].split_once('/') {
                        Some((a, f)) => (a, f.chars().collect::<Vec<char>>()),
                        None => (fields[3], Vec::new()),
                    };
                    let add = if add == "0" { "" } else { add };
                    let cond = if fields.len() > 4 {
                        parse_condition(fields[4])
                    } else {
                        parse_condition(".")
                    };
                    let idx = checker.affixes.len();
                    if is_prefix {
                        match add.chars().next() {
                            None => checker.zero_prefixes.push(idx),
                            Some(c) => checker.prefixes_by_first.entry(c).or_default().push(idx),
                        }
                    } else {
                        match add.chars().next_back() {
                            None => checker.zero_suffixes.push(idx),
                            Some(c) => checker.suffixes_by_last.entry(c).or_default().push(idx),
                        }
                    }
                    checker.affixes.push(AffixLine {
                        name,
                        is_prefix,
                        cross,
                        strip: strip.to_string(),
                        add: add.to_string(),
                        cont,
                        cond,
                    });
                }
                "COMPOUNDBEGIN" => {
                    checker.compound_begin = fields.get(1).and_then(|f| f.chars().next())
                }
                "COMPOUNDMIDDLE" => {
                    checker.compound_middle = fields.get(1).and_then(|f| f.chars().next())
                }
                "COMPOUNDEND" => {
                    checker.compound_end = fields.get(1).and_then(|f| f.chars().next())
                }
                "FORBIDDENWORD" => {
                    checker.forbiddenword = fields.get(1).and_then(|f| f.chars().next())
                }
                "COMPOUNDPERMITFLAG" => {
                    checker.permit = fields.get(1).and_then(|f| f.chars().next())
                }
                "ONLYINCOMPOUND" => {
                    checker.only_in_compound = fields.get(1).and_then(|f| f.chars().next())
                }
                "NEEDAFFIX" => checker.need_affix = fields.get(1).and_then(|f| f.chars().next()),
                "CIRCUMFIX" => checker.circumfix = fields.get(1).and_then(|f| f.chars().next()),
                "KEEPCASE" => checker.keepcase = fields.get(1).and_then(|f| f.chars().next()),
                "COMPOUNDMIN" => {
                    checker.compound_min = fields.get(1).and_then(|f| f.parse().ok()).unwrap_or(3)
                }
                "CHECKSHARPS" => checker.check_sharps = true,
                _ => {}
            }
        }
        let mut seen_first = false;
        for line in dic.lines() {
            if !seen_first {
                seen_first = true;
                continue;
            }
            let word_part = match line.find('\t') {
                Some(pos) => &line[..pos],
                None => line,
            };
            let word = match word_part.find('/') {
                Some(0) | None => word_part,
                Some(pos) => &word_part[..pos],
            };
            let flags = match word_part.find('/') {
                Some(pos) if pos > 0 => &word_part[pos + 1..],
                _ => "",
            };
            if word.is_empty() {
                continue;
            }
            checker.entries.entry(word).or_default().push(flags);
        }
        if checker.entries.is_empty() {
            return None;
        }
        Some(checker)
    }

    pub fn spell(&self, word: &str) -> bool {
        if word.len() >= MAX_WORD_UTF8_LEN {
            return false;
        }
        let all: Vec<char> = word.chars().collect();
        let start = all.iter().position(|&c| c != ' ').unwrap_or(all.len());
        let mut end = all.len();
        let mut abbv = false;
        while end > start && all[end - 1] == '.' {
            end -= 1;
            abbv = true;
        }
        if end <= start {
            return true;
        }
        let scw: Vec<char> = all[start..end].to_vec();
        if self.is_number_like(&scw) {
            return true;
        }
        let captype = cap_type(&scw);
        match captype {
            CapType::No | CapType::Huh | CapType::HuhInit => {
                let mut r = self.check_result(&scw);
                if matches!(r, CheckResult::NotFound) && abbv {
                    let mut with_dot = scw.clone();
                    with_dot.push('.');
                    r = self.check_result(&with_dot);
                }
                !matches!(r, CheckResult::NotFound | CheckResult::Forbidden)
            }
            CapType::Init => self.spell_init_cap(&scw, abbv, false),
            CapType::All => {
                let mut r = self.check_result(&scw);
                if matches!(r, CheckResult::NotFound) && abbv {
                    let mut with_dot = scw.clone();
                    with_dot.push('.');
                    r = self.check_result(&with_dot);
                }
                if !matches!(r, CheckResult::NotFound) {
                    return !matches!(r, CheckResult::Forbidden);
                }
                if self.check_sharps && scw.windows(2).any(|w| w[0] == 'S' && w[1] == 'S') {
                    let small = mk_all_small(&scw);
                    if self.spellsharps(&small, 0, 0, 0) {
                        return true;
                    }
                    let init = mk_init_cap(&scw);
                    if self.spellsharps(&init, 0, 0, 0) {
                        return true;
                    }
                    if abbv {
                        let mut small_dot = small;
                        small_dot.push('.');
                        if self.spellsharps(&small_dot, 0, 0, 0) {
                            return true;
                        }
                        let mut init_dot = mk_init_cap(&scw);
                        init_dot.push('.');
                        if self.spellsharps(&init_dot, 0, 0, 0) {
                            return true;
                        }
                    }
                }
                let converged = mk_init_cap(&mk_all_small(&scw));
                self.spell_init_cap(&converged, abbv, true)
            }
        }
    }

    fn spell_init_cap(&self, scw: &[char], abbv: bool, all_cap_fallthrough: bool) -> bool {
        match self.check_result(scw) {
            CheckResult::Forbidden => return false,
            CheckResult::Found { keepcase } => {
                if !keepcase || !all_cap_fallthrough {
                    return true;
                }
            }
            CheckResult::NotFound => {}
        }
        let lower = mk_all_small(scw);
        let mut r = self.check_result(&lower);
        let mut probe = lower.clone();
        if matches!(r, CheckResult::NotFound) && abbv {
            probe.push('.');
            r = self.check_result(&probe);
            if matches!(r, CheckResult::NotFound) {
                let mut init_dot = mk_init_cap(scw);
                init_dot.push('.');
                let r4 = self.check_result(&init_dot);
                if let CheckResult::Found { keepcase } = r4
                    && (!keepcase || !all_cap_fallthrough)
                {
                    return true;
                }
                return false;
            }
        }
        match r {
            CheckResult::Forbidden => false,
            CheckResult::NotFound => false,
            CheckResult::Found { keepcase } => {
                !(keepcase && (all_cap_fallthrough || !(self.check_sharps && probe.contains(&'ß'))))
            }
        }
    }

    fn spellsharps(&self, base: &[char], from: usize, n: usize, repnum: usize) -> bool {
        let pos = base[from..]
            .windows(2)
            .position(|w| w[0] == 's' && w[1] == 's')
            .map(|p| p + from);
        if let Some(pos) = pos {
            if n < MAX_SHARPS {
                let mut replaced = base.to_vec();
                replaced[pos] = 'ß';
                replaced.remove(pos + 1);
                if self.spellsharps(&replaced, pos + 1, n + 1, repnum + 1) {
                    return true;
                }
                if self.spellsharps(base, pos + 2, n + 1, repnum) {
                    return true;
                }
                return false;
            }
            return false;
        }
        if repnum > 0 {
            !matches!(self.check_result(base), CheckResult::NotFound)
        } else {
            false
        }
    }

    fn is_number_like(&self, chars: &[char]) -> bool {
        #[derive(PartialEq)]
        enum State {
            Begin,
            Num,
            Sep,
        }
        let mut state = State::Begin;
        for &c in chars {
            if c.is_ascii_digit() {
                state = State::Num;
            } else if c == ',' || c == '.' || c == '-' {
                if state == State::Sep || state == State::Begin {
                    break;
                }
                state = State::Sep;
            } else {
                break;
            }
        }
        !chars.is_empty()
            && state == State::Num
            && chars
                .iter()
                .all(|c| c.is_ascii_digit() || *c == ',' || *c == '.' || *c == '-')
    }

    fn check_result(&self, word: &[char]) -> CheckResult {
        if word.is_empty() {
            return CheckResult::NotFound;
        }
        if let Some(homonyms) = self.lookup_flags(word) {
            if let Some(first) = homonyms.first()
                && has_flag(first, self.forbiddenword)
            {
                return CheckResult::Forbidden;
            }
            for entry in homonyms {
                if has_flag(entry, self.need_affix) || has_flag(entry, self.only_in_compound) {
                    continue;
                }
                return CheckResult::Found {
                    keepcase: has_flag(entry, self.keepcase),
                };
            }
        }
        if let Some(hit) = self.affix_check(word, None, InCompound::Not) {
            return match hit {
                RootHit::Forbidden => CheckResult::Forbidden,
                RootHit::Flags(flags) => {
                    if has_flag(flags, self.only_in_compound) {
                        CheckResult::NotFound
                    } else {
                        CheckResult::Found {
                            keepcase: has_flag(flags, self.keepcase),
                        }
                    }
                }
            };
        }
        if (self.compound_begin.is_some() || self.compound_middle.is_some())
            && (self.compound_check(word, 0, true) || self.compound_check(word, 0, false))
        {
            return CheckResult::Found { keepcase: false };
        }
        CheckResult::NotFound
    }

    fn lookup_flags(&self, word: &[char]) -> Option<&Vec<&'static str>> {
        let mut buf = [0u8; 128];
        let mut len = 0;
        for &c in word {
            let mut encoded = [0u8; 4];
            let s = c.encode_utf8(&mut encoded);
            for &b in s.as_bytes() {
                if len == buf.len() {
                    let key = word.iter().collect::<String>();
                    return self.entries.get(key.as_str());
                }
                buf[len] = b;
                len += 1;
            }
        }
        let key = std::str::from_utf8(&buf[..len]).ok()?;
        self.entries.get(key)
    }

    fn affix_check(
        &self,
        word: &[char],
        needflag: Option<char>,
        in_compound: InCompound,
    ) -> Option<RootHit> {
        if let Some(hit) = self.prefix_check(word, needflag, in_compound) {
            return Some(hit);
        }
        self.suffix_check(word, needflag, in_compound, None, false)
    }

    fn suffix_gate(
        &self,
        line: &AffixLine,
        in_compound: InCompound,
        pfx: Option<&AffixLine>,
    ) -> bool {
        if in_compound == InCompound::Begin && !line.cont.contains(&self.permit.unwrap_or('\0')) {
            return false;
        }
        if in_compound == InCompound::Not
            && self.only_in_compound.is_some()
            && line.cont.contains(&self.only_in_compound.unwrap_or('\0'))
        {
            return false;
        }
        if let Some(circ) = self.circumfix {
            let sfx_has = line.cont.contains(&circ);
            match pfx {
                None => {
                    if sfx_has {
                        return false;
                    }
                }
                Some(p) => {
                    let pfx_has = p.cont.contains(&circ);
                    if pfx_has != sfx_has {
                        return false;
                    }
                }
            }
        }
        if let Some(h) = self.need_affix
            && line.cont.contains(&h)
            && !matches!(pfx, Some(p) if !p.cont.contains(&h))
        {
            return false;
        }
        true
    }

    fn prefix_gate(&self, line: &AffixLine, in_compound: InCompound) -> bool {
        if in_compound == InCompound::Not
            && self.only_in_compound.is_some()
            && line.cont.contains(&self.only_in_compound.unwrap_or('\0'))
        {
            return false;
        }
        if in_compound == InCompound::End && !line.cont.contains(&self.permit.unwrap_or('\0')) {
            return false;
        }
        true
    }

    fn try_suffix_line(
        &self,
        idx: usize,
        word: &[char],
        needflag: Option<char>,
        in_compound: InCompound,
        pfx: Option<&AffixLine>,
        cross: bool,
    ) -> Option<RootHit> {
        let line = &self.affixes[idx];
        if line.is_prefix {
            return None;
        }
        if cross && !line.cross {
            return None;
        }
        let add_chars: Vec<char> = line.add.chars().collect();
        if word.len() < add_chars.len() || !word.ends_with(&add_chars[..]) {
            return None;
        }
        if !self.suffix_gate(line, in_compound, pfx) {
            return None;
        }
        let stem_len = word.len() - add_chars.len();
        if stem_len == 0 {
            return None;
        }
        let mut root: Vec<char> = word[..stem_len].to_vec();
        root.extend(line.strip.chars());
        if !cond_match(&line.cond, &root, false) {
            return None;
        }
        let badflag = if in_compound == InCompound::Not {
            self.only_in_compound
        } else {
            None
        };
        let entries = self.lookup_flags(&root)?;
        for entry in entries {
            if let Some(bad) = badflag
                && entry.contains(bad)
            {
                continue;
            }
            if !entry.contains(line.name) && !matches!(pfx, Some(p) if p.cont.contains(&line.name))
            {
                continue;
            }
            if cross {
                let p = pfx?;
                if !entry.contains(p.name) && !line.cont.contains(&p.name) {
                    continue;
                }
            }
            if let Some(f) = needflag
                && !entry.contains(f)
                && !line.cont.contains(&f)
            {
                continue;
            }
            return Some(if has_flag(entry, self.forbiddenword) {
                RootHit::Forbidden
            } else {
                RootHit::Flags(entry)
            });
        }
        None
    }

    fn try_prefix_line(
        &self,
        idx: usize,
        word: &[char],
        needflag: Option<char>,
        in_compound: InCompound,
    ) -> Option<RootHit> {
        let line = &self.affixes[idx];
        if !line.is_prefix {
            return None;
        }
        let add_chars: Vec<char> = line.add.chars().collect();
        if word.len() < add_chars.len() || !word.starts_with(&add_chars[..]) {
            return None;
        }
        if !self.prefix_gate(line, in_compound) {
            return None;
        }
        let stem_len = word.len() - add_chars.len();
        if stem_len == 0 {
            return None;
        }
        let mut root: Vec<char> = line.strip.chars().collect();
        root.extend_from_slice(&word[add_chars.len()..]);
        if !cond_match(&line.cond, &root, true) {
            return None;
        }
        if let Some(entries) = self.lookup_flags(&root) {
            for entry in entries {
                if !entry.contains(line.name) {
                    continue;
                }
                if self.need_affix.is_some() && line.cont.contains(&self.need_affix.unwrap()) {
                    continue;
                }
                if let Some(f) = needflag
                    && !entry.contains(f)
                    && !line.cont.contains(&f)
                {
                    continue;
                }
                return Some(if has_flag(entry, self.forbiddenword) {
                    RootHit::Forbidden
                } else {
                    RootHit::Flags(entry)
                });
            }
        }
        if line.cross
            && let Some(hit) = self.suffix_check(&root, needflag, in_compound, Some(line), true)
        {
            return Some(hit);
        }
        None
    }

    fn prefix_check(
        &self,
        word: &[char],
        needflag: Option<char>,
        in_compound: InCompound,
    ) -> Option<RootHit> {
        let tail = word
            .first()
            .and_then(|c| self.prefixes_by_first.get(c))
            .into_iter()
            .flatten();
        for idx in self.zero_prefixes.iter().copied().chain(tail.copied()) {
            if let Some(hit) = self.try_prefix_line(idx, word, needflag, in_compound) {
                return Some(hit);
            }
        }
        None
    }

    fn suffix_check(
        &self,
        word: &[char],
        needflag: Option<char>,
        in_compound: InCompound,
        pfx: Option<&AffixLine>,
        cross: bool,
    ) -> Option<RootHit> {
        let tail = word
            .last()
            .and_then(|c| self.suffixes_by_last.get(c))
            .into_iter()
            .flatten();
        for idx in self.zero_suffixes.iter().copied().chain(tail.copied()) {
            if let Some(hit) = self.try_suffix_line(idx, word, needflag, in_compound, pfx, cross) {
                return Some(hit);
            }
        }
        None
    }

    fn compound_first_ok(&self, part: &[char], wordnum: usize) -> Option<bool> {
        let need = if wordnum == 0 {
            self.compound_begin
        } else {
            self.compound_middle
        }?;
        if let Some(entries) = self.lookup_flags(part) {
            for entry in entries {
                if has_flag(entry, self.need_affix) {
                    continue;
                }
                if !entry.contains(need) {
                    continue;
                }
                if has_flag(entry, self.forbiddenword) {
                    return Some(false);
                }
                return Some(true);
            }
        }
        if let Some(RootHit::Flags(_)) =
            self.suffix_check(part, Some(need), InCompound::Begin, None, false)
        {
            return Some(true);
        }
        if let Some(RootHit::Flags(_)) = self.prefix_check(part, Some(need), InCompound::Begin) {
            return Some(true);
        }
        None
    }

    fn compound_second_ok(&self, part: &[char]) -> Option<bool> {
        let need = self.compound_end?;
        if let Some(entries) = self.lookup_flags(part) {
            for entry in entries {
                if has_flag(entry, self.need_affix) {
                    continue;
                }
                if !entry.contains(need) {
                    continue;
                }
                if has_flag(entry, self.forbiddenword) {
                    return Some(false);
                }
                return Some(true);
            }
        }
        match self.affix_check(part, Some(need), InCompound::End) {
            Some(RootHit::Forbidden) => Some(false),
            Some(RootHit::Flags(_)) => Some(true),
            None => None,
        }
    }

    fn compound_check(&self, word: &[char], wordnum: usize, two_word_only: bool) -> bool {
        let cmin = self.compound_min;
        if word.len() < cmin * 2 {
            return false;
        }
        for i in cmin..word.len() - cmin + 1 {
            let part1 = &word[..i];
            let part2 = &word[i..];
            match self.compound_first_ok(part1, wordnum) {
                Some(false) => return false,
                Some(true) => {}
                None => continue,
            }
            match self.compound_second_ok(part2) {
                Some(false) => return false,
                Some(true) => return true,
                None => {}
            }
            if !two_word_only && self.compound_check(part2, wordnum + 1, false) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn de_checker() -> Option<HunspellChecker> {
        let dir = std::path::Path::new(
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/resource/de/hunspell",
        );
        HunspellChecker::load(&dir.join("de_DE.aff"), &dir.join("de_DE.dic"))
    }

    #[test]
    fn test_de_spell_parity() {
        let Some(checker) = de_checker() else {
            return;
        };
        let accepted = [
            "Haus",
            "hocheffizientes",
            "hocheffizient",
            "vorhaben",
            "vorhast",
            "hochexplosiv",
            "rüber",
            "lassen",
            "hinterher",
            "spioniert",
            "hinterherspionieren",
            "reinlassen",
            "hervorgeholt",
            "Arbeitszimmer",
            "Straße",
        ];
        for word in accepted {
            assert!(checker.spell(word), "expected accepted: {word}");
        }
        let rejected = [
            "rüberlassen",
            "hinterherspioniert",
            "rüberbringen",
            "hierherholen",
            "Hausx",
            "xyqv",
        ];
        for word in rejected {
            assert!(!checker.spell(word), "expected rejected: {word}");
        }
    }
}
