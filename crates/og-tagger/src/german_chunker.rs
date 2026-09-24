use og_core::AnalyzedSentence;
use og_core::AnalyzedTokenReadings;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PhraseType {
    Np,
    Nps,
    Npp,
    Pp,
}

impl PhraseType {
    fn tag(self, at_start: bool) -> &'static str {
        match self {
            PhraseType::Np => {
                if at_start {
                    "B-NP"
                } else {
                    "I-NP"
                }
            }
            PhraseType::Nps => "NPS",
            PhraseType::Npp => "NPP",
            PhraseType::Pp => "PP",
        }
    }
}

#[derive(Clone, Debug)]
enum Atom {
    All(Vec<Constraint>),
    Any,
    Forms(Vec<String>),
    Pos(String),
    PosRe(regex::Regex),
    Regex(String),
    RegexCs(String),
    Chunk(String),
}

#[derive(Clone, Debug)]
enum Piece {
    Single {
        atom: Atom,
        min: u32,
        max: Option<u32>,
    },
    Group {
        alts: Vec<Vec<Piece>>,
        min: u32,
        max: Option<u32>,
    },
}

struct Rule {
    pieces: Vec<Piece>,
    phrase: PhraseType,
    overwrite: bool,
    form_hints: Vec<Vec<String>>,
}

const FILTER_TAGS: [&str; 3] = ["PP", "NPP", "NPS"];

fn parse_atom(body: &str) -> Atom {
    let body = body.trim();
    if body == "." {
        return Atom::Any;
    }
    if let Some(value) = body.strip_prefix("posre=") {
        if let Ok(re) = regex::Regex::new(&format!("^(?:{value})")) {
            return Atom::PosRe(re);
        }
        return Atom::Pos(value.to_string());
    }
    if let Some(value) = body.strip_prefix("pos=") {
        return Atom::Pos(value.to_string());
    }
    if let Some(value) = body.strip_prefix("regexCS=") {
        return Atom::RegexCs(value.to_string());
    }
    if let Some(value) = body.strip_prefix("regex=") {
        return Atom::Regex(value.to_string());
    }
    if let Some(value) = body.strip_prefix("chunk=") {
        return Atom::Chunk(value.to_string());
    }
    Atom::Forms(body.split('|').map(str::to_string).collect())
}

fn split_top(body: &str, sep: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_bracket = false;
    let mut current = String::new();
    for c in body.chars() {
        match c {
            '<' => in_bracket = true,
            '>' => in_bracket = false,
            '(' if !in_bracket => depth += 1,
            ')' if !in_bracket => depth -= 1,
            c if c == sep && depth == 0 && !in_bracket => {
                parts.push(current.clone());
                current.clear();
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

fn tokenize(expr: &str) -> Vec<String> {
    let re = regex::Regex::new(r"<[^>]*>|\(|\)|\||[?*+]|[A-Za-zÄÖÜäöüß0-9,._\-]+").unwrap();
    re.find_iter(expr).map(|m| m.as_str().to_string()).collect()
}

fn parse_sequence(expr: &str) -> Vec<Piece> {
    let tokens = tokenize(expr);
    let mut pieces = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i].as_str();
        if tok == " " {
            i += 1;
            continue;
        }
        let (body, min, max) = {
            let mut body = tok.to_string();
            let mut min = 1u32;
            let mut max = Some(1u32);
            if i + 1 < tokens.len() && matches!(tokens[i + 1].as_str(), "?" | "*" | "+") {
                match tokens[i + 1].as_str() {
                    "?" => {
                        min = 0;
                        max = Some(1);
                    }
                    "*" => {
                        min = 0;
                        max = None;
                    }
                    _ => {
                        min = 1;
                        max = None;
                    }
                }
                i += 1;
            }
            if body.starts_with('<') && body.ends_with('>') && body.len() > 2 {
                body = body[1..body.len() - 1].to_string();
            }
            (body, min, max)
        };
        if tok == "(" {
            let mut depth = 1i32;
            let mut inner = String::new();
            i += 1;
            while i < tokens.len() {
                let t = tokens[i].as_str();
                if t == "(" {
                    depth += 1;
                } else if t == ")" {
                    depth -= 1;
                    if depth == 0 {
                        i += 1;
                        break;
                    }
                }
                inner.push(' ');
                inner.push_str(t);
                i += 1;
            }
            let (gmin, gmax) = if i < tokens.len() && matches!(tokens[i].as_str(), "?" | "*" | "+")
            {
                let q = tokens[i].as_str();
                i += 1;
                match q {
                    "?" => (0u32, Some(1u32)),
                    "*" => (0, None),
                    _ => (1, None),
                }
            } else {
                (1u32, Some(1u32))
            };
            let alts: Vec<Vec<Piece>> = split_top(&inner, '|')
                .iter()
                .map(|alt| parse_sequence(alt))
                .collect();
            pieces.push(Piece::Group {
                alts,
                min: gmin,
                max: gmax,
            });
            continue;
        }
        let atom = parse_conjunctive(&body);
        pieces.push(Piece::Single { atom, min, max });
        i += 1;
    }
    pieces
}

#[derive(Clone, Debug)]
enum Constraint {
    Positive(Atom),
    Negated(Atom),
}

fn parse_constraint(body: &str) -> Constraint {
    let trimmed = body.trim();
    if let Some(rest) = trimmed.strip_prefix('!') {
        Constraint::Negated(parse_atom(rest))
    } else {
        Constraint::Positive(parse_atom(trimmed))
    }
}

fn parse_conjunctive(body: &str) -> Atom {
    let parts = split_top(body, '&');
    if parts.len() == 1 {
        return parse_atom(&parts[0]);
    }
    let mut atoms = Vec::new();
    for part in parts {
        atoms.push(parse_constraint(&part));
    }
    Atom::All(atoms)
}

fn atom_matches(
    atom: &Atom,
    token: &AnalyzedTokenReadings,
    chunks: &[String],
    forms: &[String],
) -> bool {
    match atom {
        Atom::All(_) => true,
        Atom::Any => true,
        Atom::Forms(values) => values.iter().any(|v| forms.contains(v)),
        Atom::Pos(value) => token
            .readings()
            .iter()
            .any(|r| r.pos_tags().iter().any(|t| t.contains(value.as_str()))),
        Atom::PosRe(re) => token
            .readings()
            .iter()
            .any(|r| r.pos_tags().iter().any(|t| re.is_match(t))),
        Atom::Regex(value) => regex::Regex::new(&format!("^(?:{value})$"))
            .map(|re| forms.iter().any(|f| re.is_match(f)))
            .unwrap_or(false),
        Atom::RegexCs(value) => regex::Regex::new(&format!("^(?:{value})$"))
            .map(|re| forms.iter().any(|f| re.is_match(f)))
            .unwrap_or(false),
        Atom::Chunk(value) => chunks.iter().any(|c| c == value),
    }
}

struct Token<'a> {
    reading: &'a AnalyzedTokenReadings,
    chunks: Vec<String>,
    forms: Vec<String>,
}

impl<'a> Token<'a> {
    fn matches(&self, constraint: &Constraint) -> bool {
        match constraint {
            Constraint::Positive(atom) => {
                atom_matches(atom, self.reading, &self.chunks, &self.forms)
            }
            Constraint::Negated(atom) => {
                !atom_matches(atom, self.reading, &self.chunks, &self.forms)
            }
        }
    }

    fn new(reading: &'a AnalyzedTokenReadings) -> Self {
        let text = reading.token().token();
        let mut forms = vec![text.to_string()];
        if text.chars().any(|c| c.is_uppercase()) {
            forms.push(text.to_lowercase());
        }
        Self {
            reading,
            chunks: reading.chunks().to_vec(),
            forms,
        }
    }

    fn matches_atom(&self, atom: &Atom) -> bool {
        match atom {
            Atom::All(constraints) => constraints.iter().all(|c| self.matches(c)),
            other => atom_matches(other, self.reading, &self.chunks, &self.forms),
        }
    }
}

fn match_piece(
    piece: &Piece,
    tokens: &[Token],
    at: usize,
    used: &mut Vec<(usize, usize)>,
) -> Option<usize> {
    let try_once = |piece: &Piece,
                    tokens: &[Token],
                    at: usize,
                    used: &mut Vec<(usize, usize)>|
     -> Option<usize> {
        match piece {
            Piece::Single { atom, .. } => {
                if at >= tokens.len() {
                    return None;
                }
                if tokens[at].matches_atom(atom) {
                    Some(at + 1)
                } else {
                    None
                }
            }
            Piece::Group { alts, .. } => {
                for alt in alts {
                    if let Some(next) = match_sequence(alt, tokens, at, used) {
                        return Some(next);
                    }
                }
                None
            }
        }
    };

    let (min, max) = match piece {
        Piece::Single { min, max, .. } => (*min, *max),
        Piece::Group { min, max, .. } => (*min, *max),
    };
    let mut position = at;
    let mut count = 0u32;
    while max.is_none_or(|m| count < m) {
        let snapshot = used.len();
        match try_once(piece, tokens, position, used) {
            Some(next) if next > position => {
                used.push((position, next));
                position = next;
                count += 1;
            }
            _ => {
                used.truncate(snapshot);
                break;
            }
        }
    }
    if count >= min {
        Some(position)
    } else {
        let target = used.len() - count as usize;
        used.truncate(target);
        None
    }
}

fn match_sequence(
    pieces: &[Piece],
    tokens: &[Token],
    at: usize,
    used: &mut Vec<(usize, usize)>,
) -> Option<usize> {
    let mut position = at;
    let mut consumed = Vec::new();
    for piece in pieces {
        let mut local = Vec::new();
        match match_piece(piece, tokens, position, &mut local) {
            Some(next) => {
                position = next;
                consumed.extend(local);
            }
            None => return None,
        }
    }
    used.extend(consumed);
    Some(position)
}

fn build(expr: &str, phrase: PhraseType, overwrite: bool) -> Rule {
    let expanded = expr.replace("<NP>", "<chunk=B-NP> <chunk=I-NP>*");
    let mut form_hints = Vec::new();
    let mut chars = expanded.char_indices().peekable();
    while let Some((idx, c)) = chars.next() {
        if c != '<' {
            continue;
        }
        let rest = &expanded[idx + 1..];
        let Some(end) = rest.find('>') else { continue };
        let body = &rest[..end];
        if body.contains('=') || body == "." {
            continue;
        }
        if body.contains('|') {
            form_hints.push(body.split('|').map(str::to_string).collect());
        }
        for _ in 0..=end {
            chars.next();
        }
    }
    Rule {
        pieces: parse_sequence(&expanded),
        phrase,
        overwrite,
        form_hints,
    }
}

fn rules() -> Vec<Rule> {
    let mut v = Vec::new();
    let mut np = |e: &str| {
        v.push(build(e, PhraseType::Np, false));
    };
    np("(<posre=^ART.*>|<pos=PRO>)? <pos=ADV>* <pos=PA2>* <pos=ADJ>* <pos=SUB>+");
    np("<pos=SUB> (<und|oder>|(<bzw> <.>)) <pos=SUB>");
    np("<pos=ADJ> (<und|oder>|(<bzw> <.>)) <pos=ADJ> <pos=SUB>");
    np("<pos=ADJ> (<und|oder>|(<bzw> <.>)) <pos=PA2> <pos=SUB>");
    np("<posre=^ART.*> <pos=ADV>* <pos=ADJ>* <regexCS=[A-ZÖÄÜ][a-zöäü]+>");
    np("<pos=PRO>? <pos=ZAL> <pos=SUB>");
    np("<Herr|Herrn|Frau> <pos=EIG>+");
    np("<Herr|Herrn|Frau> <regexCS=[A-ZÖÄÜ][a-zöäü-]+>+");
    np("<der>");
    v
}

fn rules2() -> Vec<Rule> {
    let mut v = Vec::new();
    fn npp(v: &mut Vec<Rule>, e: &str, o: bool) {
        v.push(build(e, PhraseType::Npp, o));
    }
    fn nps(v: &mut Vec<Rule>, e: &str, o: bool) {
        v.push(build(e, PhraseType::Nps, o));
    }
    fn pp(v: &mut Vec<Rule>, e: &str, o: bool) {
        v.push(build(e, PhraseType::Pp, o));
    }
    npp(
        &mut v,
        "<pos=ADJ> <,> <chunk=B-NP> <chunk=I-NP>* <und|sowie> <NP>",
        false,
    );
    npp(
        &mut v,
        "<chunk=B-NP & !regex=jede[rs]?> <chunk=I-NP>* <und|sowie> <pos=ADV>? <NP>",
        false,
    );
    nps(
        &mut v,
        "<pos=ADJ> <und|sowie> <chunk=B-NP & !pos=PLU> <chunk=I-NP>*",
        true,
    );
    nps(
        &mut v,
        "<deren> <chunk=B-NP & !pos=PLU> <und|sowie> <chunk=B-NP>*",
        true,
    );
    npp(&mut v, "<pos=EIG> <und> <pos=EIG>", false);
    nps(
        &mut v,
        "<pos=ART> <pos=ADJ> <und|sowie> (<pos=ADJ>|<pos=PA2>) <chunk=I-NP & !pos=PLU>+",
        true,
    );
    nps(
        &mut v,
        "<chunk=B-NP & !pos=PLU> <chunk=I-NP>* <und|sowie> <keine> <chunk=I-NP>+",
        true,
    );
    npp(
        &mut v,
        "<NP> <und|sowie> <pos=ART> <pos=PA1> <pos=SUB>",
        true,
    );
    nps(&mut v, "<eins|eines> <chunk=B-NP> <chunk=I-NP>+", false);
    npp(
        &mut v,
        "<ich|du|er|sie|es|wir|ihr|sie> <und|oder|sowie> <NP>",
        false,
    );
    npp(&mut v, "<sowohl> <NP> <als> <auch> <NP>", false);
    npp(&mut v, "<sowohl> <pos=EIG> <als> <auch> <pos=EIG>", false);
    npp(
        &mut v,
        "<sowohl> <ich|du|er|sie|es|wir|ihr|sie> <als> <auch> <NP>",
        false,
    );
    npp(
        &mut v,
        "<pos=SUB> <und|oder|sowie> <chunk=B-NP & !ihre> <chunk=I-NP>*",
        false,
    );
    npp(&mut v, "<weder> <pos=SUB> <noch> <pos=SUB>", false);
    npp(
        &mut v,
        "<zwei|drei|vier|fünf|sechs|sieben|acht|neun|zehn|elf|zwölf> <chunk=I-NP>",
        false,
    );
    nps(
        &mut v,
        "<chunk=B-NP> <pos=PRP> <NP> <chunk=B-NP & pos=SIN> <chunk=I-NP>*",
        false,
    );
    npp(
        &mut v,
        "<chunk=B-NP> <pos=PRP> <NP> <chunk=B-NP & pos=PLU> <chunk=I-NP>*",
        false,
    );
    nps(
        &mut v,
        "<chunk=B-NP> <pos=PRP> <NP> <pos=PA2> <chunk=B-NP & !pos=PLU> <chunk=I-NP>*",
        false,
    );
    npp(
        &mut v,
        "<chunk=B-NP> <pos=PRP> <NP> <pos=PA2> <chunk=B-NP & !pos=SIN> <chunk=I-NP>*",
        false,
    );
    npp(&mut v, "<Herr|Frau> <und> <Herr|Frau> <pos=EIG>*", false);
    nps(
        &mut v,
        "<chunk=B-NP & !pos=ZAL & !pos=PLU & !chunk=NPP & !einige & !(regex=(Prozent|Kilo|Kilogramm|Gramm|Euro|Pfund))> <chunk=I-NP & !pos=PLU & !und>*",
        true,
    );
    npp(
        &mut v,
        "<chunk=B-NP & !pos=SIN & !chunk=NPS & !Ellen> <chunk=I-NP & !pos=SIN>*",
        true,
    );
    nps(
        &mut v,
        "<chunk=NPS> <pos=PRO> <pos=ADJ> <pos=ADJ> <NP>",
        false,
    );
    nps(
        &mut v,
        "<regex=eine[rs]?> <der> <am> <pos=ADJ> <pos=PA2> <NP>",
        false,
    );
    nps(
        &mut v,
        "<regex=eine[rs]?> <der> <beiden> <pos=ADJ>* <pos=SUB>",
        false,
    );
    nps(
        &mut v,
        "<regex=eine[rs]?> <seiner|ihrer> <pos=PA1> <pos=SUB>",
        false,
    );
    nps(
        &mut v,
        "<regex=[\\d,.]+> <Prozent|Kilo|Kilogramm|Gramm|Euro|Pfund>",
        false,
    );
    npp(
        &mut v,
        "<regex=[\\d,.]+> <Prozent|Kilo|Kilogramm|Gramm|Euro|Pfund>",
        false,
    );
    npp(&mut v, "<dass> <sie> <wie> <NP>", false);
    npp(&mut v, "<pos=PLU> <die> <Regel>", false);
    nps(
        &mut v,
        "<chunk=B-NP & pos=SIN> <chunk=I-NP & pos=SIN>* <,> <die> <pos=ADV>+ <chunk=NPS>+",
        false,
    );
    npp(
        &mut v,
        "<chunk=B-NP & pos=PLU> <chunk=I-NP & pos=PLU>* <,> <die> <pos=ADV>+ <chunk=NPS>+",
        false,
    );
    nps(
        &mut v,
        "<der|die|das> <pos=ADJ> <der> <pos=PA1> <pos=SUB>",
        false,
    );
    npp(
        &mut v,
        "<pos=SUB & pos=PLU> <der> <pos=PA1> <pos=SUB>",
        false,
    );
    nps(
        &mut v,
        "<der|die|das> <pos=ADJ> <der> <pos=PRO>? <pos=SUB>",
        false,
    );
    nps(
        &mut v,
        "<chunk=NPS & !einige> <chunk=NPP & (pos=GEN |pos=ZAL)>+",
        true,
    );
    npp(&mut v, "<chunk=NPP> <chunk=NPS & pos=GEN>+", true);
    nps(
        &mut v,
        "<chunk=NPS>+ <und> <chunk=NP[SP] & (pos=GEN | pos=ADV)>+",
        true,
    );
    nps(
        &mut v,
        "<chunk=NPS>+ <der> <pos=ADV> <pos=PA2> <chunk=I-NP>",
        true,
    );
    nps(
        &mut v,
        "<chunk=NPS>+ <der> (<pos=ADJ>|<pos=ZAL>) <NP>",
        true,
    );
    nps(&mut v, "<chunk=NPS>+ <der> <NP>", true);
    pp(&mut v, "<laut> <regex=.*>{0,3} <Quellen>", true);
    pp(
        &mut v,
        "<pos=PRP> <pos=ART:> <pos=ADV>* <pos=ADJ> <NP>",
        true,
    );
    v
}

fn has_all_form_hints(rule: &Rule, forms: &[String]) -> bool {
    rule.form_hints
        .iter()
        .all(|hints| hints.iter().any(|h| forms.contains(h)))
}

fn apply(rule: &Rule, tokens: &mut [Token]) {
    let all_forms: Vec<String> = tokens
        .iter()
        .flat_map(|t| t.forms.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    if !has_all_form_hints(rule, &all_forms) {
        return;
    }
    let mut start = 0;
    while start < tokens.len() {
        let mut used = Vec::new();
        if let Some(end) = match_sequence(&rule.pieces, tokens, start, &mut used)
            && end > start
        {
            let span: Vec<(usize, usize)> = (start..end).map(|i| (i, i + 1)).collect();
            for (i, _) in span {
                let mut new_chunks = tokens[i].chunks.clone();
                if rule.overwrite {
                    new_chunks.retain(|c| !FILTER_TAGS.contains(&c.as_str()));
                }
                let new_tag = rule.phrase.tag(i == start);
                if !new_chunks.iter().any(|c| c == new_tag) {
                    new_chunks.retain(|c| c != "O");
                    new_chunks.push(new_tag.to_string());
                }
                tokens[i].chunks = new_chunks;
            }
            start = end;
            continue;
        }
        start += 1;
    }
}

pub struct GermanChunker;

impl GermanChunker {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GermanChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl og_core::checker::Chunker for GermanChunker {
    fn add_chunk_tags(&self, sentence: &mut AnalyzedSentence) {
        let rule_set = rules();
        let rule_set2 = rules2();
        let mut tokens: Vec<Token> = sentence
            .tokens()
            .iter()
            .filter(|t| !t.is_whitespace())
            .map(Token::new)
            .collect();
        for rule in &rule_set {
            apply(rule, &mut tokens);
        }
        for rule in &rule_set2 {
            apply(rule, &mut tokens);
        }
        let assigned: Vec<Vec<String>> = tokens.iter().map(|t| t.chunks.clone()).collect();
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
    use og_core::AnalyzedToken;
    use og_core::checker::Chunker as _;

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

    #[test]
    fn chunks_simple_noun_phrase() {
        let mut s = sentence(&[
            ("Das", &["ART:NOM:SIN:NEU"]),
            ("schöne", &["ADJ:NOM:SIN:NEU:GRU:SOL"]),
            ("Auto", &["SUB:NOM:SIN:NEU"]),
        ]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks: Vec<String> = s
            .tokens()
            .iter()
            .filter(|t| !t.is_whitespace())
            .map(|t| t.chunks().join("|"))
            .collect();
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
        let joined: String = s
            .tokens()
            .iter()
            .filter(|t| !t.is_whitespace())
            .flat_map(|t| t.chunks().to_vec())
            .collect::<Vec<_>>()
            .join(",");
        assert!(joined.contains("NPP"), "{joined:?}");
    }

    #[test]
    fn untagged_punctuation_stays_o() {
        let mut s = sentence(&[(".", &["."])]);
        GermanChunker::new().add_chunk_tags(&mut s);
        let chunks: Vec<String> = s
            .tokens()
            .iter()
            .filter(|t| !t.is_whitespace())
            .map(|t| t.chunks().join("|"))
            .collect();
        assert!(chunks[0].is_empty() || chunks[0] == "O", "{chunks:?}");
    }
}
