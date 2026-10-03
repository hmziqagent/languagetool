use std::path::Path;

type ReadingList = Vec<(&'static str, &'static str)>;
type EntriesMap = FxHashMap<&'static str, Box<[(&'static str, &'static str)]>>;
pub type FormsByLemma = FxHashMap<&'static str, Vec<(&'static str, &'static str)>>;
type SynthByLemma = FormsByLemma;
type VerbForms = std::collections::HashMap<String, Vec<String>>;

#[derive(Default)]
pub struct FxHasher {
    hash: u64,
}

const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(FX_SEED);
    }
}

impl std::hash::Hasher for FxHasher {
    fn finish(&self) -> u64 {
        self.hash
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_le_bytes(chunk.try_into().unwrap()));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut buf = [0u8; 8];
            buf[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(buf));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.add(i as u64)
    }

    fn write_u16(&mut self, i: u16) {
        self.add(i as u64)
    }

    fn write_u32(&mut self, i: u32) {
        self.add(i as u64)
    }

    fn write_u64(&mut self, i: u64) {
        self.add(i)
    }

    fn write_usize(&mut self, i: usize) {
        self.add(i as u64)
    }
}

pub type FxBuildHasher = std::hash::BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;
pub type FxHashSet<T> = std::collections::HashSet<T, FxBuildHasher>;

fn intern<'a>(cache: &mut FxHashSet<&'a str>, owned: String) -> &'a str {
    if let Some(existing) = cache.get(owned.as_str()) {
        return existing;
    }
    let leaked: &'a str = Box::leak(owned.into_boxed_str());
    cache.insert(leaked);
    leaked
}

struct Fsa5 {
    arcs: &'static [u8],
    node_data_length: usize,
    gtl: usize,
}

const FSA5_BIT_FINAL_ARC: u8 = 1 << 0;
const FSA5_BIT_LAST_ARC: u8 = 1 << 1;
const FSA5_BIT_TARGET_NEXT: u8 = 1 << 2;
const FSA5_ADDRESS_OFFSET: usize = 1;

impl Fsa5 {
    fn from_bytes(data: &'static [u8]) -> Option<Self> {
        if data.len() < 8 || &data[0..4] != b"\\fsa" || data[4] != 0x05 {
            return None;
        }
        let hgtl = *data.get(7)?;
        let node_data_length = ((hgtl >> 4) & 0x0f) as usize;
        let gtl = (hgtl & 0x0f) as usize;
        if gtl == 0 {
            return None;
        }
        Some(Self {
            arcs: data.get(8..)?,
            node_data_length,
            gtl,
        })
    }

    fn is_next_set(&self, arc: usize) -> bool {
        self.arcs
            .get(arc + FSA5_ADDRESS_OFFSET)
            .is_some_and(|f| f & FSA5_BIT_TARGET_NEXT != 0)
    }

    fn skip_arc(&self, offset: usize) -> Option<usize> {
        if self.is_next_set(offset) {
            offset.checked_add(2)
        } else {
            offset.checked_add(1 + self.gtl)
        }
    }

    fn is_arc_last(&self, arc: usize) -> bool {
        self.arcs
            .get(arc + FSA5_ADDRESS_OFFSET)
            .is_some_and(|f| f & FSA5_BIT_LAST_ARC != 0)
    }

    fn is_arc_final(&self, arc: usize) -> bool {
        self.arcs
            .get(arc + FSA5_ADDRESS_OFFSET)
            .is_some_and(|f| f & FSA5_BIT_FINAL_ARC != 0)
    }

    fn destination(&self, arc: usize) -> Option<usize> {
        if self.is_next_set(arc) {
            self.skip_arc(arc)
        } else {
            let mut value = 0usize;
            for i in (0..self.gtl).rev() {
                let b = *self.arcs.get(arc + FSA5_ADDRESS_OFFSET + i)?;
                value = (value << 8) | b as usize;
            }
            Some(value >> 3)
        }
    }

    fn root_node(&self) -> Option<usize> {
        let first = self.node_data_length;
        let epsilon = self.skip_arc(first)?;
        let eps_first = epsilon + self.node_data_length;
        self.destination(eps_first)
    }

    fn walk(&self, node: usize, sequence: &mut Vec<u8>, out: &mut dyn FnMut(&[u8])) {
        let mut arc = Some(self.node_data_length + node);
        while let Some(current) = arc {
            let Some(&label) = self.arcs.get(current) else {
                break;
            };
            sequence.push(label);
            if self.is_arc_final(current) {
                out(sequence);
            }
            if let Some(dest) = self.destination(current)
                && dest != 0
            {
                self.walk(dest, sequence, out);
            }
            sequence.pop();
            if self.is_arc_last(current) {
                break;
            }
            arc = self.skip_arc(current);
        }
    }

    fn seek(&self, node: usize, key: &[u8]) -> Option<usize> {
        let mut node = node;
        for &byte in key {
            let mut arc = Some(self.node_data_length + node);
            let mut next = None;
            while let Some(current) = arc {
                let Some(&label) = self.arcs.get(current) else {
                    break;
                };
                if label == byte {
                    next = self.destination(current);
                    break;
                }
                if self.is_arc_last(current) {
                    break;
                }
                arc = self.skip_arc(current);
            }
            node = match next {
                Some(dest) if dest != 0 => dest,
                _ => return None,
            };
        }
        Some(node)
    }
}

struct Cfsa1 {
    arcs: &'static [u8],
    node_data_length: usize,
    gtl: usize,
    label_mapping: &'static [u8],
}

const CFSA1_BIT_FINAL_ARC: u8 = 1 << 0;
const CFSA1_BIT_LAST_ARC: u8 = 1 << 1;
const CFSA1_BIT_TARGET_NEXT: u8 = 1 << 2;

impl Cfsa1 {
    fn from_bytes(data: &'static [u8]) -> Option<Self> {
        if data.len() < 44 || &data[0..4] != b"\\fsa" || data[4] != 0xc5 {
            return None;
        }
        let hgtl = *data.get(7)?;
        let node_data_length = if hgtl & 0xf0 != 0 {
            ((hgtl >> 4) & 0x0f) as usize
        } else {
            0
        };
        let gtl = (hgtl & 0x0f) as usize;
        if gtl == 0 {
            return None;
        }
        Some(Self {
            arcs: data.get(40..)?,
            node_data_length,
            gtl,
            label_mapping: data.get(8..40)?,
        })
    }

    fn is_next_set(&self, arc: usize) -> bool {
        self.arcs
            .get(arc)
            .is_some_and(|f| f & CFSA1_BIT_TARGET_NEXT != 0)
    }

    fn is_label_compressed(&self, arc: usize) -> bool {
        self.is_next_set(arc) && (self.arcs[arc] >> 3) & 0x1f != 0
    }

    fn arc_label(&self, arc: usize) -> u8 {
        if self.is_label_compressed(arc) {
            self.label_mapping[(self.arcs[arc] >> 3) as usize & 0x1f]
        } else {
            self.arcs[arc + 1]
        }
    }

    fn skip_arc(&self, arc: usize) -> Option<usize> {
        if self.is_next_set(arc) {
            if self.is_label_compressed(arc) {
                arc.checked_add(1)
            } else {
                arc.checked_add(2)
            }
        } else {
            arc.checked_add(1 + self.gtl)
        }
    }

    fn destination(&self, arc: usize) -> Option<usize> {
        if self.is_next_set(arc) {
            self.skip_arc(arc)
        } else {
            let mut value = 0usize;
            for i in (1..self.gtl).rev() {
                let b = *self.arcs.get(arc + 1 + i)?;
                value = (value << 8) | b as usize;
            }
            let flag = *self.arcs.get(arc)? as usize;
            Some(((value << 8) | flag) >> 3)
        }
    }

    fn root_node(&self) -> Option<usize> {
        let epsilon = self.skip_arc(self.node_data_length)?;
        self.destination(epsilon + self.node_data_length)
    }

    fn walk(&self, node: usize, sequence: &mut Vec<u8>, out: &mut dyn FnMut(&[u8])) {
        let mut arc = Some(self.node_data_length + node);
        while let Some(current) = arc {
            let Some(&flag) = self.arcs.get(current) else {
                break;
            };
            let label = self.arc_label(current);
            sequence.push(label);
            if flag & CFSA1_BIT_FINAL_ARC != 0 {
                out(sequence);
            }
            if let Some(dest) = self.destination(current)
                && dest != 0
            {
                self.walk(dest, sequence, out);
            }
            sequence.pop();
            if flag & CFSA1_BIT_LAST_ARC != 0 {
                break;
            }
            arc = self.skip_arc(current);
        }
    }

    fn seek(&self, node: usize, key: &[u8]) -> Option<usize> {
        let mut node = node;
        for &byte in key {
            let mut arc = Some(self.node_data_length + node);
            let mut next = None;
            while let Some(current) = arc {
                let Some(&flag) = self.arcs.get(current) else {
                    break;
                };
                if self.arc_label(current) == byte {
                    next = self.destination(current);
                    break;
                }
                if flag & CFSA1_BIT_LAST_ARC != 0 {
                    break;
                }
                arc = self.skip_arc(current);
            }
            node = match next {
                Some(dest) if dest != 0 => dest,
                _ => return None,
            };
        }
        Some(node)
    }
}

struct Cfsa2 {
    arcs: &'static [u8],
    label_mapping: &'static [u8],
    has_numbers: bool,
}

fn read_vint(data: &[u8], offset: usize) -> Option<(usize, usize)> {
    let mut shift = 0;
    let mut value = 0usize;
    let mut pos = offset;
    loop {
        let byte = *data.get(pos)?;
        pos += 1;
        value |= ((byte & 0x7f) as usize) << shift;
        if byte & 0x80 == 0 {
            return Some((value, pos));
        }
        shift += 7;
    }
}

impl Cfsa2 {
    fn from_bytes(data: &'static [u8]) -> Option<Self> {
        if data.len() < 8 || &data[0..4] != b"\\fsa" || data[4] != 0xc6 {
            return None;
        }
        let mut pos = 5;
        let flag_bits = u16::from_be_bytes([*data.get(pos)?, *data.get(pos + 1)?]);
        pos += 2;
        let has_numbers = flag_bits & (1 << 8) != 0;
        let label_mapping_size = *data.get(pos)? as usize;
        pos += 1;
        let label_mapping = data.get(pos..pos + label_mapping_size)?;
        pos += label_mapping_size;
        Some(Self {
            arcs: data.get(pos..)?,
            label_mapping,
            has_numbers,
        })
    }

    fn first_arc(&self, node: usize) -> usize {
        if self.has_numbers {
            read_vint(self.arcs, node).map_or(node, |(_, next)| next)
        } else {
            node
        }
    }

    fn arc_label(&self, arc: usize) -> u8 {
        let index = self.arcs[arc] & 0x1f;
        if index > 0 {
            self.label_mapping[index as usize]
        } else {
            self.arcs[arc + 1]
        }
    }

    fn is_arc_last(&self, arc: usize) -> bool {
        self.arcs[arc] & 0x40 != 0
    }

    fn is_next_set(&self, arc: usize) -> bool {
        self.arcs[arc] & 0x80 != 0
    }

    fn skip_arc(&self, arc: usize) -> usize {
        let mut offset = arc + 1;
        if self.arcs[arc] & 0x1f == 0 {
            offset += 1;
        }
        if !self.is_next_set(arc) {
            while self.arcs.get(offset).is_some_and(|b| b & 0x80 != 0) {
                offset += 1;
            }
            offset += 1;
        }
        offset
    }

    fn next_arc(&self, arc: usize) -> Option<usize> {
        if self.is_arc_last(arc) {
            None
        } else {
            Some(self.skip_arc(arc))
        }
    }

    fn destination(&self, arc: usize) -> usize {
        if self.is_next_set(arc) {
            let mut current = arc;
            while !self.is_arc_last(current) {
                current = self.next_arc(current).unwrap_or(current);
            }
            self.skip_arc(current)
        } else {
            let start = arc + if self.arcs[arc] & 0x1f == 0 { 2 } else { 1 };
            read_vint(self.arcs, start).map_or(0, |(value, _)| value)
        }
    }

    fn root(&self) -> usize {
        self.destination(self.first_arc(0))
    }

    fn is_arc_final(&self, arc: usize) -> bool {
        self.arcs[arc] & 0x20 != 0
    }

    fn seek(&self, node: usize, key: &[u8]) -> Option<usize> {
        let mut node = node;
        for &byte in key {
            let mut arc = Some(self.first_arc(node));
            let mut next = None;
            while let Some(current) = arc {
                if self.arc_label(current) == byte {
                    next = Some(self.destination(current));
                    break;
                }
                arc = self.next_arc(current);
            }
            node = match next {
                Some(dest) if dest != 0 => dest,
                _ => return None,
            };
        }
        Some(node)
    }

    fn walk(&self, node: usize, sequence: &mut Vec<u8>, out: &mut dyn FnMut(&[u8])) {
        let mut arc = Some(self.first_arc(node));
        while let Some(current) = arc {
            let label = self.arc_label(current);
            sequence.push(label);
            if self.is_arc_final(current) {
                out(sequence);
            }
            let dest = self.destination(current);
            if dest != 0 {
                self.walk(dest, sequence, out);
            }
            sequence.pop();
            arc = self.next_arc(current);
        }
    }
}

enum Fsa {
    V5(Fsa5),
    C1(Cfsa1),
    C2(Cfsa2),
}

impl Fsa {
    fn from_static(data: &'static [u8]) -> Option<Self> {
        if let Some(f) = Cfsa2::from_bytes(data) {
            return Some(Self::C2(f));
        }
        if let Some(f) = Fsa5::from_bytes(data) {
            return Some(Self::V5(f));
        }
        if let Some(f) = Cfsa1::from_bytes(data) {
            return Some(Self::C1(f));
        }
        None
    }

    fn for_each_sequence(&self, out: &mut dyn FnMut(&[u8])) {
        let mut sequence: Vec<u8> = Vec::new();
        match self {
            Self::V5(f) => {
                if let Some(root) = f.root_node() {
                    f.walk(root, &mut sequence, out);
                }
            }
            Self::C1(f) => {
                if let Some(root) = f.root_node() {
                    f.walk(root, &mut sequence, out);
                }
            }
            Self::C2(f) => f.walk(f.root(), &mut sequence, out),
        }
    }

    fn seek(&self, key: &[u8]) -> Option<usize> {
        match self {
            Self::V5(f) => f.root_node().and_then(|root| f.seek(root, key)),
            Self::C1(f) => f.root_node().and_then(|root| f.seek(root, key)),
            Self::C2(f) => f.seek(f.root(), key),
        }
    }

    fn has_sequence(&self) -> bool {
        match self {
            Self::V5(f) => f
                .root_node()
                .is_some_and(|root| root != 0 && f.arcs.get(f.node_data_length + root).is_some()),
            Self::C1(f) => f
                .root_node()
                .is_some_and(|root| root != 0 && f.arcs.get(f.node_data_length + root).is_some()),
            Self::C2(f) => f.root() != 0 && f.arcs.get(f.root()).is_some(),
        }
    }

    fn separator_suffixes(&self, node: usize, separator: u8, out: &mut dyn FnMut(&[u8])) {
        match self {
            Self::V5(f) => {
                let mut arc = Some(f.node_data_length + node);
                while let Some(current) = arc {
                    let Some(&label) = f.arcs.get(current) else {
                        break;
                    };
                    if label == separator
                        && let Some(dest) = f.destination(current)
                        && dest != 0
                    {
                        let mut sequence: Vec<u8> = Vec::new();
                        f.walk(dest, &mut sequence, out);
                    }
                    if f.is_arc_last(current) {
                        break;
                    }
                    arc = f.skip_arc(current);
                }
            }
            Self::C1(f) => {
                let mut arc = Some(f.node_data_length + node);
                while let Some(current) = arc {
                    let Some(&flag) = f.arcs.get(current) else {
                        break;
                    };
                    if f.arc_label(current) == separator
                        && let Some(dest) = f.destination(current)
                        && dest != 0
                    {
                        let mut sequence: Vec<u8> = Vec::new();
                        f.walk(dest, &mut sequence, out);
                    }
                    if flag & CFSA1_BIT_LAST_ARC != 0 {
                        break;
                    }
                    arc = f.skip_arc(current);
                }
            }
            Self::C2(f) => {
                let mut arc = Some(f.first_arc(node));
                while let Some(current) = arc {
                    if f.arc_label(current) == separator {
                        let dest = f.destination(current);
                        if dest != 0 {
                            let mut sequence: Vec<u8> = Vec::new();
                            f.walk(dest, &mut sequence, out);
                        }
                    }
                    arc = f.next_arc(current);
                }
            }
        }
    }
}

fn decode_suffix(word: &[u8], encoded: &[u8]) -> Option<Vec<u8>> {
    if encoded.is_empty() {
        return None;
    }
    let trim = encoded[0].wrapping_sub(b'A') as usize;
    let trim = if trim == 255 { word.len() } else { trim };
    if trim > word.len() {
        return None;
    }
    Some([&word[..word.len() - trim], &encoded[1..]].concat())
}

fn decode_prefix_and_suffix(word: &[u8], encoded: &[u8]) -> Option<Vec<u8>> {
    if encoded.len() < 2 {
        return None;
    }
    let trim_start = encoded[0].wrapping_sub(b'A') as usize;
    let trim_end = encoded[1].wrapping_sub(b'A') as usize;
    let (trim_start, trim_end) = if trim_start == 255 || trim_end == 255 {
        (word.len(), 0)
    } else {
        (trim_start, trim_end)
    };
    if trim_start + trim_end > word.len() {
        return None;
    }
    Some([&word[trim_start..word.len() - trim_end], &encoded[2..]].concat())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LemmaEncoder {
    Raw,
    Suffix,
    PrefixAndSuffix,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DictEncoding {
    Utf8,
    Koi8R,
    Windows1251,
    Latin1,
    Latin15,
}

impl DictEncoding {
    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "koi8-r" => Self::Koi8R,
            "windows-1251" | "cp1251" => Self::Windows1251,
            "iso-8859-1" | "iso8859-1" => Self::Latin1,
            "iso-8859-15" | "iso8859-15" => Self::Latin15,
            _ => Self::Utf8,
        }
    }

    fn decode(&self, bytes: &[u8]) -> String {
        match self {
            Self::Koi8R => encoding_rs::KOI8_R.decode(bytes).0.into_owned(),
            Self::Windows1251 => encoding_rs::WINDOWS_1251.decode(bytes).0.into_owned(),
            Self::Latin1 => encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
            Self::Latin15 => {
                let latin = encoding_rs::WINDOWS_1252.decode(bytes).0;
                latin
                    .chars()
                    .map(|c| match c {
                        '¤' => '€',
                        '¦' => 'Š',
                        '¨' => 'š',
                        '´' => 'Ž',
                        '¸' => 'ž',
                        '¼' => 'Œ',
                        '½' => 'œ',
                        '¾' => 'Ÿ',
                        other => other,
                    })
                    .collect()
            }
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
        }
    }

    fn encode(&self, text: &str) -> Option<Vec<u8>> {
        match self {
            Self::Utf8 => Some(text.as_bytes().to_vec()),
            Self::Koi8R => encode_single_byte(text, encoding_rs::KOI8_R),
            Self::Windows1251 => encode_single_byte(text, encoding_rs::WINDOWS_1251),
            Self::Latin1 => encode_single_byte(text, encoding_rs::WINDOWS_1252),
            Self::Latin15 => {
                let mapped: String = text
                    .chars()
                    .map(|c| match c {
                        '€' => '¤',
                        'Š' => '¦',
                        'š' => '¨',
                        'Ž' => '´',
                        'ž' => '¸',
                        'Œ' => '¼',
                        'œ' => '½',
                        'Ÿ' => '¾',
                        other => other,
                    })
                    .collect();
                encode_single_byte(&mapped, encoding_rs::WINDOWS_1252)
            }
        }
    }
}

fn encode_single_byte(text: &str, encoding: &'static encoding_rs::Encoding) -> Option<Vec<u8>> {
    let (bytes, _, had_errors) = encoding.encode(text);
    if had_errors {
        return None;
    }
    Some(bytes.into_owned())
}

fn decode_bytes(encoding: &str, bytes: &[u8]) -> String {
    DictEncoding::parse(encoding).decode(bytes)
}

fn info_value(info: &str, key: &str) -> Option<String> {
    info.lines()
        .find_map(|l| l.trim().strip_prefix(key))
        .map(str::trim)
        .map(str::to_string)
}

pub fn load_fsa_word_list(dict_path: &Path, info_path: &Path) -> Option<Vec<String>> {
    let data: &'static [u8] = Box::leak(std::fs::read(dict_path).ok()?.into_boxed_slice());
    let info = std::fs::read_to_string(info_path).ok()?;
    let separator = info_value(&info, "fsa.dict.separator=")?.chars().next()?;
    let encoding = info_value(&info, "fsa.dict.encoding=").unwrap_or_else(|| "utf8".into());
    let sep = separator as u8;
    let fsa = Fsa::from_static(data)?;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    fsa.for_each_sequence(&mut |sequence: &[u8]| {
        let word = sequence.split(|b| *b == sep).next().unwrap_or(&[]);
        if word.is_empty() {
            return;
        }
        seen.insert(decode_bytes(&encoding, word));
    });
    Some(seen.into_iter().collect())
}

struct FsaDict {
    fsa: Fsa,
    separator: u8,
    encoder: LemmaEncoder,
    encoding: DictEncoding,
    cache: std::sync::RwLock<FxHashMap<&'static str, &'static [(&'static str, &'static str)]>>,
    interner: std::sync::Mutex<FxHashSet<&'static str>>,
}

struct Materialized {
    entries: EntriesMap,
    synth_by_lemma: Option<SynthByLemma>,
    forms_by_lemma: std::sync::OnceLock<FormsByLemma>,
    verb_forms: std::sync::OnceLock<VerbForms>,
}

enum Source {
    Fsa {
        dict: FsaDict,
        materialized: std::sync::OnceLock<Materialized>,
    },
    Decoded(Materialized),
}

pub struct MorfologikDictionary {
    source: Source,
}

impl FsaDict {
    fn decode_lemma(&self, word_bytes: &[u8], second: &[u8]) -> Option<Vec<u8>> {
        match self.encoder {
            LemmaEncoder::Raw => Some(second.to_vec()),
            LemmaEncoder::Suffix => decode_suffix(word_bytes, second),
            LemmaEncoder::PrefixAndSuffix => decode_prefix_and_suffix(word_bytes, second),
        }
    }

    fn lookup_uncached(&self, word: &str) -> ReadingList {
        let Some(word_bytes) = self.encoding.encode(word) else {
            return Vec::new();
        };
        let Some(node) = self.fsa.seek(&word_bytes) else {
            return Vec::new();
        };
        let mut readings: ReadingList = Vec::new();
        {
            let mut interner = self.interner.lock().unwrap();
            self.fsa
                .separator_suffixes(node, self.separator, &mut |suffix: &[u8]| {
                    let mut fields = suffix.split(|b| *b == self.separator);
                    let (Some(second), Some(tag_raw)) = (fields.next(), fields.next()) else {
                        return;
                    };
                    let Some(lemma_bytes) = self.decode_lemma(&word_bytes, second) else {
                        return;
                    };
                    let tag = intern(&mut interner, self.encoding.decode(tag_raw));
                    let lemma = intern(&mut interner, self.encoding.decode(&lemma_bytes));
                    readings.push((lemma, tag));
                });
        }
        readings
    }

    fn materialize(&self) -> Materialized {
        let mut interner = self.interner.lock().unwrap();
        let mut entries: FxHashMap<&'static str, ReadingList> = FxHashMap::default();
        let mut synth_map: SynthByLemma = FxHashMap::default();
        let mut forms: FormsByLemma = FxHashMap::default();
        let mut verb_forms: VerbForms = std::collections::HashMap::new();
        self.fsa.for_each_sequence(&mut |sequence: &[u8]| {
            let mut fields = sequence.split(|b| *b == self.separator);
            let (word_raw, second, tag_raw) = match (fields.next(), fields.next(), fields.next()) {
                (Some(w), Some(s), Some(t)) => (w, s, Some(t)),
                (Some(w), Some(s), None) => (w, s, None),
                _ => return,
            };
            let Some(lemma_bytes) = self.decode_lemma(word_raw, second) else {
                return;
            };
            let word = self.encoding.decode(word_raw);
            match tag_raw {
                None => {
                    let Some((lemma, tag)) = word.split_once('|') else {
                        return;
                    };
                    let lemma_id = intern(&mut interner, lemma.to_string());
                    let tag_id = intern(&mut interner, tag.to_string());
                    let form_id = intern(&mut interner, self.encoding.decode(&lemma_bytes));
                    synth_map
                        .entry(lemma_id)
                        .or_default()
                        .push((tag_id, form_id));
                }
                Some(tag_raw) => {
                    let tag = intern(&mut interner, self.encoding.decode(tag_raw));
                    let lemma = intern(&mut interner, self.encoding.decode(&lemma_bytes));
                    let word_id = intern(&mut interner, word);
                    entries.entry(word_id).or_default().push((lemma, tag));
                    forms.entry(lemma).or_default().push((tag, word_id));
                    if tag.starts_with("VER:") {
                        let entry = verb_forms.entry(lemma.to_string()).or_default();
                        if !entry.iter().any(|w| w.as_str() == word_id) {
                            entry.push(word_id.to_string());
                        }
                    }
                }
            }
        });
        Materialized {
            entries: entries
                .into_iter()
                .map(|(word, readings)| (word, readings.into_boxed_slice()))
                .collect(),
            synth_by_lemma: if synth_map.is_empty() {
                None
            } else {
                Some(synth_map)
            },
            forms_by_lemma: std::sync::OnceLock::from(forms),
            verb_forms: std::sync::OnceLock::from(verb_forms),
        }
    }
}

impl MorfologikDictionary {
    pub fn load(dict_path: &Path, info_path: &Path) -> Option<Self> {
        let data: &'static [u8] = Box::leak(std::fs::read(dict_path).ok()?.into_boxed_slice());
        let info = std::fs::read_to_string(info_path).ok()?;
        let separator = info_value(&info, "fsa.dict.separator=")?.chars().next()?;
        let encoder = info
            .lines()
            .find_map(|l| l.trim().strip_prefix("fsa.dict.encoder="))
            .map(|v| match v.trim().to_ascii_lowercase().as_str() {
                "suffix" => LemmaEncoder::Suffix,
                "prefix" => LemmaEncoder::PrefixAndSuffix,
                _ => LemmaEncoder::Raw,
            })
            .unwrap_or(LemmaEncoder::Raw);
        let encoding = DictEncoding::parse(
            &info_value(&info, "fsa.dict.encoding=").unwrap_or_else(|| "utf8".into()),
        );
        let fsa = Fsa::from_static(data)?;
        Some(Self {
            source: Source::Fsa {
                dict: FsaDict {
                    fsa,
                    separator: separator as u8,
                    encoder,
                    encoding,
                    cache: std::sync::RwLock::new(FxHashMap::default()),
                    interner: std::sync::Mutex::new(FxHashSet::default()),
                },
                materialized: std::sync::OnceLock::new(),
            },
        })
    }

    pub fn from_decoded_text(data: &str) -> Self {
        let mut interned: FxHashSet<&'static str> = FxHashSet::default();
        let mut map: FxHashMap<&'static str, ReadingList> = FxHashMap::default();
        let mut forms: FormsByLemma = FxHashMap::default();
        let mut verb_forms: VerbForms = std::collections::HashMap::new();
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, '\t');
            let (Some(form), Some(lemma), Some(tag)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let tag = tag.trim();
            if form.is_empty() || tag.is_empty() {
                continue;
            }
            let lemma: &str = if lemma.is_empty() { form } else { lemma };
            let tag = intern(&mut interned, tag.to_string());
            let lemma = intern(&mut interned, lemma.to_string());
            let form_id = intern(&mut interned, form.to_string());
            map.entry(form_id).or_default().push((lemma, tag));
            forms.entry(lemma).or_default().push((tag, form_id));
            if tag.starts_with("VER:") {
                let entry = verb_forms.entry(lemma.to_string()).or_default();
                if !entry.iter().any(|w| w.as_str() == form_id) {
                    entry.push(form_id.to_string());
                }
            }
        }
        Self {
            source: Source::Decoded(Materialized {
                entries: map
                    .into_iter()
                    .map(|(word, readings)| (word, readings.into_boxed_slice()))
                    .collect(),
                synth_by_lemma: None,
                forms_by_lemma: std::sync::OnceLock::from(forms),
                verb_forms: std::sync::OnceLock::from(verb_forms),
            }),
        }
    }

    fn materialized(&self) -> &Materialized {
        match &self.source {
            Source::Fsa { dict, materialized } => materialized.get_or_init(|| dict.materialize()),
            Source::Decoded(m) => m,
        }
    }

    pub fn synth_forms_by_lemma(&self) -> Option<&SynthByLemma> {
        self.materialized().synth_by_lemma.as_ref()
    }

    pub fn word_list(&self) -> Vec<String> {
        self.materialized()
            .entries
            .keys()
            .map(|k| k.to_string())
            .collect()
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.materialized().entries.keys().copied()
    }

    pub fn forms_by_lemma(&self) -> &FormsByLemma {
        self.materialized().forms_by_lemma.get_or_init(|| {
            let mut reverse: FormsByLemma = FxHashMap::default();
            let m = self.materialized();
            for (word, readings) in m.entries.iter() {
                for (lemma, tag) in readings.iter() {
                    reverse.entry(*lemma).or_default().push((*tag, word));
                }
            }
            reverse
        })
    }

    pub fn verb_forms_by_lemma(&self) -> &VerbForms {
        self.materialized().verb_forms.get_or_init(|| {
            let mut reverse: VerbForms = std::collections::HashMap::new();
            let m = self.materialized();
            for (word, readings) in m.entries.iter() {
                for (lemma, tag) in readings.iter() {
                    if tag.starts_with("VER:") {
                        let entry = reverse.entry(lemma.to_string()).or_default();
                        if !entry.iter().any(|w| w == *word) {
                            entry.push(word.to_string());
                        }
                    }
                }
            }
            reverse
        })
    }

    pub fn lookup(&self, word: &str) -> &[(&'static str, &'static str)] {
        let Source::Fsa { dict, .. } = &self.source else {
            return self
                .materialized()
                .entries
                .get(word)
                .map(|v| &**v)
                .unwrap_or(&[]);
        };
        {
            let cache = dict.cache.read().unwrap();
            if let Some(hit) = cache.get(word) {
                return hit;
            }
        }
        let readings = dict.lookup_uncached(word);
        let leaked: &'static [(&'static str, &'static str)] = if readings.is_empty() {
            &[]
        } else {
            Box::leak(readings.into_boxed_slice())
        };
        let key: &'static str = Box::leak(word.to_string().into_boxed_str());
        let mut cache = dict.cache.write().unwrap();
        *cache.entry(key).or_insert(leaked)
    }

    pub fn len(&self) -> usize {
        self.materialized().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        match &self.source {
            Source::Fsa { dict, materialized } => {
                if let Some(m) = materialized.get() {
                    m.entries.is_empty()
                } else {
                    !dict.fsa.has_sequence()
                }
            }
            Source::Decoded(m) => m.entries.is_empty(),
        }
    }
}

pub struct SynthDictionary {
    fsa: Fsa,
    separator: u8,
}

impl SynthDictionary {
    pub fn load(dict_path: &Path, info_path: &Path) -> Option<Self> {
        let data: &'static [u8] = Box::leak(std::fs::read(dict_path).ok()?.into_boxed_slice());
        let fsa = Fsa::from_static(data)?;
        let info = std::fs::read_to_string(info_path).ok()?;
        let separator = info
            .lines()
            .find_map(|l| l.trim().strip_prefix("fsa.dict.separator="))?
            .trim()
            .as_bytes()
            .first()
            .copied()?;
        Some(Self { fsa, separator })
    }

    pub fn lookup(&self, key: &str) -> Vec<String> {
        let Some(node) = self.fsa.seek(key.as_bytes()) else {
            return Vec::new();
        };
        let mut encoded: Vec<Vec<u8>> = Vec::new();
        self.fsa
            .separator_suffixes(node, self.separator, &mut |seq: &[u8]| {
                encoded.push(seq.to_vec())
            });
        encoded
            .iter()
            .filter_map(|bytes| {
                let field = match bytes.iter().position(|b| *b == self.separator) {
                    Some(pos) => &bytes[..pos],
                    None => bytes.as_slice(),
                };
                decode_suffix(key.as_bytes(), field).and_then(|v| String::from_utf8(v).ok())
            })
            .collect()
    }
}

pub fn shared_synth_dictionary(
    resource_dir: &Path,
    dict_stem: &str,
) -> Option<std::sync::Arc<SynthDictionary>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<std::path::PathBuf, std::sync::Weak<SynthDictionary>>,
        >,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = resource_dir.join(format!("{dict_stem}.dict"));
    let mut guard = cache.lock().unwrap();
    if let Some(dict) = guard.get(&key).and_then(std::sync::Weak::upgrade) {
        return Some(dict);
    }
    let dictionary = std::sync::Arc::new(SynthDictionary::load(
        &key,
        &resource_dir.join(format!("{dict_stem}.info")),
    )?);
    guard.insert(key, std::sync::Arc::downgrade(&dictionary));
    Some(dictionary)
}

#[cfg(test)]
mod tests {
    #[test]
    fn arabic_synth_lookup_decodes_suffix_encoded_forms() {
        let dir = std::path::PathBuf::from(
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/ar/src/main/resources/org/languagetool/resource/ar",
        );
        let Some(dict) = SynthDictionary::load(
            &dir.join("arabic_synth.dict"),
            &dir.join("arabic_synth.info"),
        ) else {
            return;
        };
        let forms = dict.lookup("آب|NJ-;F1--;---");
        assert_eq!(
            forms,
            vec!["آبة".to_string()],
            "synth lookup must decode the suffix-encoded form"
        );
        assert!(dict.lookup("آب|NONEXISTENT-TAG").is_empty());
    }

    #[test]
    fn dump_obdachloser_sequences() {
        let dir = dict_path().expect("dict");
        let data: &'static [u8] = Box::leak(
            std::fs::read(dir.join("german.dict"))
                .unwrap()
                .into_boxed_slice(),
        );
        let fsa = Fsa::from_static(data).expect("fsa");
        fsa.for_each_sequence(&mut |seq: &[u8]| {
            if seq.starts_with(b"Obdachlos") {
                eprintln!("RAW: {}", String::from_utf8_lossy(seq));
            }
        });
    }

    use super::*;

    fn dict_path() -> Option<std::path::PathBuf> {
        [
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/resource/de",
            "languagetool/languagetool-language-modules/de/src/main/resources/org/languagetool/resource/de",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.join("german.dict").exists())
    }

    fn lang_dir(lang: &str, file: &str) -> Option<std::path::PathBuf> {
        [
            format!("/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/{lang}/src/main/resources/org/languagetool/resource/{lang}"),
            format!("languagetool/languagetool-language-modules/{lang}/src/main/resources/org/languagetool/resource/{lang}"),
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.join(file).exists())
    }

    fn cross_check_lazy_lookup_matches_enumeration(dir: &std::path::Path, stem: &str) {
        let dict = MorfologikDictionary::load(
            &dir.join(format!("{stem}.dict")),
            &dir.join(format!("{stem}.info")),
        )
        .unwrap_or_else(|| panic!("load {stem} dict"));
        let materialized = dict.materialized();
        assert!(
            !materialized.entries.is_empty(),
            "{stem} dictionary must enumerate"
        );
        for (word, readings) in materialized.entries.iter() {
            let lazy = dict.lookup(word);
            assert_eq!(
                lazy, &**readings,
                "{stem}: lazy lookup diverged for {word}: {lazy:?} vs {readings:?}"
            );
        }
        assert!(dict.lookup("__no_such_word_zq__").is_empty());
    }

    static DICT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn suffix_decoder_matches_java_examples() {
        assert_eq!(
            decode_suffix(b"foo", b"Abar").as_deref(),
            Some(b"foobar".as_slice())
        );
        assert_eq!(
            decode_suffix(b"foo", b"Dbar").as_deref(),
            Some(b"bar".as_slice())
        );
    }

    #[test]
    fn prefix_and_suffix_decoder_matches_java_examples() {
        assert_eq!(
            decode_prefix_and_suffix(b"abc", b"AAd").as_deref(),
            Some(b"abcd".as_slice())
        );
        assert_eq!(
            decode_prefix_and_suffix(b"abc", b"ADxyz").as_deref(),
            Some(b"xyz".as_slice())
        );
        assert_eq!(
            decode_prefix_and_suffix("kwietnia".as_bytes(), b"AEcie\xc5\x84".as_ref()).as_deref(),
            Some("kwiecień".as_bytes())
        );
    }

    #[test]
    fn from_decoded_text_builds_lemma_inversion() {
        let data = "friend\tfriend\tNN\nfriends\tfriend\tNNS\nfriendly\tfriendly\tJJ\n";
        let dict = MorfologikDictionary::from_decoded_text(data);
        assert_eq!(dict.lookup("friends").len(), 1);
        let forms = dict.forms_by_lemma();
        let friend = forms.get("friend").expect("lemma inversion");
        assert_eq!(friend.len(), 2);
        let joined: Vec<&str> = friend.iter().map(|(t, _)| *t).collect();
        assert!(joined.contains(&"NN") && joined.contains(&"NNS"));
        assert!(friend.iter().any(|(_, f)| *f == "friends"));
    }

    #[test]
    fn final_arcs_keep_their_continuations() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let dir = match dict_path() {
            Some(d) => d,
            None => return,
        };
        let dict = MorfologikDictionary::load(&dir.join("german.dict"), &dir.join("german.info"))
            .expect("load german dict");
        let readings = dict.lookup("Abgeordnete");
        assert!(
            readings
                .iter()
                .any(|(_, tag)| *tag == "SUB:NOM:SIN:MAS:ADJ"),
            "Abgeordnete must keep the :ADJ reading behind the plain-SUB prefix: {readings:?}"
        );
        assert!(
            readings
                .iter()
                .any(|(_, tag)| *tag == "SUB:NOM:SIN:FEM:ADJ"),
            "Abgeordnete must keep SUB:NOM:SIN:FEM:ADJ: {readings:?}"
        );
        assert_eq!(
            readings.len(),
            10,
            "Abgeordnete has 10 dict entries: {readings:?}"
        );
    }

    #[test]
    fn decodes_suffix_encoded_lemmas() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let dir = match dict_path() {
            Some(d) => d,
            None => return,
        };
        let dict = MorfologikDictionary::load(&dir.join("german.dict"), &dir.join("german.info"));
        let dict = dict.expect("load german dict");
        assert!(dict.len() > 100_000, "got {}", dict.len());

        let haus = dict.lookup("Haus");
        assert!(
            haus.iter().any(|(lemma, tag)| lemma == &"Haus"
                && tag.starts_with("SUB:")
                && tag.contains(":SIN:")
                && tag.ends_with(":NEU")),
            "Haus readings: {haus:?}"
        );

        let form = "Abgeordnetenhäusern";
        let got = dict.lookup(form);
        assert!(
            got.iter()
                .any(|(lemma, tag)| lemma == &"Abgeordnetenhaus" && tag.contains("DAT:PLU")),
            "{form} readings: {got:?}"
        );
    }

    #[test]
    fn lazy_lookup_matches_eager_enumeration_german() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let Some(dir) = dict_path() else {
            return;
        };
        cross_check_lazy_lookup_matches_enumeration(&dir, "german");
    }

    #[test]
    fn lazy_lookup_matches_eager_enumeration_french_fsa5() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let Some(dir) = lang_dir("fr", "french.dict") else {
            return;
        };
        cross_check_lazy_lookup_matches_enumeration(&dir, "french");
    }

    #[test]
    #[ignore]
    fn decodes_prefix_and_suffix_encoded_lemmas() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let dir = [
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/pl/src/main/resources/org/languagetool/resource/pl",
            "languagetool/languagetool-language-modules/pl/src/main/resources/org/languagetool/resource/pl",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.join("polish.dict").exists());
        let dir = match dir {
            Some(d) => d,
            None => return,
        };
        let dict = MorfologikDictionary::load(&dir.join("polish.dict"), &dir.join("polish.info"));
        let dict = dict.expect("load polish dict");

        let maja = dict.lookup("maja");
        assert!(
            maja.iter()
                .any(|(lemma, tag)| lemma == &"maj" && tag.starts_with("subst:")),
            "maja readings: {maja:?}"
        );

        let kwietnia = dict.lookup("kwietnia");
        assert!(
            kwietnia
                .iter()
                .any(|(lemma, tag)| lemma == &"kwiecień" && tag.starts_with("subst:")),
            "kwietnia readings: {kwietnia:?}"
        );

        let trwalo = dict.lookup("trwało");
        assert!(
            trwalo
                .iter()
                .any(|(lemma, tag)| lemma == &"trwać" && tag.starts_with("verb:")),
            "trwało readings: {trwalo:?}"
        );
    }

    #[test]
    #[ignore]
    fn decodes_koi8_r_suffix_encoded_lemmas() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let dir = [
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/ru/src/main/resources/org/languagetool/resource/ru",
            "languagetool/languagetool-language-modules/ru/src/main/resources/org/languagetool/resource/ru",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.join("russian.dict").exists());
        let dir = match dir {
            Some(d) => d,
            None => return,
        };
        let dict = MorfologikDictionary::load(&dir.join("russian.dict"), &dir.join("russian.info"));
        let dict = dict.expect("load russian dict");

        let imelos = dict.lookup("имелось");
        assert!(
            imelos
                .iter()
                .any(|(lemma, tag)| lemma == &"иметься" && tag.starts_with("VB:Past")),
            "имелось readings: {imelos:?}"
        );

        let kauril = dict.lookup("караулила");
        assert!(
            kauril
                .iter()
                .any(|(lemma, tag)| lemma == &"караулить" && tag.starts_with("VB:Past")),
            "караулила readings: {kauril:?}"
        );
    }

    #[test]
    #[ignore]
    fn lazy_lookup_matches_eager_enumeration_russian_koi8r() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let Some(dir) = lang_dir("ru", "russian.dict") else {
            return;
        };
        cross_check_lazy_lookup_matches_enumeration(&dir, "russian");
    }

    #[test]
    #[ignore]
    fn lazy_lookup_matches_eager_enumeration_italian_latin15() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let Some(dir) = lang_dir("it", "italian.dict") else {
            return;
        };
        cross_check_lazy_lookup_matches_enumeration(&dir, "italian");
    }

    #[test]
    #[ignore]
    fn lazy_lookup_matches_eager_enumeration_asturian_cfsa1() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let Some(dir) = lang_dir("ast", "asturian.dict") else {
            return;
        };
        cross_check_lazy_lookup_matches_enumeration(&dir, "asturian");
    }

    #[test]
    #[ignore]
    fn loads_cfsa1_version_cfsa_dict() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let dir = [
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/ast/src/main/resources/org/languagetool/resource/ast",
            "languagetool/languagetool-language-modules/ast/src/main/resources/org/languagetool/resource/ast",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.join("asturian.dict").exists());
        let dir = match dir {
            Some(d) => d,
            None => return,
        };
        let dict =
            MorfologikDictionary::load(&dir.join("asturian.dict"), &dir.join("asturian.info"));
        let dict = dict.expect("load asturian dict (CFSA version 0xc5)");

        assert!(!dict.is_empty(), "asturian dictionary must not be empty");

        let va = dict.lookup("va");
        assert!(
            va.iter()
                .any(|(lemma, tag)| lemma == &"dir" && tag.starts_with("VMIP")),
            "va readings: {va:?}"
        );

        let entamu = dict.lookup("entamu");
        assert!(
            entamu
                .iter()
                .any(|(lemma, tag)| lemma == &"entamu" && tag.starts_with("NCMS")),
            "entamu readings: {entamu:?}"
        );
    }

    #[test]
    #[ignore]
    fn slovak_v6_prefix_dict_lookups_match_slovak_tagger_test() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let dir = [
            "/home/agent/opengrammar-rs/languagetool/languagetool-language-modules/sk/src/main/resources/org/languagetool/resource/sk",
            "languagetool/languagetool-language-modules/sk/src/main/resources/org/languagetool/resource/sk",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.join("slovak.dict").exists());
        let dir = match dir {
            Some(d) => d,
            None => return,
        };
        let dict = MorfologikDictionary::load(&dir.join("slovak.dict"), &dir.join("slovak.info"));
        let dict = dict.expect("load slovak dict (CFSA version 0xc6, prefix encoder)");

        let tu = dict.lookup("tu");
        let tu_tags: Vec<&str> = tu.iter().map(|(_, t)| *t).collect();
        for tag in ["J", "PD", "T"] {
            assert!(tu_tags.contains(&tag), "tu tags: {tu:?}");
        }

        let najdete = dict.lookup("nájdete");
        assert!(
            najdete
                .iter()
                .any(|(lemma, tag)| lemma == &"nájsť" && *tag == "VKdpb+"),
            "nájdete readings: {najdete:?}"
        );

        let cisla = dict.lookup("čísla");
        assert!(
            cisla.iter().any(|(lemma, tag)| lemma == &"číslo"
                && ["SSnp1", "SSnp4", "SSnp5", "SSns2"].contains(tag)),
            "čísla readings: {cisla:?}"
        );

        let kultura = dict.lookup("kultúra");
        assert!(
            kultura
                .iter()
                .any(|(lemma, tag)| lemma == &"kultúra" && ["SSfs1", "SSfs5"].contains(tag)),
            "Kultúra readings: {kultura:?}"
        );
    }
    #[test]
    fn is_empty_answers_before_materialization_and_agrees_after() {
        let _guard = DICT_TEST_LOCK.lock().unwrap();
        let Some(dir) = dict_path() else {
            return;
        };
        let dict = MorfologikDictionary::load(&dir.join("german.dict"), &dir.join("german.info"))
            .expect("load german dict");
        assert!(!dict.is_empty());
        let len = dict.len();
        assert!(len > 100_000, "german dict len: {len}");
        assert!(!dict.is_empty());

        let empty = MorfologikDictionary::from_decoded_text("");
        assert!(empty.is_empty());
        let one = MorfologikDictionary::from_decoded_text("haus\thaus\tNN");
        assert!(!one.is_empty());
    }
}
