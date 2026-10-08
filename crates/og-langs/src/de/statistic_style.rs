use super::helpers::has_pos_tag_starting_with;
use og_core::{
    AnalyzedSentence, AnalyzedTokenReadings, Category, IssueType, RuleMatch, RuleMatchContext,
    RuleMatchRule, rule::TextLevelRule,
};
use std::sync::LazyLock;

static OPENING_QUOTES: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^[\"“„»«]$").unwrap());
static ENDING_QUOTES: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^[\"“”»«]$").unwrap());
static MARKS_REGEX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new("^[,;.:?•!-–—]$").unwrap());

fn is_mark(token: &AnalyzedTokenReadings) -> bool {
    MARKS_REGEX.is_match(token.token().token())
}

fn is_opening_quote(token: &AnalyzedTokenReadings) -> bool {
    OPENING_QUOTES.is_match(token.token().token())
}

fn is_whitespace_before(tokens: &[&AnalyzedTokenReadings], i: usize) -> bool {
    i > 0 && tokens[i].token().start() > tokens[i - 1].token().end()
}

fn round_percent(percent: f64) -> i32 {
    (percent + 0.5) as i32
}

#[derive(Clone, Copy, PartialEq)]
pub enum SentenceCondition {
    Passive,
    ModalVerb,
    Man,
    ConjunctionAtBegin,
}

pub struct StatisticSentenceStyleRule {
    condition: SentenceCondition,
    min_percent: i32,
    denominator: f64,
}

impl StatisticSentenceStyleRule {
    pub fn new(condition: SentenceCondition) -> Self {
        match condition {
            SentenceCondition::Passive => Self {
                condition,
                min_percent: 8,
                denominator: 100.0,
            },
            SentenceCondition::ModalVerb => Self {
                condition,
                min_percent: 18,
                denominator: 100.0,
            },
            SentenceCondition::Man => Self {
                condition,
                min_percent: 15,
                denominator: 1000.0,
            },
            SentenceCondition::ConjunctionAtBegin => Self {
                condition,
                min_percent: 10,
                denominator: 100.0,
            },
        }
    }

    fn id_for(&self) -> &'static str {
        match self.condition {
            SentenceCondition::Passive => "PASSIVE_SENTENCE_DE",
            SentenceCondition::ModalVerb => "SENTENCE_WITH_MODAL_VERB_DE",
            SentenceCondition::Man => "SENTENCE_WITH_MAN_DE",
            SentenceCondition::ConjunctionAtBegin => "SENTENCE_BEGINNING_WITH_CONJUNCTION_DE",
        }
    }

    fn description_for(&self) -> &'static str {
        match self.condition {
            SentenceCondition::Passive => "Statistische Stilanalyse: Passivsätze",
            SentenceCondition::ModalVerb => "Statistische Stilanalyse: Sätze mit Modalverben",
            SentenceCondition::Man => {
                "Statistische Stilanalyse: Sätze mit indirekter Leseransprache 'man'"
            }
            SentenceCondition::ConjunctionAtBegin => {
                "Statistische Stilanalyse: Sätze beginnend mit Konjunktion"
            }
        }
    }

    fn limit_message(&self, limit: i32, percent: f64) -> String {
        match self.condition {
            SentenceCondition::Passive => {
                if limit == 0 {
                    "Passivsatz: Aktiv formulierte Sätze sprechen im Regelfall den Leser stärker an.".to_string()
                } else {
                    format!(
                        "Mehr als {limit}% Passivsätze {{{}%}} gefunden. Aktiv formulierte Sätze sprechen im Regelfall den Leser stärker an.",
                        round_percent(percent)
                    )
                }
            }
            SentenceCondition::ModalVerb => {
                if limit == 0 {
                    "Modalverb: Modalverben blähen den Text häufig auf und sollten vermieden werden.".to_string()
                } else {
                    format!(
                        "Mehr als {limit}% Sätze mit Modalverben {{{}%}} gefunden. Modalverben blähen den Text häufig auf und sollten vermieden werden.",
                        round_percent(percent)
                    )
                }
            }
            SentenceCondition::Man => {
                if limit == 0 {
                    "Sätze mit der indirekten Leseransprache 'man' sind stilistisch wenig elegant formuliert. Lässt sich das Wort vermeiden?".to_string()
                } else {
                    format!(
                        "Mehr als {limit}‰ Sätze mit der indirekten Leseransprache 'man' {{{}‰}} gefunden. Lässt sich das Wort vermeiden?",
                        round_percent(percent)
                    )
                }
            }
            SentenceCondition::ConjunctionAtBegin => {
                if limit == 0 {
                    "Eine Konjunktion sollte nur in Ausnahmefällen am Satzanfang verwendet werden. Formulieren Sie den Satz um, falls möglich.".to_string()
                } else {
                    format!(
                        "Mehr als {limit}% Sätze beginnen mit einer Konjunktion {{{}%}} gefunden. Formulieren Sie den Satz um, falls möglich.",
                        round_percent(percent)
                    )
                }
            }
        }
    }

    fn condition_fulfilled<'t>(
        &self,
        sentence: &'t [&'t AnalyzedTokenReadings],
    ) -> Option<&'t AnalyzedTokenReadings> {
        match self.condition {
            SentenceCondition::Passive => {
                let mut i = 0;
                while i < sentence.len() {
                    if sentence[i].has_lemma("werden") {
                        let token = sentence[i];
                        i += 1;
                        while i < sentence.len() {
                            if has_pos_tag_starting_with(sentence[i], "VER:PA2:") {
                                return Some(token);
                            } else if is_mark(sentence[i]) {
                                return None;
                            }
                            i += 1;
                        }
                    } else if has_pos_tag_starting_with(sentence[i], "VER:PA2:") {
                        i += 1;
                        while i < sentence.len() {
                            if sentence[i].has_lemma("werden") {
                                return Some(sentence[i]);
                            } else if is_mark(sentence[i]) {
                                return None;
                            }
                            i += 1;
                        }
                    }
                    i += 1;
                }
                None
            }
            SentenceCondition::ModalVerb => {
                let mut i = 0;
                while i < sentence.len() {
                    if has_pos_tag_starting_with(sentence[i], "VER:MOD") {
                        let token = sentence[i];
                        i += 1;
                        while i < sentence.len() {
                            if has_pos_tag_starting_with(sentence[i], "VER:INF") {
                                return Some(token);
                            } else if is_mark(sentence[i]) {
                                return None;
                            }
                            i += 1;
                        }
                    } else if has_pos_tag_starting_with(sentence[i], "VER:INF") {
                        i += 1;
                        while i < sentence.len() {
                            if has_pos_tag_starting_with(sentence[i], "VER:MOD") {
                                return Some(sentence[i]);
                            } else if is_mark(sentence[i]) {
                                return None;
                            }
                            i += 1;
                        }
                    }
                    i += 1;
                }
                None
            }
            SentenceCondition::Man => sentence.iter().find(|t| t.has_lemma("man")).copied(),
            SentenceCondition::ConjunctionAtBegin => {
                if sentence.len() < 3 {
                    return None;
                }
                let mut num = 0;
                if is_opening_quote(sentence[0]) {
                    num += 1;
                }
                if !has_pos_tag_starting_with(sentence[num], "KON") {
                    return None;
                }
                let token = sentence[num];
                let token_text = token.token().token();
                if token_text == "Wie"
                    || token_text == "Seit"
                    || token_text == "Allerdings"
                    || (token_text == "Aber"
                        && sentence
                            .get(num + 1)
                            .is_some_and(|t| t.token().token() == "auch"))
                {
                    return None;
                }
                if token_text == "Um" {
                    for t in sentence.iter().skip(1) {
                        if t.token().token() == "," || t.token().token() == "herum" {
                            return None;
                        }
                    }
                    return Some(token);
                }
                if !has_pos_tag_starting_with(token, "KON:UNT")
                    || token_text == "Sondern"
                    || (token_text == "Auch"
                        && sentence
                            .get(num + 1)
                            .is_some_and(|t| t.token().token() == "wenn"))
                {
                    if token_text == "Entweder" {
                        if sentence.iter().any(|t| t.token().token() == "oder") {
                            return None;
                        }
                    } else if token_text == "Sowohl" {
                        for window in sentence.windows(2).skip(1) {
                            if window[0].token().token() == "als"
                                && window[1].token().token() == "auch"
                            {
                                return None;
                            }
                        }
                    } else if token_text == "Weder" {
                        if sentence.iter().any(|t| t.token().token() == "noch") {
                            return None;
                        }
                    } else {
                        if sentence.last().is_some_and(|t| t.token().token() == "?") {
                            return None;
                        }
                        return Some(token);
                    }
                }
                for t in sentence.iter().skip(2) {
                    if t.token().token() == "," {
                        return None;
                    }
                }
                Some(token)
            }
        }
    }
}

impl TextLevelRule for StatisticSentenceStyleRule {
    fn id(&self) -> &str {
        self.id_for()
    }

    fn description(&self) -> &str {
        self.description_for()
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        Category::new("CREATIVE_WRITING", "Stiltipps für kreatives Schreiben")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut rule_matches = Vec::new();
        let mut start_pos: Vec<usize> = Vec::new();
        let mut end_pos: Vec<usize> = Vec::new();
        let mut relevant_sentence_texts: Vec<&AnalyzedSentence> = Vec::new();
        let mut sentence_count = 0usize;
        let exclude_direct_speech = true;
        let mut is_direct_speech = false;
        for sentence in sentences {
            let tokens = sentence.non_whitespace_tokens();
            let mut relevant_sentence_part: Vec<&AnalyzedTokenReadings> = Vec::new();
            let mut is_sentence_count = false;
            let mut found: Option<(usize, usize)> = None;
            for n in 1..tokens.len() {
                let token = tokens[n];
                let s_token = token.token().token();
                if exclude_direct_speech
                    && !is_direct_speech
                    && OPENING_QUOTES.is_match(s_token)
                    && n < tokens.len() - 1
                    && !is_whitespace_before(&tokens, n + 1)
                {
                    is_direct_speech = true;
                    if !relevant_sentence_part.is_empty() {
                        is_sentence_count = true;
                        if let Some(found_token) = self.condition_fulfilled(&relevant_sentence_part)
                        {
                            found = Some((found_token.token().start(), found_token.token().end()));
                            break;
                        }
                    }
                } else if exclude_direct_speech
                    && is_direct_speech
                    && ENDING_QUOTES.is_match(s_token)
                    && n > 1
                    && !is_whitespace_before(&tokens, n)
                {
                    is_direct_speech = false;
                    relevant_sentence_part.clear();
                } else if !is_direct_speech {
                    relevant_sentence_part.push(token);
                }
                if n == tokens.len() - 1 && !relevant_sentence_part.is_empty() {
                    is_sentence_count = true;
                    if let Some(found_token) = self.condition_fulfilled(&relevant_sentence_part) {
                        found = Some((found_token.token().start(), found_token.token().end()));
                    }
                }
            }
            if is_sentence_count {
                sentence_count += 1;
            }
            if let Some((start, end)) = found {
                start_pos.push(start);
                end_pos.push(end);
                relevant_sentence_texts.push(sentence);
            }
        }
        let num_matches = start_pos.len();
        let percent = if sentence_count > 0 {
            (num_matches as f64 * self.denominator) / sentence_count as f64
        } else {
            0.0
        };
        if percent > self.min_percent as f64 {
            for (i, sentence) in relevant_sentence_texts.iter().enumerate() {
                let rm_rule = RuleMatchRule::new(self.id(), self.description())
                    .with_category(self.category())
                    .with_issue_type(self.issue_type().as_str());
                rule_matches.push(
                    RuleMatch::new(
                        self.limit_message(self.min_percent, percent),
                        start_pos[i],
                        end_pos[i] - start_pos[i],
                        rm_rule,
                        RuleMatchContext::new(
                            sentence.text().to_string(),
                            start_pos[i],
                            end_pos[i] - start_pos[i],
                        ),
                    )
                    .with_sentence(sentence.text().to_string()),
                );
            }
        }
        rule_matches
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum WordCondition {
    FillerWords,
    NonSignificantVerbs,
    UnnecessaryPhrases,
}

const NON_SIGNIFICANT: [&str; 4] = ["haben", "sein", "machen", "tun"];

const UNNECESSARY_PHRASES: [&[&str]; 22] = [
    &["dann", "und", "wann"],
    &["des", "Ungeachtet"],
    &["ganz", "und", "gar"],
    &["hie", "und", "da"],
    &["im", "Allgemeinen"],
    &["in", "der", "Tat"],
    &["in", "diesem", "Zusammenhang"],
    &["mehr", "oder", "weniger"],
    &["meines", "Erachtens"],
    &["ohne", "weiteres"],
    &["ohne", "Zweifel"],
    &["samt", "und", "sonders"],
    &["sowohl", "als", "auch"],
    &["voll", "und", "ganz"],
    &["von", "Neuem"],
    &["allem", "Anschein", "nach"],
    &["aufs", "Neue"],
    &["ein", "bisschen"],
    &["ein", "wenig"],
    &["des", "Öfteren"],
    &["bei", "weitem"],
    &["an", "sich"],
];

static FILLER_WORDS: LazyLock<std::collections::HashSet<&'static str>> = LazyLock::new(|| {
    [
        "aber",
        "abermals",
        "allein",
        "allemal",
        "allenfalls",
        "allenthalben",
        "allerdings",
        "allesamt",
        "allzu",
        "also",
        "alt",
        "andauernd",
        "andererseits",
        "andernfalls",
        "anscheinend",
        "auch",
        "auffallend",
        "augenscheinlich",
        "ausdrücklich",
        "ausgerechnet",
        "ausnahmslos",
        "außerdem",
        "äußerst",
        "beinahe",
        "bekanntlich",
        "bereits",
        "besonders",
        "bestenfalls",
        "bestimmt",
        "bloß",
        "dabei",
        "dadurch",
        "dafür",
        "dagegen",
        "daher",
        "damals",
        "danach",
        "demgegenüber",
        "demgemäß",
        "demnach",
        "denkbar",
        "denn",
        "dennoch",
        "deshalb",
        "deswegen",
        "doch",
        "durchaus",
        "durchweg",
        "eben",
        "eigentlich",
        "einerseits",
        "einfach",
        "einige",
        "einigermaßen",
        "einmal",
        "ergo",
        "erheblich",
        "etliche",
        "etwa",
        "etwas",
        "fast",
        "folgendermaßen",
        "folglich",
        "förmlich",
        "fortwährend",
        "fraglos",
        "freilich",
        "ganz",
        "gänzlich",
        "gar",
        "gelegentlich",
        "gemeinhin",
        "genau",
        "geradezu",
        "gewiss",
        "gewissermaßen",
        "glatt",
        "gleichsam",
        "gleichwohl",
        "glücklicherweise",
        "gottseidank",
        "größtenteils",
        "häufig",
        "hingegen",
        "hinlänglich",
        "höchst",
        "höchstens",
        "immer",
        "immerhin",
        "immerzu",
        "indessen",
        "infolgedessen",
        "insbesondere",
        "inzwischen",
        "irgend",
        "irgendein",
        "irgendjemand",
        "irgendwann",
        "irgendwie",
        "irgendwo",
        "ja",
        "je",
        "jedenfalls",
        "jedoch",
        "jemals",
        "kaum",
        "keinesfalls",
        "keineswegs",
        "längst",
        "lediglich",
        "leider",
        "letztlich",
        "manchmal",
        "mehrfach",
        "meinetwegen",
        "meist",
        "meistens",
        "meistenteils",
        "mindestens",
        "mithin",
        "mitunter",
        "möglicherweise",
        "möglichst",
        "nämlich",
        "naturgemäß",
        "natürlich",
        "neuerdings",
        "neuerlich",
        "neulich",
        "nichtsdestoweniger",
        "nie",
        "niemals",
        "nun",
        "nur",
        "offenbar",
        "offenkundig",
        "offensichtlich",
        "oft",
        "ohnedies",
        "partout",
        "plötzlich",
        "praktisch",
        "quasi",
        "recht",
        "reichlich",
        "reiflich",
        "relativ",
        "restlos",
        "richtiggehend",
        "rundheraus",
        "rundum",
        "sattsam",
        "schlicht",
        "schlichtweg",
        "schließlich",
        "schlussendlich",
        "schon",
        "sehr",
        "selbst",
        "selbstredend",
        "selbstverständlich",
        "selten",
        "seltsamerweise",
        "sicher",
        "sicherlich",
        "so",
        "sogar",
        "sonst",
        "sowieso",
        "sozusagen",
        "stellenweise",
        "stets",
        "trotzdem",
        "überaus",
        "überdies",
        "überhaupt",
        "übrigens",
        "umständehalber",
        "unbedingt",
        "unerhört",
        "ungefähr",
        "ungemein",
        "ungewöhnlich",
        "ungleich",
        "unglücklicherweise",
        "unlängst",
        "unmaßgeblich",
        "unsagbar",
        "unsäglich",
        "unstreitig",
        "unzweifelhaft",
        "vergleichsweise",
        "vermutlich",
        "vielfach",
        "vielleicht",
        "voll",
        "vollends",
        "völlig",
        "vollkommen",
        "vollständig",
        "wahrscheinlich",
        "weidlich",
        "weitgehend",
        "wenigstens",
        "wieder",
        "wiederum",
        "wirklich",
        "wohl",
        "wohlgemerkt",
        "womöglich",
        "ziemlich",
        "zudem",
        "zugegeben",
        "zumeist",
        "zusehends",
        "zuweilen",
        "zweifellos",
        "zweifelsfrei",
        "zweifelsohne",
    ]
    .into_iter()
    .collect()
});

pub struct StatisticWordStyleRule {
    condition: WordCondition,
    min_percent: i32,
    denominator: f64,
}

impl StatisticWordStyleRule {
    pub fn new(condition: WordCondition) -> Self {
        match condition {
            WordCondition::FillerWords => Self {
                condition,
                min_percent: 8,
                denominator: 100.0,
            },
            WordCondition::NonSignificantVerbs => Self {
                condition,
                min_percent: 8,
                denominator: 1000.0,
            },
            WordCondition::UnnecessaryPhrases => Self {
                condition,
                min_percent: 8,
                denominator: 10000.0,
            },
        }
    }

    fn id_for(&self) -> &'static str {
        match self.condition {
            WordCondition::FillerWords => "FILLER_WORDS_DE",
            WordCondition::NonSignificantVerbs => "NON_SIGNIFICANT_VERB_DE",
            WordCondition::UnnecessaryPhrases => "UNNECESSARY_PHRASES_DE",
        }
    }

    fn description_for(&self) -> &'static str {
        match self.condition {
            WordCondition::FillerWords => "Statistische Stilanalyse: Füllwörter",
            WordCondition::NonSignificantVerbs => {
                "Statistische Stilanalyse: Verben mit wenig Aussagekraft"
            }
            WordCondition::UnnecessaryPhrases => "Statistische Stilanalyse: Potenzielle Phrasen",
        }
    }

    fn limit_message(&self, limit: i32, percent: f64) -> String {
        match self.condition {
            WordCondition::FillerWords => {
                if limit == 0 {
                    "Dieses Wort könnte ein Füllwort sein. Möglicherweise ist es besser es zu löschen.".to_string()
                } else {
                    format!(
                        "Mehr als {limit}% Füllwörter {{{}%}} gefunden. Möglicherweise ist es besser dieses potentielle Füllwort zu löschen.",
                        round_percent(percent)
                    )
                }
            }
            WordCondition::NonSignificantVerbs => {
                if limit == 0 {
                    "Dieses Verb hat wenig Aussagekraft. Verwenden Sie wenn möglich ein anderes oder formulieren Sie den Satz um.".to_string()
                } else {
                    format!(
                        "Mehr als {limit}‰ wenig aussagekräftige Verben {{{}‰}} gefunden. Verwenden Sie wenn möglich ein anderes Verb oder formulieren Sie den Satz um.",
                        round_percent(percent)
                    )
                }
            }
            WordCondition::UnnecessaryPhrases => {
                if limit == 0 {
                    "Der Ausdruck gilt als Phrase. Es wird empfohlen ihn zu löschen, falls möglich."
                        .to_string()
                } else {
                    format!(
                        "Mehr als {limit}‱ potenzielle Phrasen {{{}‱}} gefunden. Es wird empfohlen den Ausdruck zu löschen, falls möglich.",
                        round_percent(percent)
                    )
                }
            }
        }
    }

    fn is_unknown_word(token: &AnalyzedTokenReadings) -> bool {
        super::helpers::is_pos_tag_unknown(token)
            && token.token().token().chars().count() > 2
            && token
                .token()
                .token()
                .chars()
                .all(|c| c.is_ascii_alphabetic() || "ÄÖÜäöüß".contains(c))
    }

    fn filler_is_exception(tokens: &[&AnalyzedTokenReadings], num: usize) -> bool {
        if num == 1 || tokens[num - 1].token().token() == "," {
            return true;
        }
        let token_text = tokens[num].token().token();
        if token_text == "allein" {
            return tokens.iter().any(|t| t.has_lemma("sein"));
        }
        if token_text == "recht"
            && tokens
                .iter()
                .any(|t| t.has_lemma("haben") || t.has_lemma("geben"))
        {
            return true;
        }
        if num < tokens.len() - 1
            && matches!(token_text, "so" | "besonders")
            && has_pos_tag_starting_with(tokens[num + 1], "ADJ")
        {
            return true;
        }
        if has_pos_tag_starting_with(tokens[num], "ADJ") && tokens[num - 1].token().token() == "so"
        {
            return true;
        }
        if token_text == "nur" && tokens[num - 1].token().token() == "nicht" {
            for i in num + 1..tokens.len().saturating_sub(2) {
                if tokens[i].token().token() == ","
                    && (tokens[i + 1].token().token() == "auch"
                        || (tokens[i + 1].token().token() == "sondern"
                            && tokens
                                .get(i + 2)
                                .is_some_and(|t| t.token().token() == "auch")))
                {
                    return true;
                }
            }
        }
        if num > 2
            && token_text == "auch"
            && tokens[num - 1].token().token() == "sondern"
            && tokens[num - 2].token().token() == ","
        {
            for i in 1..num - 2 {
                if tokens[i].token().token() == "nicht" && tokens[i + 1].token().token() == "nur" {
                    return true;
                }
            }
        }
        false
    }

    fn is_two_word_exception(first: &str, second: &str) -> bool {
        (first == "aber" && (second == "nur" || second == "auch"))
            || (first == "auch" && second == "nur")
            || (first == "immer" && second == "wieder")
            || (first == "genau" && second == "so")
            || (first == "so" && matches!(second, "etwas" | "viel" | "oft"))
            || (first == "schon" && second == "fast")
    }

    fn filler_condition(tokens: &[&AnalyzedTokenReadings], n: usize) -> bool {
        FILLER_WORDS.contains(tokens[n].token().token())
            && !Self::filler_is_exception(tokens, n)
            && (n < 2
                || !Self::is_two_word_exception(
                    tokens[n - 1].token().token(),
                    tokens[n].token().token(),
                ))
            && (n + 2 > tokens.len()
                || !Self::is_two_word_exception(
                    tokens[n].token().token(),
                    tokens[n + 1].token().token(),
                ))
    }

    fn verb_is_exception(tokens: &[&AnalyzedTokenReadings], num: usize) -> bool {
        let token_text = tokens[num].token().token();
        if token_text.starts_with("sein") || token_text.starts_with("Sein") {
            return true;
        }
        if tokens[num].has_lemma("machen")
            && tokens.iter().any(|t| {
                matches!(
                    t.token().token(),
                    "Angst" | "Weg" | "frisch" | "bemerkbar" | "aufmerksam"
                )
            })
        {
            return true;
        }
        let is_haben = tokens[num].has_lemma("haben");
        if is_haben
            && tokens.iter().any(|t| {
                matches!(
                    t.token().token(),
                    "Glück" | "Angst" | "Mühe" | "Recht" | "recht"
                )
            })
        {
            return true;
        }
        if is_haben || tokens[num].has_lemma("sein") {
            for t in tokens.iter() {
                if has_pos_tag_starting_with(t, "PA2")
                    || has_pos_tag_starting_with(t, "VER:PA2")
                    || t.token().token() == "Flucht"
                    || Self::is_unknown_word(t)
                {
                    return true;
                }
            }
        }
        false
    }

    fn first_char_to_lower(tokens: &[&AnalyzedTokenReadings], n: usize) -> String {
        let token = tokens[n].token().token();
        if n != 1 || token.chars().count() < 2 {
            token.to_string()
        } else {
            super::helpers::lowercase_first_char(token)
        }
    }

    fn phrase_is_exception(tokens: &[&AnalyzedTokenReadings], phrase: &[&str]) -> bool {
        phrase.len() == 2
            && phrase[0] == "an"
            && phrase[1] == "sich"
            && tokens.iter().any(|t| t.has_lemma("drücken"))
    }

    fn condition_fulfilled(&self, tokens: &[&AnalyzedTokenReadings], n: usize) -> Option<usize> {
        match self.condition {
            WordCondition::FillerWords => {
                if Self::filler_condition(tokens, n) {
                    Some(n)
                } else {
                    None
                }
            }
            WordCondition::NonSignificantVerbs => {
                if NON_SIGNIFICANT.iter().any(|l| tokens[n].has_lemma(l))
                    && !Self::verb_is_exception(tokens, n)
                {
                    Some(n)
                } else {
                    None
                }
            }
            WordCondition::UnnecessaryPhrases => {
                for phrase in UNNECESSARY_PHRASES.iter() {
                    let mut j = 0;
                    while j < phrase.len()
                        && n + j < tokens.len()
                        && phrase[j] == Self::first_char_to_lower(tokens, n + j)
                    {
                        j += 1;
                    }
                    if j == phrase.len() {
                        if Self::phrase_is_exception(tokens, phrase) {
                            return None;
                        }
                        return Some(n + phrase.len() - 1);
                    }
                }
                None
            }
        }
    }
}

impl TextLevelRule for StatisticWordStyleRule {
    fn id(&self) -> &str {
        self.id_for()
    }

    fn description(&self) -> &str {
        self.description_for()
    }

    fn is_default_on(&self) -> bool {
        false
    }

    fn category(&self) -> Category {
        Category::new("CREATIVE_WRITING", "Stiltipps für kreatives Schreiben")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Style
    }

    fn match_text(&self, _text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch> {
        let mut rule_matches = Vec::new();
        let mut start_pos: Vec<usize> = Vec::new();
        let mut end_pos: Vec<usize> = Vec::new();
        let mut relevant_sentence_texts: Vec<&AnalyzedSentence> = Vec::new();
        let mut word_count = 0usize;
        let exclude_direct_speech = true;
        let mut is_direct_speech = false;
        for sentence in sentences {
            let tokens = sentence.non_whitespace_tokens();
            for n in 1..tokens.len() {
                let token = tokens[n];
                let s_token = token.token().token();
                if exclude_direct_speech
                    && !is_direct_speech
                    && OPENING_QUOTES.is_match(s_token)
                    && n < tokens.len() - 1
                    && !is_whitespace_before(&tokens, n + 1)
                {
                    is_direct_speech = true;
                } else if exclude_direct_speech
                    && is_direct_speech
                    && ENDING_QUOTES.is_match(s_token)
                    && n > 1
                    && !is_whitespace_before(&tokens, n)
                {
                    is_direct_speech = false;
                } else if !is_direct_speech && s_token.chars().any(|c| c.is_alphabetic()) {
                    word_count += 1;
                    if let Some(n_end) = self.condition_fulfilled(&tokens, n) {
                        start_pos.push(token.token().start());
                        end_pos.push(tokens[n_end].token().end());
                        relevant_sentence_texts.push(sentence);
                    }
                }
            }
        }
        let num_matches = start_pos.len() + rule_matches.len();
        let percent = if word_count > 0 {
            (num_matches as f64 * self.denominator) / word_count as f64
        } else {
            0.0
        };
        if percent > self.min_percent as f64 {
            for (i, sentence) in relevant_sentence_texts.iter().enumerate() {
                let rm_rule = RuleMatchRule::new(self.id(), self.description())
                    .with_category(self.category())
                    .with_issue_type(self.issue_type().as_str());
                rule_matches.push(
                    RuleMatch::new(
                        self.limit_message(self.min_percent, percent),
                        start_pos[i],
                        end_pos[i] - start_pos[i],
                        rm_rule,
                        RuleMatchContext::new(
                            sentence.text().to_string(),
                            start_pos[i],
                            end_pos[i] - start_pos[i],
                        ),
                    )
                    .with_sentence(sentence.text().to_string()),
                );
            }
        }
        rule_matches
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn passive_sentences_flagged_when_frequent() {
        let engine = crate::engine::LanguageEngine::shared_for_code("de").expect("german engine");
        let text = "Das Haus wurde gebaut. Der Zaun wurde gestrichen. Das Auto wurde gewaschen. "
            .repeat(3);
        let mut request = og_core::CheckRequest::new(&text, engine.language().clone());
        request.enabled_rules = vec!["PASSIVE_SENTENCE_DE".to_string()];
        request.picky = true;
        let matches = engine.check_with_request(&request);
        assert!(
            matches
                .matches
                .iter()
                .any(|m| m.rule().id() == "PASSIVE_SENTENCE_DE"),
            "expected passive sentence matches"
        );
    }

    #[test]
    fn filler_words_flagged_when_frequent() {
        let engine = crate::engine::LanguageEngine::shared_for_code("de").expect("german engine");
        let text = "Das ist eigentlich ganz gut. Es war vielleicht so. Das ist wirklich sehr gut. "
            .repeat(3);
        let mut request = og_core::CheckRequest::new(&text, engine.language().clone());
        request.enabled_rules = vec!["FILLER_WORDS_DE".to_string()];
        request.picky = true;
        let matches = engine.check_with_request(&request);
        assert!(
            matches
                .matches
                .iter()
                .any(|m| m.rule().id() == "FILLER_WORDS_DE"),
            "expected filler word matches"
        );
    }
}
