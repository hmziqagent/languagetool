use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchRule,
    rule::Rule,
};
use std::collections::HashSet;
use std::sync::LazyLock;

const PEOPLE_TXT: &str = include_str!(
    "../../../../languagetool/languagetool-language-modules/ga/src/main/resources/org/languagetool/rules/ga/people.txt"
);

static DAOINE: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut set = HashSet::new();
    for line in PEOPLE_TXT.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(stripped) = line.strip_prefix('*') {
            set.insert(stripped.to_string());
        } else {
            set.insert(line.to_lowercase());
        }
    }
    set
});

const NUMBER_REPLACEMENTS: [(&str, &str); 9] = [
    ("dhá", "beirt"),
    ("trí", "triúr"),
    ("ceathair", "ceathrar"),
    ("cúig", "cúigear"),
    ("sé", "seisear"),
    ("seacht", "seachtar"),
    ("ocht", "ochtar"),
    ("naoi", "naonúr"),
    ("deich", "deichniúr"),
];

fn eq_ignore_case(a: &str, b: &str) -> bool {
    let mut ai = a.chars().flat_map(char::to_uppercase);
    let mut bi = b.chars().flat_map(char::to_uppercase);
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return true,
            (Some(x), Some(y)) if x == y => {}
            _ => return false,
        }
    }
}

pub struct DhaNoBeirtRule {
    description: String,
    category: Category,
}

impl DhaNoBeirtRule {
    pub fn new() -> Self {
        Self {
            description: "'dhá' nó 'beirt'".to_string(),
            category: Category::new("MISC", "Miscellaneous"),
        }
    }

    fn is_number(token: &str) -> bool {
        NUMBER_REPLACEMENTS
            .iter()
            .any(|(num, _)| eq_ignore_case(num, token))
    }

    fn number_replacement(token: &str) -> Option<&'static str> {
        NUMBER_REPLACEMENTS
            .iter()
            .find(|(num, _)| *num == token)
            .map(|(_, repl)| *repl)
    }

    fn is_person(token: &AnalyzedTokenReadings) -> bool {
        if DAOINE.contains(&token.token().token().to_lowercase()) {
            return true;
        }
        token
            .readings()
            .iter()
            .filter_map(|r| r.lemma())
            .any(|lemma| DAOINE.contains(lemma))
    }

    fn push_match(
        &self,
        sentence: &AnalyzedSentence,
        token: &AnalyzedTokenReadings,
        message: String,
        matches: &mut Vec<RuleMatch>,
    ) {
        let from = token.token().start();
        let to = token.token().end();
        let rm_rule = RuleMatchRule::new("GA_DHA_NO_BEIRT", &self.description)
            .with_category(self.category.clone())
            .with_issue_type(IssueType::Misspelling.as_str());
        matches.push(
            RuleMatch::new(
                message,
                from,
                to - from,
                rm_rule,
                super::context_window(sentence, from, to, 40),
            )
            .with_short_message("Uimhir phearsanta".to_string())
            .with_sentence(sentence.text().to_string()),
        );
    }
}

impl Default for DhaNoBeirtRule {
    fn default() -> Self {
        Self::new()
    }
}

impl Rule for DhaNoBeirtRule {
    fn id(&self) -> &str {
        "GA_DHA_NO_BEIRT"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn category(&self) -> Category {
        self.category.clone()
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Misspelling
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        let mut rule_matches = Vec::new();
        let mut prev_token_index = 0usize;
        let mut replacement: Option<&'static str> = None;
        let mut msg: Option<String> = None;
        for i in 1..tokens.len() {
            if Self::is_number(tokens[i].token().token())
                && i < tokens.len().saturating_sub(2)
                && Self::is_person(tokens[i + 1])
            {
                if eq_ignore_case(tokens[i].token().token(), "dhá") {
                    for j in i + 2..tokens.len() {
                        if eq_ignore_case(tokens[j].token().token(), "déag") {
                            replacement = Some("dháréag");
                            let message =
                                "Ba chóir duit <suggestion>dháréag</suggestion> a scríobh"
                                    .to_string();
                            self.push_match(
                                sentence,
                                tokens[prev_token_index + 1],
                                message,
                                &mut rule_matches,
                            );
                            self.push_match(
                                sentence,
                                tokens[j],
                                "Ba chóir duit \"déag\" a scriosadh.".to_string(),
                                &mut rule_matches,
                            );
                            msg = None;
                        }
                    }
                }
                if replacement.is_none() {
                    replacement = Self::number_replacement(tokens[i].token().token());
                    if msg.is_none() {
                        let repl = replacement.unwrap_or("null");
                        msg = Some(format!(
                            "Ba chóir duit <suggestion>{repl} {}</suggestion> a scríobh",
                            tokens[i + 1].token().token()
                        ));
                    }
                }
            }
            if let Some(message) = msg.take() {
                self.push_match(
                    sentence,
                    tokens[prev_token_index + 1],
                    message,
                    &mut rule_matches,
                );
            }
            prev_token_index = i;
        }
        rule_matches
    }
}
