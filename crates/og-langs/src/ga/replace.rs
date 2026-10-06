use og_core::{
    AnalyzedSentence, Category, IssueType, RuleMatch, RuleMatchRule, SuggestedReplacement,
    rule::Rule,
};
use std::collections::HashMap;

pub struct GaSimpleReplaceRule {
    id: &'static str,
    description: &'static str,
    short: &'static str,
    message: &'static str,
    category: Category,
    issue_type: IssueType,
    ignore_tagged_words: bool,
    url: Option<&'static str>,
    entries: HashMap<String, Vec<String>>,
}

impl GaSimpleReplaceRule {
    pub fn new(
        id: &'static str,
        description: &'static str,
        short: &'static str,
        message: &'static str,
    ) -> Self {
        Self {
            id,
            description,
            short,
            message,
            category: Category::new("TYPOS", "Possible Typo"),
            issue_type: IssueType::Misspelling,
            ignore_tagged_words: false,
            url: None,
            entries: HashMap::new(),
        }
    }

    pub fn with_category(mut self, category: Category) -> Self {
        self.category = category;
        self
    }

    pub fn with_issue_type(mut self, issue_type: IssueType) -> Self {
        self.issue_type = issue_type;
        self
    }

    pub fn with_ignore_tagged_words(mut self) -> Self {
        self.ignore_tagged_words = true;
        self
    }

    pub fn with_url(mut self, url: &'static str) -> Self {
        self.url = Some(url);
        self
    }

    pub fn with_map(mut self, entries: HashMap<String, Vec<String>>) -> Self {
        self.entries = entries;
        self
    }

    pub fn load_data(&mut self, data: &str) {
        self.entries = parse_replace_data(data);
    }

    fn message_for(&self, token: &str, replacements: &[String]) -> String {
        self.message
            .replacen("$suggestions", &replacements.join(", "), 1)
            .replacen("$match", token, 1)
    }
}

pub fn parse_replace_data(data: &str) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    for line in data.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('=').collect();
        if parts.len() != 2 {
            continue;
        }
        let replacements: Vec<String> = parts[1].split('|').map(str::to_string).collect();
        for wrong_form in parts[0].split('|') {
            map.insert(wrong_form.to_string(), replacements.clone());
        }
    }
    map
}

fn is_all_uppercase(s: &str) -> bool {
    s.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
}

fn uppercase_first_char(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => {
            let mut out: String = first.to_uppercase().collect();
            out.push_str(chars.as_str());
            out
        }
        None => s.to_string(),
    }
}

fn is_tagged(token: &og_core::AnalyzedTokenReadings) -> bool {
    token.readings().iter().any(|r| !r.pos_tags().is_empty())
}

impl Rule for GaSimpleReplaceRule {
    fn id(&self) -> &str {
        self.id
    }

    fn description(&self) -> &str {
        self.description
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        self.issue_type.clone()
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let mut rule_matches = Vec::new();
        for token in sentence.non_whitespace_tokens() {
            if token.is_sentence_start()
                || token.is_immunized()
                || token.is_ignored_by_speller()
                || (self.ignore_tagged_words && is_tagged(token))
            {
                continue;
            }
            let original = token.token().token();
            let lower = original.to_lowercase();
            let Some(mut replacements) = self
                .entries
                .get(original)
                .or_else(|| self.entries.get(lower.as_str()))
                .cloned()
            else {
                continue;
            };
            if is_all_uppercase(original) {
                replacements = replacements.iter().map(|s| s.to_uppercase()).collect();
            }
            replacements.retain(|r| r != original);
            if replacements.is_empty() {
                continue;
            }
            let message = self.message_for(original, &replacements);
            let mut rm_rule = RuleMatchRule::new(self.id, self.description)
                .with_category(self.category.clone())
                .with_issue_type(self.issue_type.as_str());
            if let Some(url) = self.url {
                rm_rule = rm_rule.with_urls(vec![url.to_string()]);
            }
            if !is_all_uppercase(original)
                && original.chars().next().is_some_and(|c| c.is_uppercase())
            {
                replacements = replacements
                    .iter()
                    .map(|s| uppercase_first_char(s))
                    .collect();
            }
            let from = token.token().start();
            let to = token.token().end();
            rule_matches.push(
                RuleMatch::new(
                    message,
                    from,
                    to - from,
                    rm_rule,
                    super::context_window(sentence, from, to, 40),
                )
                .with_short_message(self.short.to_string())
                .with_replacements(
                    replacements
                        .iter()
                        .map(|r| SuggestedReplacement::new(r))
                        .collect(),
                )
                .with_sentence(sentence.text().to_string()),
            );
        }
        rule_matches
    }
}

const REPLACE_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/rules/ga/replace.txt"
);
const REPLACE_PRESTANDARD_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/rules/ga/replace-prestandard.txt"
);
const REPLACE_FGB_EQ_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/rules/ga/replace-fgb-eq.txt"
);
const ENGLISH_HOMOPHONES_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/rules/ga/english-homophones.txt"
);
const PLACENAMES_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/rules/ga/placenames.txt"
);
const NAMES_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/rules/ga/names.txt"
);
const SPACES_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/rules/ga/spaces.txt"
);

pub fn irish_replace_rule() -> GaSimpleReplaceRule {
    let mut rule = GaSimpleReplaceRule::new(
        "GA_REPLACE",
        "Litriú mícheart, m.sh., \"agsu\" in áit \"agus\"",
        "Litriú mícheart",
        "Litriú mícheart: \"$suggestions\".",
    );
    rule.load_data(REPLACE_TXT);
    rule
}

pub fn prestandard_replace_rule() -> GaSimpleReplaceRule {
    let mut rule = GaSimpleReplaceRule::new(
        "GA_PRESTANDARD_REPLACE",
        "Litriú réamhchaighdeánach, m.sh., \"baoghal\" in áit \"baol\"",
        "Litriú réamhchaighdeánach",
        "Litriú réamhchaighdeánach:\"$suggestions\".",
    );
    rule.load_data(REPLACE_PRESTANDARD_TXT);
    rule
}

pub fn irish_fgb_eq_replace_rule() -> GaSimpleReplaceRule {
    let mut rule = GaSimpleReplaceRule::new(
        "GA_FGB_EQ_REPLACE",
        "Ceannfhocal FGB neamhchoitianta, m.sh., \"urlamh\" in áit \"ullamh\"",
        "Neamhchoitianta",
        "Focal ceart ach tá \"$suggestions\" níos coitianta.",
    );
    rule.load_data(REPLACE_FGB_EQ_TXT);
    rule
}

pub fn english_homophone_rule() -> GaSimpleReplaceRule {
    let mut rule = GaSimpleReplaceRule::new(
        "GA_ENGLISH_HOMOPHONE",
        "Is homafóin iad na focail, m.sh., \"well\" agus \"bhuel\"",
        "Homafón Béarla.",
        "Is é $suggestions an litriú Gaelach ar \"$match\"",
    );
    rule.load_data(ENGLISH_HOMOPHONES_TXT);
    rule
}

pub fn logainm_rule() -> GaSimpleReplaceRule {
    let mut rule = GaSimpleReplaceRule::new(
        "GA_LOGAINM",
        "Logainm Béarla, m.sh., 'Dublin' in áit 'Baile Átha Cliath'.",
        "Logainm Béarla",
        "Is logainm Béarla é \"$match\" ar \"$suggestions\".",
    )
    .with_ignore_tagged_words()
    .with_url("https://www.logainm.ie/ga/");
    rule.load_data(PLACENAMES_TXT);
    rule
}

pub fn people_rule() -> GaSimpleReplaceRule {
    let mut rule = GaSimpleReplaceRule::new(
        "GA_PEOPLE",
        "Ainm Béarla, m.sh., 'Damocles' in áit 'Dámaicléas'.",
        "Ainm Béarla",
        "Ainm Béarla. \"$suggestions\".",
    );
    rule.load_data(NAMES_TXT);
    rule
}

pub fn spaces_rule() -> GaSimpleReplaceRule {
    let mut rule = GaSimpleReplaceRule::new(
        "GA_SPASANNA",
        "Spás ar iarraidh",
        "Spás ar iarraidh",
        "Spás ar iarraidh: \"$suggestions\".",
    )
    .with_ignore_tagged_words();
    rule.load_data(SPACES_TXT);
    rule
}
