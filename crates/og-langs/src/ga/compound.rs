use og_core::{
    AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, Category, IssueType, RuleMatch,
    RuleMatchRule, SuggestedReplacement, rule::Rule,
};
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_TERMS: usize = 5;
const COMPOUNDS_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/resource/ga/compounds.txt"
);

struct CompoundRuleData {
    incorrect: HashSet<String>,
    joined: HashSet<String>,
    joined_lower: HashSet<String>,
    dash: HashSet<String>,
    has_digit_patterns: bool,
}

impl CompoundRuleData {
    fn load(data: &str) -> Self {
        let mut out = Self {
            incorrect: HashSet::new(),
            joined: HashSet::new(),
            joined_lower: HashSet::new(),
            dash: HashSet::new(),
            has_digit_patterns: false,
        };
        for line in data.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut exp = line.replace('-', " ");
            if exp.ends_with('+') {
                exp.truncate(exp.len() - 1);
                out.joined.insert(exp.clone());
            } else if exp.ends_with('*') {
                exp.truncate(exp.len() - 1);
                out.dash.insert(exp.clone());
            } else if exp.ends_with('?') {
                exp.truncate(exp.len() - 1);
                out.joined.insert(exp.clone());
                out.joined_lower.insert(exp.clone());
            } else if exp.ends_with('$') {
                exp.truncate(exp.len() - 1);
                out.joined.insert(exp.clone());
                out.joined_lower.insert(exp.clone());
                out.dash.insert(exp.clone());
            } else {
                out.joined.insert(exp.clone());
                out.dash.insert(exp.clone());
            }
            if exp.contains("\\d") {
                out.has_digit_patterns = true;
            }
            out.incorrect.insert(exp);
        }
        out
    }
}

pub struct IrishCompoundRule {
    data: CompoundRuleData,
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize(input: &str) -> String {
    collapse_whitespace(&input.trim().replace(" - ", " ").replace('-', " "))
}

fn uncapitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let mut out: String = first.to_lowercase().collect();
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

fn merge_compound(s: &str, uncapitalize_mid: bool) -> String {
    let mut out = String::new();
    for (k, part) in s.replace('-', " ").split(' ').enumerate() {
        if k == 0 {
            out.push_str(part);
        } else if uncapitalize_mid {
            out.push_str(&uncapitalize_first(part));
        } else {
            out.push_str(part);
        }
    }
    out
}

fn collapse_dashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        while c == '-' && chars.peek() == Some(&'-') {
            chars.next();
        }
        out.push(c);
    }
    out
}

fn is_not_all_uppercase(s: &str) -> bool {
    s.split(' ')
        .all(|part| part == "-" || !part.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()))
}

fn replace_digit_runs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars.peek().is_some_and(|n| n.is_ascii_digit()) {
                chars.next();
            }
            out.push_str("\\d+");
        } else {
            out.push(c);
        }
    }
    out
}

fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

impl IrishCompoundRule {
    pub fn new() -> Self {
        Self {
            data: CompoundRuleData::load(COMPOUNDS_TXT),
        }
    }

    pub fn entry_count(&self) -> usize {
        self.data.incorrect.len()
    }
}

impl Default for IrishCompoundRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for IrishCompoundRule {
    fn id(&self) -> &str {
        "GA_COMPOUNDS"
    }

    fn description(&self) -> &str {
        "Focail fhleiscínithe, e.g., Moltar 'ró-úsáid' seachas 'ró úsáid'"
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn category(&self) -> Category {
        Category::new("MISC", "Miscellaneous")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Misspelling
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let full = sentence.tokens();
        let mut toks: Vec<(&AnalyzedTokenReadings, bool)> = Vec::with_capacity(full.len());
        for (i, t) in full.iter().enumerate() {
            if t.is_whitespace() {
                continue;
            }
            toks.push((t, i > 0 && full[i - 1].is_whitespace()));
        }
        let n = toks.len();
        let sent_start = sentence.start();
        let text = sentence.text();
        let mut prev_tokens: VecDeque<(&AnalyzedTokenReadings, bool)> =
            VecDeque::with_capacity(MAX_TERMS);
        let synthetic = AnalyzedTokenReadings::new(AnalyzedToken::new("", 0, 0));
        let mut rule_matches: Vec<RuleMatch> = Vec::new();
        let mut prev_from: Option<usize> = None;
        for i in 0..n + MAX_TERMS {
            let current: Option<(&AnalyzedTokenReadings, bool)> =
                if i < n { Some(toks[i]) } else { None };
            if i == 0 {
                if let Some(pair) = current {
                    prev_tokens.push_back(pair);
                }
                continue;
            }
            if current.is_some_and(|(t, _)| t.is_immunized()) {
                continue;
            }
            let mut strings: Vec<String> = Vec::with_capacity(MAX_TERMS);
            let mut origs: Vec<String> = Vec::with_capacity(MAX_TERMS);
            let mut string_end: HashMap<String, usize> = HashMap::with_capacity(MAX_TERMS * 2);
            let mut sb = String::new();
            let mut is_first_sent_start = false;
            for (j, (atr, ws_before)) in prev_tokens.iter().enumerate() {
                if *ws_before {
                    sb.push(' ');
                }
                // Java's SENT_START token has empty text (JLanguageTool.java:1813);
                // this engine's carries "<S>", so drop it to keep keys identical.
                if !atr.is_sentence_start() {
                    sb.push_str(atr.token().token());
                }
                if j == 0 {
                    is_first_sent_start = atr.has_pos_tag("SENT_START");
                }
                if j >= 1 || (j == 0 && !is_first_sent_start) {
                    let mut key = normalize(&sb);
                    if is_first_sent_start {
                        key = uncapitalize_first(&key);
                    }
                    string_end.entry(key.clone()).or_insert(j);
                    strings.push(key);
                    origs.push(sb.trim().to_string());
                }
            }
            for k in (0..strings.len()).rev() {
                let key = strings[k].as_str();
                let contains_digits =
                    self.data.has_digit_patterns && key.split(' ').any(is_numeric);
                let digit_key = contains_digits.then(|| replace_digit_runs(key));
                let hit = self.data.incorrect.contains(key)
                    || digit_key
                        .as_deref()
                        .is_some_and(|dk| self.data.incorrect.contains(dk));
                if !hit {
                    continue;
                }
                let orig = origs[k].as_str();
                if self.data.dash.contains(key) && !orig.contains(' ') {
                    break;
                }
                let mut replacement: Vec<String> = Vec::new();
                let mut msg: &str = "Litrítear an nath seo mar fhocal amháin nó le fleiscín.";
                if self.data.dash.contains(key)
                    || digit_key
                        .as_deref()
                        .is_some_and(|dk| self.data.incorrect.contains(dk))
                {
                    replacement.push(orig.replace(' ', "-"));
                    msg = "Litrítear an focal seo le fleiscín de ghnáth.";
                }
                if is_not_all_uppercase(orig) && self.data.joined.contains(key) {
                    replacement.push(merge_compound(orig, self.data.joined_lower.contains(key)));
                    msg = "Litrítear an focal seo mar fhocal amháin de ghnáth.";
                }
                if key
                    .split(' ')
                    .next()
                    .is_some_and(|p| p.chars().count() == 1)
                {
                    replacement.clear();
                    replacement.push(orig.replace(' ', "-"));
                    msg = "Litrítear an focal seo le fleiscín de ghnáth.";
                } else if replacement.is_empty() || replacement.len() == 2 {
                    msg = "Litrítear an nath seo mar fhocal amháin nó le fleiscín.";
                }
                let end_j = string_end[strings[k].as_str()];
                let first = prev_tokens
                    .front()
                    .map(|(t, _)| t.token().start())
                    .unwrap_or(0);
                let end = prev_tokens[end_j].0.token().end();
                let original_slice = &text[first - sent_start..end - sent_start];
                replacement = replacement
                    .into_iter()
                    .map(|r| collapse_dashes(&r))
                    .filter(|r| r != original_slice)
                    .collect();
                if replacement.is_empty() {
                    break;
                }
                if prev_from == Some(first) {
                    break;
                }
                prev_from = Some(first);
                let rm_rule = RuleMatchRule::new(self.id(), self.description())
                    .with_category(self.category())
                    .with_issue_type(self.issue_type().as_str());
                rule_matches.push(
                    RuleMatch::new(
                        msg,
                        first,
                        end - first,
                        rm_rule,
                        super::context_window(sentence, first, end, 40),
                    )
                    .with_short_message("Fadhb leis an bhfleiscíniú")
                    .with_replacements(replacement.iter().map(SuggestedReplacement::new).collect())
                    .with_sentence(text.to_string()),
                );
                break;
            }
            if prev_tokens.len() == MAX_TERMS {
                prev_tokens.pop_front();
            }
            if let Some(pair) = current {
                prev_tokens.push_back(pair);
            } else {
                prev_tokens.push_back((&synthetic, false));
            }
        }
        rule_matches
    }
}
