use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::path::Path;

pub struct GisModel {
    outcomes: Vec<Box<str>>,
    preds: HashMap<Box<str>, Box<[(u32, f64)]>>,
    prior: f64,
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u16(&mut self) -> Option<u16> {
        let b = self.data.get(self.pos..self.pos + 2)?;
        self.pos += 2;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }

    fn i32(&mut self) -> Option<i32> {
        let b = self.data.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f64(&mut self) -> Option<f64> {
        let b = self.data.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(f64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn utf(&mut self) -> Option<String> {
        let len = self.u16()? as usize;
        let bytes = self.data.get(self.pos..self.pos + len)?;
        self.pos += len;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

impl GisModel {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let mut r = Reader { data, pos: 0 };
        r.utf()?;
        r.i32()?;
        r.f64()?;
        let num_outcomes = r.i32()?;
        let mut outcomes = Vec::with_capacity(num_outcomes.max(0) as usize);
        for _ in 0..num_outcomes {
            outcomes.push(r.utf()?.into_boxed_str());
        }
        let num_patterns = r.i32()?;
        let mut patterns: Vec<(usize, Vec<u32>)> = Vec::with_capacity(num_patterns.max(0) as usize);
        for _ in 0..num_patterns {
            let text = r.utf()?;
            let mut nums = text.split(' ').map(|t| t.parse::<u32>());
            let count = nums.next().and_then(|r| r.ok())? as usize;
            let indices: Vec<u32> = nums.collect::<Result<Vec<u32>, _>>().ok()?;
            patterns.push((count, indices));
        }
        let num_contexts = r.i32()?;
        let mut pred_names = Vec::with_capacity(num_contexts.max(0) as usize);
        for _ in 0..num_contexts {
            pred_names.push(r.utf()?);
        }
        let mut params: Vec<f64> = Vec::with_capacity((data.len() - r.pos) / 8);
        while let Some(v) = r.f64() {
            params.push(v);
        }

        let mut entries: Vec<Vec<(u32, f64)>> = vec![Vec::new(); pred_names.len()];
        let mut ctx = 0;
        let mut param_pos = 0;
        for (count, indices) in patterns {
            for _ in 0..count {
                let entry = entries.get_mut(ctx)?;
                entry.reserve(indices.len());
                for &oid in &indices {
                    entry.push((oid, *params.get(param_pos)?));
                    param_pos += 1;
                }
                ctx += 1;
            }
        }

        let mut preds: HashMap<Box<str>, Box<[(u32, f64)]>> =
            HashMap::with_capacity(pred_names.len());
        for (name, entry) in pred_names.into_iter().zip(entries) {
            preds.insert(name.into_boxed_str(), entry.into_boxed_slice());
        }
        let prior = (1.0 / outcomes.len() as f64).ln();
        Some(Self {
            outcomes,
            preds,
            prior,
        })
    }

    pub fn from_file(path: &Path) -> Option<Self> {
        Self::from_bytes(&std::fs::read(path).ok()?)
    }

    pub fn outcome_index(&self, outcome: &str) -> Option<usize> {
        self.outcomes.iter().position(|o| &**o == outcome)
    }

    pub fn num_outcomes(&self) -> usize {
        self.outcomes.len()
    }

    pub fn outcome(&self, index: usize) -> &str {
        &self.outcomes[index]
    }

    fn eval(&self, context: &[String], probs: &mut [f64]) {
        probs.fill(self.prior);
        for feature in context {
            if let Some(params) = self.preds.get(feature.as_str()) {
                for (oid, weight) in params.iter() {
                    probs[*oid as usize] += weight;
                }
            }
        }
        let mut normal = 0.0;
        for p in probs.iter_mut() {
            *p = p.exp();
            normal += *p;
        }
        for p in probs.iter_mut() {
            *p /= normal;
        }
    }

    fn best_outcome(&self, probs: &[f64]) -> usize {
        let mut best = 0;
        let mut best_score = f64::NEG_INFINITY;
        for (i, p) in probs.iter().enumerate() {
            if *p > best_score {
                best_score = *p;
                best = i;
            }
        }
        best
    }
}

struct BeamSequence {
    score: f64,
    outcomes: Vec<usize>,
}

impl Eq for BeamSequence {}

impl PartialEq for BeamSequence {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl PartialOrd for BeamSequence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BeamSequence {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .partial_cmp(&other.score)
            .unwrap_or(Ordering::Equal)
    }
}

const ZERO_LOG: f64 = -100000.0;

fn beam_search(
    model: &GisModel,
    sequence_len: usize,
    beam_size: usize,
    contexts_for: &mut dyn FnMut(usize, &[usize]) -> Vec<String>,
    valid: &dyn Fn(usize, &[usize], usize) -> bool,
) -> Vec<usize> {
    let mut prev: BinaryHeap<BeamSequence> = BinaryHeap::new();
    prev.push(BeamSequence {
        score: 0.0,
        outcomes: Vec::new(),
    });
    let num_outcomes = model.num_outcomes();
    let mut probs: Vec<f64> = vec![0.0; num_outcomes];

    for i in 0..sequence_len {
        let sz = beam_size.min(prev.len());
        let mut next: BinaryHeap<BeamSequence> = BinaryHeap::new();
        for _ in 0..sz {
            let top = prev.pop().unwrap();
            let contexts = contexts_for(i, &top.outcomes);
            model.eval(&contexts, &mut probs);
            let mut sorted = probs.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
            let min = sorted[probs.len().saturating_sub(beam_size).min(probs.len() - 1)];

            let advance = |next: &mut BinaryHeap<BeamSequence>, threshold: f64| {
                for (p, probability) in probs.iter().enumerate() {
                    if *probability >= threshold && valid(i, &top.outcomes, p) {
                        let score = top.score + probability.ln();
                        if score > ZERO_LOG {
                            let mut outcomes = top.outcomes.clone();
                            outcomes.push(p);
                            next.push(BeamSequence { score, outcomes });
                        }
                    }
                }
            };

            advance(&mut next, min);
            if next.is_empty() {
                advance(&mut next, f64::NEG_INFINITY);
            }
        }
        prev = next;
    }

    prev.pop().map(|s| s.outcomes).unwrap_or_default()
}

pub struct OpenNlpTokenizer {
    model: GisModel,
}

impl OpenNlpTokenizer {
    pub fn new(model: GisModel) -> Self {
        Self { model }
    }

    pub fn tokenize_spans(&self, text: &str) -> Vec<(usize, usize)> {
        let mut spans = whitespace_spans(text);
        let mut result = Vec::with_capacity(spans.len());
        let mut probs = vec![0.0; self.model.num_outcomes()];
        for (span_start, span_end) in spans.drain(..) {
            let tok = &text[span_start..span_end];
            if tok.chars().count() < 2 || is_alphanumeric(tok) {
                result.push((span_start, span_end));
                continue;
            }
            let mut part_start = span_start;
            for j in (span_start + 1)..span_end {
                if !text.is_char_boundary(j) {
                    continue;
                }
                let position = text[span_start..j].chars().count();
                let contexts = tokenizer_contexts(tok, position);
                self.model.eval(&contexts, &mut probs);
                if self.model.outcome(self.model.best_outcome(&probs)) == "T" {
                    result.push((part_start, j));
                    part_start = j;
                }
            }
            result.push((part_start, span_end));
        }
        result
    }
}

fn whitespace_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if is_java_whitespace(c) {
            if let Some(s) = start.take() {
                spans.push((s, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        spans.push((s, text.len()));
    }
    spans
}

fn is_java_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

fn is_alphanumeric(tok: &str) -> bool {
    !tok.is_empty() && tok.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn add_char_preds(key: &str, c: char, preds: &mut Vec<String>) {
    preds.push(format!("{key}={c}"));
    if c.is_alphabetic() {
        preds.push(format!("{key}_alpha"));
        if c.is_uppercase() {
            preds.push(format!("{key}_caps"));
        }
    } else if c.is_numeric() {
        preds.push(format!("{key}_num"));
    } else if is_java_whitespace(c) {
        preds.push(format!("{key}_ws"));
    } else if c == '.' || c == '?' || c == '!' {
        preds.push(format!("{key}_eos"));
    } else if c == '`' || c == '"' || c == '\'' {
        preds.push(format!("{key}_quote"));
    } else if c == '[' || c == '{' || c == '(' {
        preds.push(format!("{key}_lp"));
    } else if c == ']' || c == '}' || c == ')' {
        preds.push(format!("{key}_rp"));
    }
}

fn tokenizer_contexts(token: &str, index: usize) -> Vec<String> {
    let chars: Vec<char> = token.chars().collect();
    let mut preds = Vec::with_capacity(16);
    preds.push(format!("p={}", chars[..index].iter().collect::<String>()));
    preds.push(format!("s={}", chars[index..].iter().collect::<String>()));
    if index > 0 {
        add_char_preds("p1", chars[index - 1], &mut preds);
        if index > 1 {
            add_char_preds("p2", chars[index - 2], &mut preds);
            preds.push(format!("p21={}{}", chars[index - 2], chars[index - 1]));
        } else {
            preds.push("p2=bok".to_string());
        }
        preds.push(format!("p1f1={}{}", chars[index - 1], chars[index]));
    } else {
        preds.push("p1=bok".to_string());
    }
    add_char_preds("f1", chars[index], &mut preds);
    if index + 1 < chars.len() {
        add_char_preds("f2", chars[index + 1], &mut preds);
        preds.push(format!("f12={}{}", chars[index], chars[index + 1]));
    } else {
        preds.push("f2=bok".to_string());
    }
    if chars[0] == '&' && chars[chars.len() - 1] == ';' {
        preds.push("cc".to_string());
    }
    preds
}

pub struct OpenNlpPosTagger {
    model: GisModel,
    tag_dictionary: HashMap<String, Vec<usize>>,
}

impl OpenNlpPosTagger {
    pub fn new(model: GisModel, tag_dictionary: HashMap<String, Vec<usize>>) -> Self {
        Self {
            model,
            tag_dictionary,
        }
    }

    pub fn tag(&self, tokens: &[&str]) -> Vec<usize> {
        let model = &self.model;
        let dict = &self.tag_dictionary;
        beam_search(
            model,
            tokens.len(),
            3,
            &mut |index, outcomes| pos_contexts(index, tokens, outcomes, model),
            &|index, _outcomes, outcome| {
                dict.get(tokens[index])
                    .map(|tags| tags.contains(&outcome))
                    .unwrap_or(true)
            },
        )
    }
}

fn pos_contexts(
    index: usize,
    tokens: &[&str],
    outcomes: &[usize],
    model: &GisModel,
) -> Vec<String> {
    let se = "*SE*";
    let sb = "*SB*";
    let lex = tokens[index];
    let next = if tokens.len() > index + 1 {
        tokens[index + 1]
    } else {
        se
    };
    let nextnext = if tokens.len() > index + 2 {
        Some(tokens[index + 2])
    } else if tokens.len() > index + 1 {
        Some(se)
    } else {
        None
    };

    let mut e = Vec::with_capacity(24);
    e.push("default".to_string());
    e.push(format!("w={lex}"));
    let char_count = lex.chars().count();
    for li in 0..4 {
        let skip = char_count.saturating_sub(li + 1);
        let start = char_index(lex, skip);
        e.push(format!("suf={}", &lex[start..]));
    }
    for li in 0..4 {
        let end = char_index(lex, (li + 1).min(char_count));
        e.push(format!("pre={}", &lex[..end]));
    }
    if lex.contains('-') {
        e.push("h".to_string());
    }
    if lex.chars().any(|c| c.is_ascii_uppercase()) {
        e.push("c".to_string());
    }
    if lex.chars().any(|c| c.is_ascii_digit()) {
        e.push("d".to_string());
    }

    if index >= 1 {
        let prev = tokens[index - 1];
        e.push(format!("p={prev}"));
        e.push(format!("t={}", model.outcome(outcomes[index - 1])));
        if index >= 2 {
            let prevprev = tokens[index - 2];
            e.push(format!("pp={prevprev}"));
            e.push(format!(
                "t2={},{}",
                model.outcome(outcomes[index - 2]),
                model.outcome(outcomes[index - 1])
            ));
        }
    } else {
        e.push(format!("p={sb}"));
    }

    e.push(format!("n={next}"));
    if let Some(nn) = nextnext {
        e.push(format!("nn={nn}"));
    }
    e
}

fn char_index(s: &str, char_position: usize) -> usize {
    s.char_indices()
        .nth(char_position)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

pub struct OpenNlpChunker {
    model: GisModel,
}

impl OpenNlpChunker {
    pub fn new(model: GisModel) -> Self {
        Self { model }
    }

    pub fn chunk(&self, tokens: &[&str], tags: &[&str]) -> Vec<usize> {
        let model = &self.model;
        beam_search(
            model,
            tokens.len(),
            10,
            &mut |index, outcomes| chunker_contexts(index, tokens, tags, outcomes, model),
            &|_index, outcomes, outcome| {
                let name = model.outcome(outcome);
                let prev_name = outcomes.last().map(|o| model.outcome(*o));
                match prev_name {
                    None => !name.starts_with("I-"),
                    Some(prev) => {
                        if name.starts_with("I-") {
                            prev != "O" && name.split_at(2).1 == prev.split_at(2).1
                        } else {
                            true
                        }
                    }
                }
            },
        )
    }
}

fn chunker_contexts(
    index: usize,
    tokens: &[&str],
    tags: &[&str],
    preds: &[usize],
    model: &GisModel,
) -> Vec<String> {
    let n = tokens.len();
    let (w_2, t_2, p_2) = if index < 2 {
        (
            "w_2=bos".to_string(),
            "t_2=bos".to_string(),
            "p_2=bos".to_string(),
        )
    } else {
        (
            format!("w_2={}", tokens[index - 2]),
            format!("t_2={}", tags[index - 2]),
            format!("p_2={}", model.outcome(preds[index - 2])),
        )
    };
    let (w_1, t_1, p_1) = if index < 1 {
        (
            "w_1=bos".to_string(),
            "t_1=bos".to_string(),
            "p_1=bos".to_string(),
        )
    } else {
        (
            format!("w_1={}", tokens[index - 1]),
            format!("t_1={}", tags[index - 1]),
            format!("p_1={}", model.outcome(preds[index - 1])),
        )
    };
    let w0 = format!("w0={}", tokens[index]);
    let t0 = format!("t0={}", tags[index]);
    let (w1, t1) = if index + 1 >= n {
        ("w1=eos".to_string(), "t1=eos".to_string())
    } else {
        (
            format!("w1={}", tokens[index + 1]),
            format!("t1={}", tags[index + 1]),
        )
    };
    let (w2, t2) = if index + 2 >= n {
        ("w2=eos".to_string(), "t2=eos".to_string())
    } else {
        (
            format!("w2={}", tokens[index + 2]),
            format!("t2={}", tags[index + 2]),
        )
    };

    vec![
        w_2.clone(),
        w_1.clone(),
        w0.clone(),
        w1.clone(),
        w2.clone(),
        format!("{w_1}{w0}"),
        format!("{w0}{w1}"),
        t_2.clone(),
        t_1.clone(),
        t0.clone(),
        t1.clone(),
        t2.clone(),
        format!("{t_2}{t_1}"),
        format!("{t_1}{t0}"),
        format!("{t0}{t1}"),
        format!("{t1}{t2}"),
        format!("{t_2}{t_1}{t0}"),
        format!("{t_1}{t0}{t1}"),
        format!("{t0}{t1}{t2}"),
        p_2,
        p_1.clone(),
        format!("{p_1}{t_2}"),
        format!("{p_1}{t_1}"),
        format!("{p_1}{t0}"),
        format!("{p_1}{t1}"),
        format!("{p_1}{t2}"),
        format!("{p_1}{t_2}{t_1}"),
        format!("{p_1}{t_1}{t0}"),
        format!("{p_1}{t0}{t1}"),
        format!("{p_1}{t1}{t2}"),
        format!("{p_1}{t_2}{t_1}{t0}"),
        format!("{p_1}{t_1}{t0}{t1}"),
        format!("{p_1}{t0}{t1}{t2}"),
        format!("{p_1}{w_2}"),
        format!("{p_1}{w_1}"),
        format!("{p_1}{w0}"),
        format!("{p_1}{w1}"),
        format!("{p_1}{w2}"),
        format!("{p_1}{w_1}{w0}"),
        format!("{p_1}{w0}{w1}"),
    ]
}

pub struct OpenNlpEnglishChunker {
    pub tokenizer: OpenNlpTokenizer,
    pos: OpenNlpPosTagger,
    chunker: OpenNlpChunker,
}

fn parse_tag_dictionary(xml: &str, model: &GisModel) -> HashMap<String, Vec<usize>> {
    let mut dict: HashMap<String, Vec<usize>> = HashMap::new();
    let mut current_tags: Option<Vec<usize>> = None;
    let mut buffer = String::new();
    for event in quick_xml_events(xml) {
        match event {
            XmlEvent::Start(name, attrs) => {
                if name == "entry" {
                    current_tags = attrs.iter().find(|(k, _)| k == "tags").and_then(|(_, v)| {
                        let ids = v
                            .split_whitespace()
                            .filter_map(|t| model.outcome_index(t))
                            .collect::<Vec<usize>>();
                        (!ids.is_empty()).then_some(ids)
                    });
                    if current_tags.is_none() {
                        current_tags = Some(Vec::new());
                    }
                }
            }
            XmlEvent::Text(text) => buffer.push_str(&text),
            XmlEvent::End(name) => {
                if name == "token" {
                    let token = buffer.trim().to_string();
                    if let Some(tags) = current_tags.take()
                        && !token.is_empty()
                    {
                        dict.insert(token, tags);
                    }
                } else if name == "entry" {
                    current_tags = None;
                }
                buffer.clear();
            }
        }
    }
    dict
}

enum XmlEvent {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
}

fn quick_xml_events(xml: &str) -> Vec<XmlEvent> {
    let mut events = Vec::new();
    let mut i = 0;
    while let Some(open) = xml[i..].find('<') {
        let abs = i + open;
        if abs > i {
            events.push(XmlEvent::Text(xml[i..abs].to_string()));
        }
        let close = match xml[abs..].find('>') {
            Some(c) => abs + c,
            None => break,
        };
        let inner = &xml[abs + 1..close];
        i = close + 1;
        if let Some(name) = inner.strip_prefix('/') {
            events.push(XmlEvent::End(name.trim().to_string()));
        } else if inner.starts_with('?') || inner.starts_with('!') {
            continue;
        } else {
            let (name, attrs) = parse_tag(inner);
            if name.is_empty() {
                continue;
            }
            if inner.ends_with('/') {
                events.push(XmlEvent::Start(name.clone(), attrs));
                events.push(XmlEvent::End(name));
            } else {
                events.push(XmlEvent::Start(name, attrs));
            }
        }
    }
    events
}

fn parse_tag(inner: &str) -> (String, Vec<(String, String)>) {
    let bytes: Vec<char> = inner.chars().collect();
    let mut p = 0;
    while p < bytes.len() && !bytes[p].is_whitespace() {
        p += 1;
    }
    let name: String = bytes[..p].iter().collect();
    let mut attrs = Vec::new();
    while p < bytes.len() {
        while p < bytes.len() && bytes[p].is_whitespace() {
            p += 1;
        }
        let key_start = p;
        while p < bytes.len() && bytes[p] != '=' && !bytes[p].is_whitespace() {
            p += 1;
        }
        if key_start == p {
            break;
        }
        let key: String = bytes[key_start..p].iter().collect();
        if p < bytes.len() && bytes[p] == '=' {
            p += 1;
            if p < bytes.len() && (bytes[p] == '"' || bytes[p] == '\'') {
                let quote = bytes[p];
                p += 1;
                let value_start = p;
                while p < bytes.len() && bytes[p] != quote {
                    p += 1;
                }
                attrs.push((key, bytes[value_start..p].iter().collect()));
                p += 1;
            } else {
                let value_start = p;
                while p < bytes.len() && !bytes[p].is_whitespace() {
                    p += 1;
                }
                attrs.push((key, bytes[value_start..p].iter().collect()));
            }
        }
    }
    (name, attrs)
}

impl OpenNlpEnglishChunker {
    pub fn from_resource_dir(dir: &Path) -> Option<Self> {
        let tokenizer = GisModel::from_file(&dir.join("en-token.model"))?;
        let pos_model = GisModel::from_file(&dir.join("en-pos-maxent.model"))?;
        let chunk_model = GisModel::from_file(&dir.join("en-chunker.model"))?;
        let tagdict = parse_tag_dictionary(
            &std::fs::read_to_string(dir.join("en-pos-maxent.tagdict")).ok()?,
            &pos_model,
        );
        Some(Self {
            tokenizer: OpenNlpTokenizer::new(tokenizer),
            pos: OpenNlpPosTagger::new(pos_model, tagdict),
            chunker: OpenNlpChunker::new(chunk_model),
        })
    }

    pub fn shared() -> Option<&'static Self> {
        static SHARED: std::sync::OnceLock<Option<OpenNlpEnglishChunker>> =
            std::sync::OnceLock::new();
        SHARED
            .get_or_init(|| {
                let candidates = [
                    "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/resource/en",
                    "languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/resource/en",
                ];
                candidates
                    .iter()
                    .find_map(|p| Self::from_resource_dir(Path::new(p)))
            })
            .as_ref()
    }

    pub fn install(resource_dir: &Path) {
        static SHARED: std::sync::OnceLock<Option<OpenNlpEnglishChunker>> =
            std::sync::OnceLock::new();
        let _ = SHARED.set(Self::from_resource_dir(resource_dir));
    }

    pub fn add_chunk_tags_on(&self, readings: &mut [og_core::AnalyzedTokenReadings]) {
        let spans_and_tags = self.chunk_readings(readings);
        for (idx, chunk_tags) in spans_and_tags {
            readings[idx].set_chunks(chunk_tags);
        }
    }

    fn chunk_readings(
        &self,
        readings: &[og_core::AnalyzedTokenReadings],
    ) -> Vec<(usize, Vec<String>)> {
        let mut text = String::new();
        for token in readings {
            if is_sentence_start(token) {
                continue;
            }
            text.push_str(token.token().token());
        }
        let clean = text.replace('\u{2019}', "'");
        let spans = self.tokenizer.tokenize_spans(&clean);
        let token_texts: Vec<&str> = spans.iter().map(|(s, e)| &clean[*s..*e]).collect();
        let tag_indices = self.pos.tag(&token_texts);
        let tag_names: Vec<&str> = tag_indices
            .iter()
            .map(|i| self.pos.model.outcome(*i))
            .collect();
        let chunk_indices = self.chunker.chunk(&token_texts, &tag_names);
        let mapped = map_tokens_to_readings(&token_texts, readings);
        let raw_tags: Vec<Option<&str>> = chunk_indices
            .iter()
            .map(|i| Some(self.chunker.model.outcome(*i)))
            .collect();
        let filtered = english_chunk_filter(&raw_tags, &token_texts, &mapped, readings);
        let mut result = Vec::with_capacity(filtered.len());
        for (idx, chunk_tags) in filtered.iter().enumerate() {
            if chunk_tags.is_empty() {
                continue;
            }
            if let Some(atr_idx) = mapped[idx] {
                result.push((atr_idx, chunk_tags.clone()));
            }
        }
        result
    }

    pub fn add_chunk_tags(&self, sentence: &mut og_core::AnalyzedSentence) {
        let updates = self.chunk_readings(sentence.tokens());
        let tokens = sentence.tokens_mut();
        for (idx, chunk_tags) in updates {
            tokens[idx].set_chunks(chunk_tags);
        }
    }
}

fn is_sentence_start(reading: &og_core::AnalyzedTokenReadings) -> bool {
    reading.token().token() == "<S>" && reading.token().has_pos_tag("SENT_START")
}

fn map_tokens_to_readings(
    token_texts: &[&str],
    readings: &[og_core::AnalyzedTokenReadings],
) -> Vec<Option<usize>> {
    let mut result: Vec<Option<usize>> = vec![None; token_texts.len()];
    let mut opennlp_pos = 0usize;
    let mut reading_pos = 0usize;
    let mut reading_idx = 0usize;
    for (i, token) in token_texts.iter().enumerate() {
        let start = opennlp_pos;
        let end = start + token.len();
        opennlp_pos = end;
        while reading_idx < readings.len() {
            let current = reading_idx;
            let reading_token = readings[current].token().token();
            let skip = is_sentence_start(&readings[current])
                || reading_token.trim().is_empty()
                || (reading_token.chars().count() == 1 && reading_token.starts_with('\u{a0}'));
            if skip {
                reading_idx += 1;
                continue;
            }
            let normalized_len = reading_token.replace('\u{2019}', "'").len();
            let r_start = reading_pos;
            let r_end = r_start + normalized_len;
            reading_pos = r_end;
            reading_idx += 1;
            if r_start == start && r_end == end {
                result[i] = Some(current);
                break;
            }
            if r_start > start {
                break;
            }
        }
    }
    result
}

fn english_chunk_filter(
    raw_tags: &[Option<&str>],
    token_texts: &[&str],
    mapped: &[Option<usize>],
    readings: &[og_core::AnalyzedTokenReadings],
) -> Vec<Vec<String>> {
    let n = raw_tags.len();
    let is_begin_np = |i: usize| raw_tags.get(i) == Some(&Some("B-NP"));
    let is_cont_np = |i: usize| raw_tags.get(i) == Some(&Some("I-NP"));
    let is_end_np = |i: usize| if i + 2 > n { true } else { !is_cont_np(i + 1) };

    let has_plural_reading = |i: usize| -> bool {
        mapped
            .get(i)
            .and_then(|r| *r)
            .map(|r| readings[r].has_pos_tag("NNS"))
            .unwrap_or(false)
    };

    let chunk_type_plural = |start: usize| -> bool {
        for j in start..n {
            if !is_begin_np(j) && !is_cont_np(j) {
                break;
            }
            if has_plural_reading(j) {
                return true;
            }
        }
        false
    };

    let end_of_np_is_singular = |start: usize| -> bool {
        for j in start..n {
            if is_end_np(j) {
                return !chunk_type_plural(start);
            }
        }
        false
    };

    let mut result: Vec<Vec<String>> = vec![Vec::new(); n];
    let mut new_chunk_tag: Option<String> = None;
    for i in 0..n {
        let mut tags: Vec<String> = Vec::new();
        if is_begin_np(i) {
            let plural = chunk_type_plural(i);
            if !plural || end_of_np_is_singular(i) {
                tags.push("B-NP-singular".to_string());
                new_chunk_tag = Some("NP-singular".to_string());
            } else {
                tags.push("B-NP-plural".to_string());
                new_chunk_tag = Some("NP-plural".to_string());
            }
        }
        if new_chunk_tag.is_some() && is_end_np(i) {
            tags.push(format!("E-{}", new_chunk_tag.clone().unwrap()));
            new_chunk_tag = None;
        }
        if new_chunk_tag.is_some() && is_cont_np(i) {
            tags.push(format!("I-{}", new_chunk_tag.clone().unwrap()));
        }
        if tags.is_empty()
            && let Some(tag) = raw_tags[i]
        {
            tags.push(tag.to_string());
        }
        result[i] = tags;
    }
    let _ = token_texts;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource_dir() -> Option<&'static std::path::Path> {
        static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
        DIR.get_or_init(|| {
            [
                "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/resource/en",
                "languagetool/languagetool-language-modules/en/src/main/resources/org/languagetool/resource/en",
            ]
            .iter()
            .map(std::path::PathBuf::from)
            .find(|p| p.join("en-chunker.model").exists())
        })
        .as_deref()
    }

    fn chunker() -> Option<OpenNlpEnglishChunker> {
        resource_dir().and_then(OpenNlpEnglishChunker::from_resource_dir)
    }

    #[test]
    fn tokenizer_splits_punctuation_from_words() {
        let Some(dir) = resource_dir() else { return };
        let model = GisModel::from_file(&dir.join("en-token.model")).unwrap();
        let tok = OpenNlpTokenizer::new(model);
        let spans = tok.tokenize_spans("Hello world, don't shout!");
        let toks: Vec<&str> = spans
            .iter()
            .map(|(s, e)| &"Hello world, don't shout!"[*s..*e])
            .collect();
        assert_eq!(toks, vec!["Hello", "world", ",", "do", "n't", "shout", "!"]);
    }

    #[test]
    fn pos_tagger_labels_a_simple_sentence() {
        let Some(dir) = resource_dir() else { return };
        let model = GisModel::from_file(&dir.join("en-pos-maxent.model")).unwrap();
        let dict = parse_tag_dictionary(
            &std::fs::read_to_string(dir.join("en-pos-maxent.tagdict")).unwrap(),
            &model,
        );
        assert!(!dict.is_empty());
        let tagger = OpenNlpPosTagger::new(model, dict);
        let words = [
            "The", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog", ".",
        ];
        let tags: Vec<String> = tagger
            .tag(&words)
            .iter()
            .map(|i| tagger.model.outcome(*i).to_string())
            .collect();
        println!("TAGS: {tags:?}");
        assert_eq!(tags[0], "DT");
        assert_eq!(tags[3], "NN");
        assert_eq!(tags[7], "JJ");
        assert_eq!(tags[8], "NN");
        assert_eq!(tags[9], ".");
    }

    #[test]
    fn java_english_chunker_test_add_chunk_tags() {
        let Some(chunker) = chunker() else { return };
        let sentence = "A short test of the bicycle is needed";
        let mut readings = Vec::new();
        let mut pos = 0;
        let mut parts: Vec<&str> = Vec::new();
        let mut last = 0;
        for (i, c) in sentence.char_indices() {
            if c == ' ' {
                if i > last {
                    parts.push(&sentence[last..i]);
                }
                parts.push(&sentence[i..i + 1]);
                last = i + 1;
            }
        }
        if last < sentence.len() {
            parts.push(&sentence[last..]);
        }
        for part in parts {
            let at = og_core::AnalyzedToken::new(part, pos, pos + part.len());
            readings.push(og_core::AnalyzedTokenReadings::new(at));
            pos += part.len();
        }
        chunker.add_chunk_tags_on(&mut readings);
        let tags: Vec<String> = readings.iter().map(|r| r.chunks().join("|")).collect();
        assert_eq!(readings.len(), 15);
        assert_eq!(tags[0], "B-NP-singular", "{tags:?}");
        assert_eq!(tags[2], "I-NP-singular", "{tags:?}");
        assert_eq!(tags[4], "E-NP-singular", "{tags:?}");
        assert_eq!(tags[8], "B-NP-singular", "{tags:?}");
        assert_eq!(tags[10], "E-NP-singular", "{tags:?}");
        assert_eq!(tags[12], "B-VP", "{tags:?}");
        assert_eq!(tags[14], "I-VP", "{tags:?}");
    }

    #[test]
    fn chunker_labels_noun_phrases() {
        let Some(chunker) = chunker() else { return };
        let words = [
            "The", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog", ".",
        ];
        let tags = ["DT", "JJ", "JJ", "NN", "VBZ", "IN", "DT", "JJ", "NN", "."];
        let chunks: Vec<String> = chunker
            .chunker
            .chunk(&words, &tags)
            .iter()
            .map(|i| chunker.chunker.model.outcome(*i).to_string())
            .collect();
        assert_eq!(chunks[0], "B-NP");
        assert_eq!(chunks[3], "I-NP");
        assert_eq!(chunks[4], "B-VP");
        assert_eq!(chunks[9], "O");
    }

    #[test]
    fn full_pipeline_assignes_np_chunks_to_readings() {
        let Some(chunker) = chunker() else { return };
        let text = "The quick brown fox jumps over the lazy dog.";
        let mut sentence = og_core::AnalyzedSentence::new(text, 0, text.len());
        let mut tokens = Vec::new();
        let mut pos = 0;
        let mut last = 0;
        let mut parts: Vec<&str> = Vec::new();
        for (i, c) in text.char_indices() {
            if c == ' ' {
                if i > last {
                    parts.push(&text[last..i]);
                }
                parts.push(&text[i..i + 1]);
                last = i + 1;
            }
        }
        if last < text.len() {
            parts.push(&text[last..]);
        }
        for part in parts {
            let at = og_core::AnalyzedToken::new(part, pos, pos + part.len());
            tokens.push(og_core::AnalyzedTokenReadings::new(at));
            pos += part.len();
        }
        sentence.set_tokens(tokens);
        chunker.add_chunk_tags(&mut sentence);
        let chunks: Vec<String> = sentence
            .tokens()
            .iter()
            .map(|t| t.chunks().join("|"))
            .collect();
        assert!(chunks[0].starts_with("B-NP"), "got {:?}", chunks);
        assert!(chunks[8].contains("E-NP"), "got {:?}", chunks);
        assert!(chunks[10].contains("B-PP"), "got {:?}", chunks);
        assert!(chunks[16].is_empty(), "got {:?}", chunks);
    }
}
