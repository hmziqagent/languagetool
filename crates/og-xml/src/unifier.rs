use std::collections::{HashMap, HashSet};

use crate::compiler::{CompiledPatternToken, CompiledUnification, PosMatcher};
use crate::types::XmlUnifyGroup;

pub const UNIFY_IGNORE: &str = "unify-ignore";

pub fn normalize_apostrophes(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains('\u{2019}') || text.contains('\u{2018}') || text.contains('\u{201B}') {
        std::borrow::Cow::Owned(text.replace(['\u{2019}', '\u{2018}', '\u{201B}'], "'"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    let mut ia = a.chars();
    let mut ib = b.chars();
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) => {
                if x != y
                    && java_simple_upper(x) != java_simple_upper(y)
                    && java_simple_lower(x) != java_simple_lower(y)
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

fn java_simple_upper(c: char) -> char {
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

fn java_simple_lower(c: char) -> char {
    // Character.toLowerCase(İ) is 'i' (simple mapping); Rust's full mapping is two chars
    if c == '\u{0130}' {
        return 'i';
    }
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

pub fn surface_text(token: &og_core::AnalyzedTokenReadings) -> &str {
    if token.token().token() == "<S>" && token.is_sentence_start() {
        ""
    } else {
        token.token().token()
    }
}

pub fn has_no_tag_reading(token: &og_core::AnalyzedTokenReadings) -> bool {
    token.readings().iter().all(|r| {
        r.pos_tags()
            .iter()
            .all(|t| t == "SENT_END" || t == "PARAGRAPH_END")
    })
}

pub fn reading_pos_matches(
    postag: &str,
    postag_re: Option<&regex::Regex>,
    postag_fancy: Option<&fancy_regex::Regex>,
    tags: &[String],
    token_untagged: bool,
) -> bool {
    match postag_re {
        Some(re) => {
            tags.iter().any(|t| re.is_match(t)) || (re.is_match("UNKNOWN") && token_untagged)
        }
        None => match postag_fancy {
            Some(re) => {
                tags.iter().any(|t| re.is_match(t).unwrap_or(false))
                    || (re.is_match("UNKNOWN").unwrap_or(false) && token_untagged)
            }
            None => {
                if postag == "UNKNOWN" {
                    token_untagged
                } else {
                    tags.iter().any(|t| t == postag)
                }
            }
        },
    }
}

pub fn reading_matches_element(
    pattern: &CompiledPatternToken,
    surface: &str,
    reading: &og_core::AnalyzedToken,
    token_untagged: bool,
) -> bool {
    let pos_ok = match pattern.postag.as_deref() {
        Some(postag) => {
            reading_pos_matches(
                postag,
                pattern.compiled_postag_regexp.as_ref(),
                pattern.compiled_postag_regexp_fancy.as_ref(),
                reading.pos_tags(),
                token_untagged,
            ) != pattern.negate_pos
        }
        None => true,
    };
    let has_string = pattern.text.is_some()
        || pattern.compiled_regexp.is_some()
        || pattern.compiled_regexp_fancy.is_some();
    if !has_string {
        return !pattern.negate && pos_ok;
    }
    let test = if pattern.inflected {
        reading.lemma().unwrap_or(surface)
    } else {
        surface
    };
    let text_ok = if let Some(text) = pattern.text.as_deref() {
        let norm = normalize_apostrophes(text);
        let t = normalize_apostrophes(test);
        if pattern.case_sensitive {
            *t == *norm
        } else {
            eq_ignore_case(t.as_ref(), norm.as_ref())
        }
    } else if let Some(re) = pattern.compiled_regexp_fancy.as_ref() {
        re.is_match(test).unwrap_or(false)
    } else if let Some(re) = pattern.compiled_regexp.as_ref() {
        if pattern.case_sensitive {
            re.is_match(test)
        } else if let Some(ci) = pattern.compiled_regexp_ci.as_ref() {
            ci.is_match(test)
        } else {
            re.is_match(test)
        }
    } else {
        true
    };
    (text_ok != pattern.negate) && pos_ok
}

pub struct UnifyReading<'a> {
    pub tags: Vec<&'a str>,
    pub id: u32,
}

pub struct UnifyElemPlan<'a> {
    pub elem_idx: usize,
    pub neutral_readings: Vec<usize>,
    pub reading_sets: Vec<Vec<Vec<UnifyReading<'a>>>>,
}

#[derive(Clone)]
pub struct Unifier<'a> {
    maps: Vec<HashMap<String, HashSet<String>>>,
    slots: Vec<Vec<usize>>,
    slot_ids: Vec<Vec<u32>>,
    equiv_matched: Vec<usize>,
    kept: HashMap<String, HashSet<String>>,
    features: Vec<(String, Vec<&'a str>)>,
    eq_matchers: HashMap<(&'a str, &'a str), &'a PosMatcher>,
    tok_cnt: usize,
    readings_counter: usize,
    all_feats_in: bool,
    in_unification: bool,
    uni_matched: bool,
    uni_all_matched: bool,
}

impl<'a> Unifier<'a> {
    pub fn new(
        features: Vec<(String, Vec<&'a str>)>,
        eq_matchers: HashMap<(&'a str, &'a str), &'a PosMatcher>,
    ) -> Self {
        Self {
            maps: Vec::new(),
            slots: Vec::new(),
            slot_ids: Vec::new(),
            equiv_matched: Vec::new(),
            kept: HashMap::new(),
            features,
            eq_matchers,
            tok_cnt: 0,
            readings_counter: 1,
            all_feats_in: false,
            in_unification: false,
            uni_matched: false,
            uni_all_matched: false,
        }
    }

    pub fn from_unifications(
        unifications: &'a [CompiledUnification],
        group: &'a XmlUnifyGroup,
    ) -> Self {
        let mut eq_matchers: HashMap<(&str, &str), &PosMatcher> =
            HashMap::with_capacity(unifications.len() * 4);
        let mut declared: HashMap<&str, Vec<&str>> = HashMap::with_capacity(unifications.len());
        for uni in unifications {
            for (eq_type, matcher) in &uni.equivalences {
                eq_matchers
                    .entry((uni.feature.as_str(), eq_type.as_str()))
                    .or_insert(matcher);
                declared
                    .entry(uni.feature.as_str())
                    .or_default()
                    .push(eq_type.as_str());
            }
        }
        let features: Vec<(String, Vec<&'a str>)> = if group.features.is_empty() {
            unifications
                .iter()
                .map(|u| {
                    (
                        u.feature.clone(),
                        declared
                            .get(u.feature.as_str())
                            .cloned()
                            .unwrap_or_default(),
                    )
                })
                .collect()
        } else {
            group
                .features
                .iter()
                .map(|(name, types)| {
                    let resolved: Vec<&'a str> = if types.is_empty() {
                        declared.get(name.as_str()).cloned().unwrap_or_default()
                    } else {
                        types.iter().map(String::as_str).collect()
                    };
                    (name.clone(), resolved)
                })
                .collect()
        };
        Self::new(features, eq_matchers)
    }

    pub fn is_unified(&mut self, reading: &UnifyReading<'_>, last_reading: bool) -> bool {
        if self.in_unification {
            self.uni_matched |= self.is_satisfied(reading);
            self.uni_all_matched = self.uni_matched;
            if last_reading {
                self.start_next_token();
                self.uni_matched = false;
            }
            self.uni_all_matched && self.get_final_unification_value()
        } else {
            self.is_satisfied(reading);
            if last_reading {
                self.in_unification = true;
                self.uni_matched = false;
                self.all_feats_in = true;
            }
            true
        }
    }

    fn is_satisfied(&mut self, reading: &UnifyReading<'_>) -> bool {
        let tags = &reading.tags;
        if self.all_feats_in && self.equiv_matched.is_empty() {
            return false;
        }
        if self.all_feats_in {
            return self.check_next(reading);
        }
        let mut map: HashMap<String, HashSet<String>> = HashMap::new();
        for (feature, types) in &self.features {
            let mut matched = HashSet::new();
            for eq_type in types {
                let Some(matcher) = self.eq_matchers.get(&(feature.as_str(), *eq_type)) else {
                    return false;
                };
                if tags.iter().any(|t| matcher.is_match(t)) {
                    matched.insert((*eq_type).to_string());
                }
            }
            if matched.is_empty() {
                return false;
            }
            map.insert(feature.clone(), matched);
        }
        let map_idx = self.maps.len();
        self.maps.push(map);
        if self.slots.is_empty() {
            self.slots.push(vec![map_idx]);
            self.slot_ids.push(vec![reading.id]);
        } else {
            self.slots[0].push(map_idx);
            self.slot_ids[0].push(reading.id);
        }
        self.equiv_matched.push(map_idx);
        self.tok_cnt += 1;
        true
    }

    fn check_next(&mut self, reading: &UnifyReading<'_>) -> bool {
        let tags = &reading.tags;
        let mut any_feat_unified = false;
        let mut matched_here: HashMap<String, HashSet<String>> = HashMap::new();
        for i in 0..self.tok_cnt {
            let mut all_feats_unified = true;
            for (feature, types) in &self.features {
                let mut feat_unified = false;
                let branch = &self.maps[self.equiv_matched[i]];
                let set = branch.get(feature);
                for eq_type in types {
                    if set.is_some_and(|s| s.contains(*eq_type))
                        && self
                            .eq_matchers
                            .get(&(feature.as_str(), *eq_type))
                            .is_some_and(|m| tags.iter().any(|t| m.is_match(t)))
                    {
                        feat_unified = true;
                        self.kept
                            .entry(feature.clone())
                            .or_default()
                            .insert((*eq_type).to_string());
                        matched_here
                            .entry(feature.clone())
                            .or_default()
                            .insert((*eq_type).to_string());
                    }
                }
                all_feats_unified &= feat_unified;
            }
            any_feat_unified |= all_feats_unified;
        }
        if any_feat_unified {
            let map_idx = self.maps.len();
            self.maps.push(matched_here);
            if self.slots.len() == self.readings_counter {
                self.slots.push(vec![map_idx]);
                self.slot_ids.push(vec![reading.id]);
            } else if self.readings_counter < self.slots.len() {
                self.slots[self.readings_counter].push(map_idx);
                self.slot_ids[self.readings_counter].push(reading.id);
            } else {
                return false;
            }
        }
        any_feat_unified
    }

    fn start_next_token(&mut self) {
        self.readings_counter += 1;
        let Unifier {
            maps, slots, kept, ..
        } = self;
        for slot in slots.iter_mut() {
            for idx in slot.iter().copied() {
                let map = &mut maps[idx];
                map.retain(|feature, types| {
                    if feature.as_str() == UNIFY_IGNORE {
                        return true;
                    }
                    match kept.get(feature) {
                        Some(kept_types) => {
                            types.retain(|t| kept_types.contains(t));
                            true
                        }
                        None => false,
                    }
                });
            }
        }
        self.kept.clear();
    }

    pub fn add_neutral(&mut self, readings: usize) {
        let map_idx = self.maps.len();
        self.maps
            .push(std::iter::once((UNIFY_IGNORE.to_string(), HashSet::new())).collect());
        self.slots.push(vec![map_idx; readings]);
        self.slot_ids.push((0..readings as u32).collect());
        self.readings_counter += 1;
    }

    pub fn get_final_unification_value(&self) -> bool {
        let n_feats = self.features.len();
        let mut tok_unified = 0usize;
        for (j, slot) in self.slots.iter().enumerate() {
            let mut unified_tokens_found = false;
            for (i, &map_idx) in slot.iter().enumerate() {
                let map = &self.maps[map_idx];
                if map.contains_key(UNIFY_IGNORE) {
                    if i == 0 {
                        tok_unified += 1;
                    }
                    unified_tokens_found = true;
                    continue;
                }
                let mut feat_unified = 0usize;
                for (feature, _) in &self.features {
                    match map.get(feature) {
                        Some(s) if s.is_empty() => feat_unified = 0,
                        _ => feat_unified += 1,
                    }
                    if feat_unified == n_feats && tok_unified <= j {
                        tok_unified += 1;
                        unified_tokens_found = true;
                        break;
                    }
                }
            }
            if !unified_tokens_found {
                return false;
            }
        }
        tok_unified == self.slots.len()
    }

    pub fn unified_readings(&self) -> Option<Vec<Vec<u32>>> {
        let n_feats = self.features.len();
        let mut out: Vec<Vec<u32>> = Vec::with_capacity(self.slots.len());
        for (j, slot) in self.slots.iter().enumerate() {
            let mut unified_tokens_found = false;
            let mut ids: Vec<u32> = Vec::with_capacity(slot.len());
            for (i, &map_idx) in slot.iter().enumerate() {
                let map = &self.maps[map_idx];
                if map.contains_key(UNIFY_IGNORE) {
                    if let Some(&id) = self.slot_ids[j].get(i) {
                        ids.push(id);
                    }
                    unified_tokens_found = true;
                    continue;
                }
                let mut feat_unified = 0usize;
                for (feature, _) in &self.features {
                    match map.get(feature) {
                        Some(s) if s.is_empty() => feat_unified = 0,
                        _ => feat_unified += 1,
                    }
                    if feat_unified == n_feats {
                        if let Some(&id) = self.slot_ids[j].get(i) {
                            ids.push(id);
                        }
                        unified_tokens_found = true;
                    }
                }
            }
            if !unified_tokens_found {
                return None;
            }
            out.push(ids);
        }
        Some(out)
    }
}

pub fn eval_unify_plans(
    plans: &[UnifyElemPlan<'_>],
    plan_idx: usize,
    last_elem: usize,
    negate: bool,
    uni: &mut Unifier<'_>,
) -> bool {
    let Some(plan) = plans.get(plan_idx) else {
        return true;
    };
    if plan.reading_sets.is_empty() {
        for count in &plan.neutral_readings {
            uni.add_neutral(*count);
        }
        return eval_unify_plans(plans, plan_idx + 1, last_elem, negate, uni);
    }
    for variant in &plan.reading_sets {
        let mut branch_uni = if plan.reading_sets.len() > 1 {
            Some(uni.clone())
        } else {
            None
        };
        let active = branch_uni.as_mut().unwrap_or(uni);
        let mut failed = false;
        let total_sets = variant.len();
        for (s, set) in variant.iter().enumerate() {
            let total = set.len();
            let mut any_matched = false;
            for (i, reading) in set.iter().enumerate() {
                any_matched |= active.is_unified(reading, i + 1 == total);
            }
            if plan.elem_idx != last_elem {
                continue;
            }
            if negate && any_matched {
                failed = true;
                break;
            }
            if s + 1 == total_sets && !any_matched && !negate {
                failed = true;
                break;
            }
        }
        if failed {
            continue;
        }
        if eval_unify_plans(plans, plan_idx + 1, last_elem, negate, active) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_ignore_case_matches_java_equalsignorecase_for_ascii() {
        assert!(eq_ignore_case("Hello", "hello"));
        assert!(!eq_ignore_case("Hello", "hell"));
        assert!(eq_ignore_case("", ""));
    }

    #[test]
    fn eq_ignore_case_folds_dotless_and_dotted_i_like_java() {
        assert!(
            eq_ignore_case("i", "ı"),
            "String.equalsIgnoreCase folds i with ı via toUpperCase"
        );
        assert!(eq_ignore_case("İ", "i"));
        assert!(eq_ignore_case("İ", "I"));
        assert!(eq_ignore_case("bıg", "BIG"));
        assert!(!eq_ignore_case("bıg", "beg"));
    }

    #[test]
    fn eq_ignore_case_uses_per_char_mapping_not_full_lowercase() {
        assert!(!eq_ignore_case("ß", "SS"));
        assert!(eq_ignore_case("ß", "ß"));
        assert!(eq_ignore_case("ſ", "s"));
        assert!(eq_ignore_case("K", "k"));
        assert!(!eq_ignore_case("ab", "abc"));
    }
}
