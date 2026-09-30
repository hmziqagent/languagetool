use crate::openregex::PhraseTag;
use crate::openregex::Token;
use crate::openregex::apply;
use crate::openregex::build_expanded;
use crate::openregex::lru_cache::BoundedMap;
use crate::openregex::sentence_forms;
use og_core::AnalyzedSentence;
use std::sync::Arc;
use std::sync::OnceLock;

const FILTER_TAGS: [&str; 3] = ["PP", "NPP", "NPS"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum PhraseType {
    Np,
    Nps,
    Npp,
    Pp,
}

impl PhraseType {
    fn tag(self) -> PhraseTag {
        match self {
            PhraseType::Np => PhraseTag::Bi {
                start: "B-NP",
                inner: "I-NP",
            },
            PhraseType::Nps => PhraseTag::Flat("NPS"),
            PhraseType::Npp => PhraseTag::Flat("NPP"),
            PhraseType::Pp => PhraseTag::Flat("PP"),
        }
    }
}

fn is_hint_char(c: char) -> bool {
    matches!(
        c,
        'a'..='z' | 'A'..='Z' | 'ä' | 'ö' | 'ü' | 'ß' | 'Ä' | 'Ö' | 'Ü' | '|' | '(' | ')'
            | '[' | ']' | '?' | ','
    )
}

fn possible_values(body: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = body.split('|').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    if parts.iter().any(|p| {
        p.chars().any(|c| {
            matches!(
                c,
                '(' | ')' | '[' | ']' | '?' | '\\' | '.' | '*' | '+' | '^' | '$' | '{' | '}'
            )
        })
    }) {
        return None;
    }
    Some(parts.iter().map(|p| p.to_lowercase()).collect())
}

fn calc_form_hints(expanded: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = expanded.chars().collect();
    let mut hints = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '<' {
            i += 1;
            continue;
        }
        let before_ok = i == 0 || chars[i - 1] == ' ';
        let mut j = i + 1;
        while j < chars.len() && chars[j] != '>' {
            j += 1;
        }
        if j >= chars.len() {
            break;
        }
        let body: String = chars[i + 1..j].iter().collect();
        let after = if j + 1 < chars.len() && chars[j + 1] == '+' {
            j + 2
        } else {
            j + 1
        };
        let after_ok = after >= chars.len() || chars[after] == ' ';
        if before_ok
            && after_ok
            && !body.is_empty()
            && body.chars().all(is_hint_char)
            && let Some(values) = possible_values(&body)
        {
            hints.push(values);
        }
        i = j + 1;
    }
    hints
}

fn expand(expr: &str) -> String {
    expr.replace("<NP>", "<chunk=B-NP> <chunk=I-NP>*")
        .replace("&prozent;", "Prozent|Kilo|Kilogramm|Gramm|Euro|Pfund")
}

fn build(expr: &str, phrase: PhraseType, overwrite: bool) -> crate::openregex::Rule {
    let expanded = expand(expr);
    let hints = calc_form_hints(&expanded);
    build_expanded(&expanded, phrase.tag(), overwrite, &FILTER_TAGS, hints)
}

fn build_with_hints(
    expr: &str,
    phrase: PhraseType,
    overwrite: bool,
    form_hints: Vec<Vec<String>>,
) -> crate::openregex::Rule {
    let expanded = expand(expr);
    build_expanded(&expanded, phrase.tag(), overwrite, &FILTER_TAGS, form_hints)
}

fn rules() -> &'static [crate::openregex::Rule] {
    static RULES: OnceLock<Vec<crate::openregex::Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut v = Vec::new();
        let und_oder_bzw = vec![vec![
            "und".to_string(),
            "oder".to_string(),
            "bzw".to_string(),
        ]];
        v.push(build(
            r"(<posre=^ART.*>|<pos=PRO>)? <pos=ADV>* <pos=PA2>* <pos=ADJ>* <pos=SUB>+",
            PhraseType::Np,
            false,
        ));
        v.push(build_with_hints(
            r"<pos=SUB> (<und|oder>|(<bzw> <.>)) <pos=SUB>",
            PhraseType::Np,
            false,
            und_oder_bzw.clone(),
        ));
        v.push(build_with_hints(
            r"<pos=ADJ> (<und|oder>|(<bzw> <.>)) <pos=PA2> <pos=SUB>",
            PhraseType::Np,
            false,
            und_oder_bzw.clone(),
        ));
        v.push(build_with_hints(
            r"<pos=ADJ> (<und|oder>|(<bzw> <.>)) <pos=ADJ> <pos=SUB>",
            PhraseType::Np,
            false,
            und_oder_bzw,
        ));
        v.push(build(
            r"<posre=^ART.*> <pos=ADV>* <pos=ADJ>* <regexCS=[A-ZÖÄÜ][a-zöäü]+>",
            PhraseType::Np,
            false,
        ));
        v.push(build(
            r"<pos=PRO>? <pos=ZAL> <pos=SUB>",
            PhraseType::Np,
            false,
        ));
        v.push(build(
            r"<Herr|Herrn|Frau> <pos=EIG>+",
            PhraseType::Np,
            false,
        ));
        v.push(build(
            r"<Herr|Herrn|Frau> <regexCS=[A-ZÖÄÜ][a-zöäü-]+>+",
            PhraseType::Np,
            false,
        ));
        v.push(build(r"<der>", PhraseType::Np, false));
        v
    })
}

fn rules2() -> &'static [crate::openregex::Rule] {
    static RULES: OnceLock<Vec<crate::openregex::Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let mut v = Vec::new();
        let mut add = |e: &str, p: PhraseType, o: bool| v.push(build(e, p, o));
        add(
            r"<pos=ADJ> <,> <chunk=B-NP> <chunk=I-NP>* <und|sowie> <NP>",
            PhraseType::Npp,
            false,
        );
        add(
            r"<chunk=B-NP & !regex=jede[rs]?> <chunk=I-NP>* <und|sowie> <pos=ADV>? <NP>",
            PhraseType::Npp,
            false,
        );
        add(
            r"<pos=ADJ> <und|sowie> <chunk=B-NP & !pos=PLU> <chunk=I-NP>*",
            PhraseType::Nps,
            true,
        );
        add(
            r"<deren> <chunk=B-NP & !pos=PLU> <und|sowie> <chunk=B-NP>*",
            PhraseType::Nps,
            true,
        );
        add(r"<pos=EIG> <und> <pos=EIG>", PhraseType::Npp, false);
        add(
            r"<pos=ART> <pos=ADJ> <und|sowie> (<pos=ADJ>|<pos=PA2>) <chunk=I-NP & !pos=PLU>+",
            PhraseType::Nps,
            true,
        );
        add(
            r"<chunk=B-NP & !pos=PLU> <chunk=I-NP>* <und|sowie> <keine> <chunk=I-NP>+",
            PhraseType::Nps,
            true,
        );
        add(
            r"<NP> <und|sowie> <pos=ART> <pos=PA1> <pos=SUB>",
            PhraseType::Npp,
            true,
        );
        add(
            r"<eins|eines> <chunk=B-NP> <chunk=I-NP>+",
            PhraseType::Nps,
            false,
        );
        add(
            r"<ich|du|er|sie|es|wir|ihr|sie> <und|oder|sowie> <NP>",
            PhraseType::Npp,
            false,
        );
        add(r"<sowohl> <NP> <als> <auch> <NP>", PhraseType::Npp, false);
        add(
            r"<sowohl> <pos=EIG> <als> <auch> <pos=EIG>",
            PhraseType::Npp,
            false,
        );
        add(
            r"<sowohl> <ich|du|er|sie|es|wir|ihr|sie> <als> <auch> <NP>",
            PhraseType::Npp,
            false,
        );
        add(
            r"<pos=SUB> <und|oder|sowie> <chunk=B-NP & !ihre> <chunk=I-NP>*",
            PhraseType::Npp,
            false,
        );
        add(r"<weder> <pos=SUB> <noch> <pos=SUB>", PhraseType::Npp, false);
        add(
            r"<zwei|drei|vier|fünf|sechs|sieben|acht|neun|zehn|elf|zwölf> <chunk=I-NP>",
            PhraseType::Npp,
            false,
        );
        add(
            r"<chunk=B-NP> <pos=PRP> <NP> <chunk=B-NP & pos=SIN> <chunk=I-NP>*",
            PhraseType::Nps,
            false,
        );
        add(
            r"<chunk=B-NP> <pos=PRP> <NP> <chunk=B-NP & pos=PLU> <chunk=I-NP>*",
            PhraseType::Npp,
            false,
        );
        add(
            r"<chunk=B-NP> <pos=PRP> <NP> <pos=PA2> <chunk=B-NP & !pos=PLU> <chunk=I-NP>*",
            PhraseType::Nps,
            false,
        );
        add(
            r"<chunk=B-NP> <pos=PRP> <NP> <pos=PA2> <chunk=B-NP & !pos=SIN> <chunk=I-NP>*",
            PhraseType::Npp,
            false,
        );
        add(
            r"<Herr|Frau> <und> <Herr|Frau> <pos=EIG>*",
            PhraseType::Npp,
            false,
        );
        add(
            r"<chunk=B-NP & !pos=ZAL & !pos=PLU & !chunk=NPP & !einige & !(regex=&prozent;)> <chunk=I-NP & !pos=PLU & !und>*",
            PhraseType::Nps,
            false,
        );
        add(
            r"<chunk=B-NP & !pos=SIN & !chunk=NPS & !Ellen> <chunk=I-NP & !pos=SIN>*",
            PhraseType::Npp,
            false,
        );
        add(
            r"<chunk=NPS> <pos=PRO> <pos=ADJ> <pos=ADJ> <NP>",
            PhraseType::Nps,
            false,
        );
        add(
            r"<regex=eine[rs]?> <der> <am> <pos=ADJ> <pos=PA2> <NP>",
            PhraseType::Nps,
            false,
        );
        add(
            r"<regex=eine[rs]?> <der> <beiden> <pos=ADJ>* <pos=SUB>",
            PhraseType::Nps,
            false,
        );
        add(
            r"<regex=eine[rs]?> <seiner|ihrer> <pos=PA1> <pos=SUB>",
            PhraseType::Nps,
            false,
        );
        add(r"<regex=[\d,.]+> <&prozent;>", PhraseType::Nps, false);
        add(r"<regex=[\d,.]+> <&prozent;>", PhraseType::Npp, false);
        add(r"<dass> <sie> <wie> <NP>", PhraseType::Npp, false);
        add(r"<pos=PLU> <die> <Regel>", PhraseType::Npp, false);
        add(
            r"<chunk=B-NP & pos=SIN> <chunk=I-NP & pos=SIN>* <,> <die> <pos=ADV>+ <chunk=NPS>+",
            PhraseType::Nps,
            false,
        );
        add(
            r"<chunk=B-NP & pos=PLU> <chunk=I-NP & pos=PLU>* <,> <die> <pos=ADV>+ <chunk=NPS>+",
            PhraseType::Npp,
            false,
        );
        add(
            r"<der|die|das> <pos=ADJ> <der> <pos=PA1> <pos=SUB>",
            PhraseType::Nps,
            false,
        );
        add(
            r"<pos=SUB & pos=PLU> <der> <pos=PA1> <pos=SUB>",
            PhraseType::Npp,
            false,
        );
        add(
            r"<der|die|das> <pos=ADJ> <der> <pos=PRO>? <pos=SUB>",
            PhraseType::Nps,
            false,
        );
        add(
            r"<chunk=NPS & !einige> <chunk=NPP & (pos=GEN |pos=ZAL)>+",
            PhraseType::Nps,
            true,
        );
        add(
            r"<chunk=NPP> <chunk=NPS & pos=GEN>+",
            PhraseType::Npp,
            true,
        );
        add(
            r"<chunk=NPS>+ <und> <chunk=NP[SP] & (pos=GEN | pos=ADV)>+",
            PhraseType::Nps,
            true,
        );
        add(
            r"<chunk=NPS>+ <der> <pos=ADV> <pos=PA2> <chunk=I-NP>",
            PhraseType::Nps,
            true,
        );
        add(
            r"<chunk=NPS>+ <der> (<pos=ADJ>|<pos=ZAL>) <NP>",
            PhraseType::Nps,
            true,
        );
        add(r"<chunk=NPS>+ <der> <NP>", PhraseType::Nps, true);
        add(
            r"<chunk=NPS>+ <der> <pos=ADJ> <pos=ADV> <pos=PA2> <NP>",
            PhraseType::Nps,
            true,
        );
        add(
            r"<chunk=NPS>+ <pos=PRO:POS> <pos=ADJ> <NP>",
            PhraseType::Nps,
            true,
        );
        add(
            r"<der|das> <pos=ADJ> <der> <pos=ZAL> <NP>",
            PhraseType::Nps,
            true,
        );
        add(r"<eine> <menge> <NP>+", PhraseType::Npp, true);
        add(r"<er|sie|es> <und> <NP> <NP>", PhraseType::Npp, false);
        add(r"<laut> <regex=.*>{0,3} <Quellen>", PhraseType::Pp, true);
        add(
            r"<pos=PRP> <pos=ART:> <pos=ADV>* <pos=ADJ> <NP>",
            PhraseType::Pp,
            true,
        );
        add(r"<pos=PRP> <chunk=NPP>+ <,> <NP>", PhraseType::Pp, true);
        add(r"<pos=PRP> <chunk=NPP>+", PhraseType::Pp, true);
        add(r"<pos=PRP> <der> <chunk=NPP>+", PhraseType::Pp, false);
        add(r"<pos=PRP> <NP>", PhraseType::Pp, false);
        add(
            r"<pos=PRP> <NP> <pos=ADJ> <und|oder|bzw.> <NP>",
            PhraseType::Pp,
            false,
        );
        add(r"<pos=PRP> (<NP>)+", PhraseType::Pp, false);
        add(
            r"<pos=PRP> <chunk=B-NP> <pos=ADV> <NP>",
            PhraseType::Pp,
            false,
        );
        add(
            r"<pos=PRP> <pos=ADV> <pos=ZAL> <chunk=B-NP>",
            PhraseType::Pp,
            false,
        );
        add(r"<pos=PRP> <pos=PRO> <NP>", PhraseType::Pp, false);
        add(
            r"<pos=PRP> <pos=ADJ> <und|oder|sowie> <NP>",
            PhraseType::Pp,
            false,
        );
        add(r"<pos=PRP> <pos=ADV> <regex=\d+> <NP>", PhraseType::Pp, false);
        add(r"<pos=PRP> <pos=PA1> <NP>", PhraseType::Pp, false);
        add(
            r"<pos=PRP> <pos=ADJ> <pos=PA1> <NP>",
            PhraseType::Pp,
            false,
        );
        add(
            r"<pos=PRP> <NP> <NP> <und|oder> <NP>",
            PhraseType::Pp,
            false,
        );
        add(r"<pos=PRP> <pos=ADV> <pos=ADJ> <NP>", PhraseType::Pp, false);
        add(
            r"<pos=PRP> <pos=ADJ:PRD:GRU> <pos=ZAL> <NP>",
            PhraseType::Pp,
            false,
        );
        add(
            r"<die> <pos=ADJ> <Sekunden|Minuten|Stunden|Tage|Wochen|Monate|Jahre|Jahrzehnte|Jahrhunderte> (<NP>)?",
            PhraseType::Pp,
            false,
        );
        add(
            r"<die> <pos=ADJ> <pos=ZAL> <Sekunden|Minuten|Stunden|Tage|Wochen|Monate|Jahre|Jahrzehnte|Jahrhunderte> (<NP>)?",
            PhraseType::Pp,
            false,
        );
        add(
            r"<regex=(vor)?letzte[sn]?> <Woche|Monat|Jahr|Jahrzehnt|Jahrhundert>",
            PhraseType::Pp,
            false,
        );
        add(
            r"<für> <in> <pos=EIG> <pos=PA1> <pos=SUB> <und> <pos=SUB>",
            PhraseType::Pp,
            true,
        );
        add(
            r"<chunk=NPP> <zwischen> <pos=EIG> <und|sowie> <NP>",
            PhraseType::Npp,
            false,
        );
        add(
            r"<,> <die|welche> <NP> <chunk=NPS & pos=GEN>+",
            PhraseType::Npp,
            false,
        );
        add(r"<NP> <,> <NP> <,> <NP>", PhraseType::Npp, false);
        add(
            r"<NP> <,> <NP> <,> <wie> <auch> <chunk=NPS>+",
            PhraseType::Npp,
            false,
        );
        v
    })
}

pub struct GermanChunker;

impl GermanChunker {
    pub fn new() -> Self {
        Self
    }

    fn compute_chunks(&self, sentence: &AnalyzedSentence) -> Vec<Vec<String>> {
        let forms = sentence_forms(sentence);
        let mut tokens: Vec<Token> = sentence
            .tokens()
            .iter()
            .filter(|t| !t.is_whitespace())
            .map(|t| Token::new(t, t.chunks().to_vec()))
            .collect();
        for rule in rules() {
            apply(rule, &mut tokens, &forms);
        }
        for rule in rules2() {
            apply(rule, &mut tokens, &forms);
        }
        tokens.into_iter().map(|t| t.chunks).collect()
    }
}

impl Default for GermanChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl og_core::checker::Chunker for GermanChunker {
    fn add_chunk_tags(&self, sentence: &mut AnalyzedSentence) {
        static CACHE: std::sync::OnceLock<std::sync::Mutex<BoundedMap>> =
            std::sync::OnceLock::new();
        let cache_cell = CACHE.get_or_init(|| std::sync::Mutex::new(BoundedMap::new()));
        let mut key = String::new();
        for atr in sentence.tokens() {
            if atr.is_whitespace() {
                continue;
            }
            key.push_str(atr.token().token());
            key.push('\u{1}');
            let reading = atr.token();
            for tag in reading.pos_tags() {
                key.push_str(tag);
                key.push('\u{2}');
            }
            for chunk in atr.chunks() {
                key.push_str(chunk);
                key.push('\u{3}');
            }
            key.push('\u{4}');
        }
        if key.is_empty() {
            return;
        }
        let assigned: Vec<Vec<String>> = {
            let mut cache = cache_cell.lock().unwrap();
            if let Some(hit) = cache.get(&key) {
                hit.clone()
            } else {
                let computed = self.compute_chunks(sentence);
                cache.put(key, computed.clone());
                computed
            }
        };
        let sentence_tokens = sentence.tokens_mut();
        let mut idx = 0;
        for token in sentence_tokens.iter_mut() {
            if token.is_whitespace() {
                continue;
            }
            if let Some(chunks) = assigned.get(idx) {
                token.set_chunks(chunks.clone());
            }
            idx += 1;
        }
    }
}

pub fn shared_german_chunker() -> Arc<GermanChunker> {
    static SHARED: std::sync::OnceLock<Arc<GermanChunker>> = std::sync::OnceLock::new();
    SHARED.get_or_init(|| Arc::new(GermanChunker)).clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openregex::Token;
    use og_core::AnalyzedToken;
    use og_core::AnalyzedTokenReadings;
    use og_core::checker::Chunker as _;
    use std::collections::HashSet;

    fn sentence(parts: &[(&str, &[&str])]) -> AnalyzedSentence {
        let mut tokens = vec![AnalyzedTokenReadings::new(
            AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
        )];
        let mut offset = 0;
        for (text, tags) in parts {
            let readings: Vec<AnalyzedToken> = tags
                .iter()
                .map(|t| {
                    AnalyzedToken::new(*text, offset, offset + text.len())
                        .with_pos_tags(vec![t.to_string()])
                })
                .collect();
            let primary = AnalyzedToken::new(*text, offset, offset + text.len())
                .with_pos_tags(tags.iter().map(|t| t.to_string()).collect());
            tokens.push(AnalyzedTokenReadings::new(primary).with_readings(readings));
            offset += text.len() + 1;
        }
        let mut s = AnalyzedSentence::new("", 0, offset);
        s.set_tokens(tokens);
        s
    }

    fn sentence_with_chunks(parts: &[(&str, &[&str], &[&str])]) -> AnalyzedSentence {
        let mut tokens = vec![AnalyzedTokenReadings::new(
            AnalyzedToken::new("<S>", 0, 0).with_pos_tags(vec!["SENT_START".to_string()]),
        )];
        let mut offset = 0;
        for (text, tags, chunks) in parts {
            let readings: Vec<AnalyzedToken> = tags
                .iter()
                .map(|t| {
                    AnalyzedToken::new(*text, offset, offset + text.len())
                        .with_pos_tags(vec![t.to_string()])
                })
                .collect();
            let primary = AnalyzedToken::new(*text, offset, offset + text.len())
                .with_pos_tags(tags.iter().map(|t| t.to_string()).collect());
            let mut atr = AnalyzedTokenReadings::new(primary).with_readings(readings);
            atr.set_chunks(chunks.iter().map(|c| c.to_string()).collect());
            tokens.push(atr);
            offset += text.len() + 1;
        }
        let mut s = AnalyzedSentence::new("", 0, offset);
        s.set_tokens(tokens);
        s
    }

    fn chunk_strings(s: &AnalyzedSentence) -> Vec<String> {
        s.tokens()
            .iter()
            .filter(|t| !t.is_whitespace())
            .map(|t| t.chunks().join("|"))
            .collect()
    }

    fn apply_rule_to(
        rule: &crate::openregex::Rule,
        parts: &[(&str, &[&str], &[&str])],
    ) -> Vec<String> {
        let s = sentence_with_chunks(parts);
        let forms: HashSet<String> = s
            .tokens()
            .iter()
            .map(|t| t.token().token().to_lowercase())
            .collect();
        let mut tokens: Vec<Token> = s
            .tokens()
            .iter()
            .filter(|t| !t.is_whitespace())
            .map(|t| Token::new(t, t.chunks().to_vec()))
            .collect();
        apply(rule, &mut tokens, &forms);
        tokens.iter().map(|t| t.chunks.join("|")).collect()
    }

    #[test]
    fn chunks_simple_noun_phrase() {
        let mut s = sentence(&[
            ("Das", &["ART:NOM:SIN:NEU"]),
            ("schöne", &["ADJ:NOM:SIN:NEU:GRU:SOL"]),
            ("Auto", &["SUB:NOM:SIN:NEU"]),
        ]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks = chunk_strings(&s);
        assert!(chunks[1].contains("B-NP"), "{chunks:?}");
        assert!(chunks[2].contains("I-NP"), "{chunks:?}");
        assert!(chunks[3].contains("I-NP"), "{chunks:?}");
        assert!(
            chunks[3].contains("NPS") || chunks[3].contains("NPP"),
            "{chunks:?}"
        );
    }

    #[test]
    fn chunks_two_nouns_with_und_as_plural() {
        let mut s = sentence(&[
            ("Ein", &["ART:NOM:SIN:NEU:IND"]),
            ("Hund", &["SUB:NOM:SIN:MAS"]),
            ("und", &["KON"]),
            ("eine", &["ART:NOM:SIN:FEM:IND"]),
            ("Katze", &["SUB:NOM:SIN:FEM"]),
        ]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks = chunk_strings(&s);
        assert!(chunks.iter().any(|c| c.contains("NPP")), "{chunks:?}");
    }

    #[test]
    fn untagged_punctuation_stays_o() {
        let mut s = sentence(&[(".", &["."])]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks = chunk_strings(&s);
        assert!(chunks[0].is_empty() || chunks[0] == "O", "{chunks:?}");
    }

    #[test]
    fn mythen_und_sagen_form_one_np() {
        let mut s = sentence(&[
            ("Mythen", &["SUB:NOM:PLU"]),
            ("und", &["KON"]),
            ("Sagen", &["SUB:NOM:PLU"]),
        ]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks = chunk_strings(&s);
        assert!(chunks[1].contains("B-NP"), "{chunks:?}");
        assert!(chunks[2].contains("I-NP"), "{chunks:?}");
        assert!(chunks[3].contains("I-NP"), "{chunks:?}");
    }

    #[test]
    fn julia_und_karsten_is_npp() {
        let mut s = sentence(&[
            ("Julia", &["EIG:NOM:SIN:FEM"]),
            ("und", &["KON"]),
            ("Karsten", &["EIG:NOM:SIN:MAS"]),
        ]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks = chunk_strings(&s);
        assert!(chunks.iter().any(|c| c.contains("NPP")), "{chunks:?}");
    }

    #[test]
    fn groesste_und_erfolgreichste_erfindung_is_nps() {
        let mut s = sentence(&[
            ("die", &["ART:NOM:SIN:FEM"]),
            ("größte", &["ADJ:NOM:SIN:FEM:GRU:SUP"]),
            ("und", &["KON"]),
            ("erfolgreichste", &["ADJ:NOM:SIN:FEM:GRU:SUP"]),
            ("Erfindung", &["SUB:NOM:SIN:FEM"]),
        ]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks = chunk_strings(&s);
        assert!(chunks[2].contains("NPS"), "{chunks:?}");
        assert!(chunks[3].contains("NPS"), "{chunks:?}");
        assert!(chunks[4].contains("NPS"), "{chunks:?}");
    }

    #[test]
    fn laut_quellen_is_pp_with_minmax() {
        let mut s = sentence(&[
            ("laut", &["PRP:DAT"]),
            ("den", &["ART:DAT:PLU"]),
            ("meisten", &["ADJ:DAT:PLU:GRU:SUP"]),
            ("Quellen", &["SUB:DAT:PLU"]),
        ]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks = chunk_strings(&s);
        assert!(chunks[1].contains("PP"), "{chunks:?}");
        assert!(chunks[4].contains("PP"), "{chunks:?}");
    }

    #[test]
    fn dot_atom_matches_literal_dot_only() {
        let rule = build(r"<bzw> <.>", PhraseType::Np, false);
        let chunks = apply_rule_to(
            &rule,
            &[
                ("bzw", &["KON"], &[]),
                (".", &["$."], &[]),
                ("und", &["KON"], &[]),
            ],
        );
        assert!(chunks[1].contains("B-NP"), "{chunks:?}");
        assert!(chunks[2].contains("I-NP"), "{chunks:?}");
        assert!(!chunks[3].contains("NP"), "{chunks:?}");

        let rule = build(r"<bzw> <.>", PhraseType::Np, false);
        let chunks = apply_rule_to(&rule, &[("bzw", &["KON"], &[]), ("x", &["SUB"], &[])]);
        assert!(!chunks[1].contains("NP"), "{chunks:?}");
    }

    #[test]
    fn chunk_atom_is_a_regex() {
        let rule = build(r"<chunk=NP[SP]>", PhraseType::Nps, false);
        let chunks = apply_rule_to(&rule, &[("Friedens", &["SUB:GEN:SIN:MAS"], &["NPS"])]);
        assert!(chunks[1].contains("NPS"), "{chunks:?}");

        let chunks = apply_rule_to(&rule, &[("Friedens", &["SUB:GEN:SIN:MAS"], &["B-NP"])]);
        assert!(!chunks[1].contains("NPS"), "{chunks:?}");
    }

    #[test]
    fn leftmost_longest_match_backtracks_repetition() {
        let rule = build(r"<regex=a+> <regex=aa>", PhraseType::Np, false);
        let chunks = apply_rule_to(&rule, &[("a", &["X"], &[]), ("aa", &["X"], &[])]);
        assert!(chunks[1].contains("B-NP"), "{chunks:?}");
        assert!(chunks[2].contains("I-NP"), "{chunks:?}");
    }

    #[test]
    fn string_atoms_match_case_insensitively() {
        let rule = build(r"<und|oder|sowie>", PhraseType::Pp, false);
        let chunks = apply_rule_to(&rule, &[("Und", &["KON"], &[])]);
        assert!(chunks[1].contains("PP"), "{chunks:?}");
    }

    #[test]
    fn regex_atom_is_case_insensitive_full_match() {
        let rule = build(r"<regex=jede[rs]?>", PhraseType::Pp, false);
        let chunks = apply_rule_to(&rule, &[("Jeder", &["ART"], &[])]);
        assert!(chunks[1].contains("PP"), "{chunks:?}");

        let chunks = apply_rule_to(&rule, &[("jedem", &["ART"], &[])]);
        assert!(!chunks[1].contains("PP"), "{chunks:?}");
    }

    #[test]
    fn posre_atom_is_case_sensitive_full_match() {
        let rule = build(r"<posre=^ART.*>", PhraseType::Pp, false);
        let chunks = apply_rule_to(&rule, &[("der", &["ART:NOM:SIN:NEU"], &[])]);
        assert!(chunks[1].contains("PP"), "{chunks:?}");

        let chunks = apply_rule_to(&rule, &[("x", &["PART:NOM"], &[])]);
        assert!(!chunks[1].contains("PP"), "{chunks:?}");
    }

    #[test]
    fn negated_leaf_conjunction() {
        let rule = build(r"<chunk=B-NP & !pos=PLU>", PhraseType::Nps, false);
        let chunks = apply_rule_to(&rule, &[("Hunde", &["SUB:NOM:PLU"], &["B-NP"])]);
        assert!(!chunks[1].contains("NPS"), "{chunks:?}");

        let chunks = apply_rule_to(&rule, &[("Hund", &["SUB:NOM:SIN:MAS"], &["B-NP"])]);
        assert!(chunks[1].contains("NPS"), "{chunks:?}");
    }

    #[test]
    fn parenthesized_leaf_disjunction() {
        let rule = build(r"<chunk=NPP & (pos=GEN |pos=ZAL)>+", PhraseType::Nps, false);
        let chunks = apply_rule_to(&rule, &[("Städte", &["SUB:GEN:PLU"], &["NPP"])]);
        assert!(chunks[1].contains("NPS"), "{chunks:?}");

        let chunks = apply_rule_to(&rule, &[("Städte", &["SUB:DAT:PLU"], &["NPP"])]);
        assert!(!chunks[1].contains("NPS"), "{chunks:?}");
    }

    #[test]
    fn form_hint_gate_skips_rules_without_forms() {
        let rule = build(r"<sowohl> <NP> <als> <auch> <NP>", PhraseType::Npp, false);
        let chunks = apply_rule_to(
            &rule,
            &[
                ("a", &["X"], &["B-NP"]),
                ("b", &["X"], &["I-NP"]),
                ("c", &["X"], &["B-NP"]),
                ("d", &["X"], &["I-NP"]),
            ],
        );
        assert!(!chunks.iter().any(|c| c.contains("NPP")), "{chunks:?}");
    }

    #[test]
    fn all_rules_build() {
        assert_eq!(rules().len(), 9);
        assert_eq!(rules2().len(), 73);
    }
}
