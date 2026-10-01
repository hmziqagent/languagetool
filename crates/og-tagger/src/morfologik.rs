use std::collections::{HashMap, HashSet};
use std::path::Path;

type DictionaryEntries = HashMap<Box<str>, Box<[(&'static str, &'static str)]>>;
type ReadingList = Vec<(&'static str, &'static str)>;

fn intern<'a>(cache: &mut HashSet<&'a str>, owned: String) -> &'a str {
    let leaked: &'a str = Box::leak(owned.into_boxed_str());
    if let Some(existing) = cache.get(leaked) {
        return existing;
    }
    cache.insert(leaked);
    leaked
}

struct Fsa5 {
    arcs: Vec<u8>,
    node_data_length: usize,
    gtl: usize,
}

const FSA5_BIT_FINAL_ARC: u8 = 1 << 0;
const FSA5_BIT_LAST_ARC: u8 = 1 << 1;
const FSA5_BIT_TARGET_NEXT: u8 = 1 << 2;
const FSA5_ADDRESS_OFFSET: usize = 1;

impl Fsa5 {
    fn from_bytes(data: &[u8]) -> Option<Self> {
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
            arcs: data[8..].to_vec(),
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

    fn sequences(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if let Some(root) = self.root_node() {
            let mut sequence: Vec<u8> = Vec::new();
            self.walk(root, &mut sequence, &mut out);
        }
        out
    }

    fn walk(&self, node: usize, sequence: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        let mut arc = Some(self.node_data_length + node);
        while let Some(current) = arc {
            let Some(&label) = self.arcs.get(current) else {
                break;
            };
            sequence.push(label);
            if self.is_arc_final(current) {
                out.push(sequence.clone());
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
}

pub struct MorfologikDictionary {
    entries: DictionaryEntries,
    verb_forms: std::sync::OnceLock<std::collections::HashMap<String, Vec<String>>>,
    forms_by_lemma: std::sync::OnceLock<
        std::collections::HashMap<&'static str, Vec<(&'static str, &'static str)>>,
    >,
}

struct Cfsa1 {
    arcs: Vec<u8>,
    node_data_length: usize,
    gtl: usize,
    label_mapping: Vec<u8>,
}

const CFSA1_BIT_FINAL_ARC: u8 = 1 << 0;
const CFSA1_BIT_LAST_ARC: u8 = 1 << 1;
const CFSA1_BIT_TARGET_NEXT: u8 = 1 << 2;

impl Cfsa1 {
    fn from_bytes(data: &[u8]) -> Option<Self> {
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
            arcs: data[40..].to_vec(),
            node_data_length,
            gtl,
            label_mapping: data.get(8..40)?.to_vec(),
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

    fn sequences(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        if let Some(root) = self.root_node() {
            let mut sequence: Vec<u8> = Vec::new();
            self.walk(root, &mut sequence, &mut out);
        }
        out
    }

    fn walk(&self, node: usize, sequence: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        let mut arc = Some(self.node_data_length + node);
        while let Some(current) = arc {
            let Some(&flag) = self.arcs.get(current) else {
                break;
            };
            let label = self.arc_label(current);
            sequence.push(label);
            if flag & CFSA1_BIT_FINAL_ARC != 0 {
                out.push(sequence.clone());
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
}

struct Cfsa2 {
    arcs: Vec<u8>,
    label_mapping: Vec<u8>,
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
    fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 || &data[0..4] != b"\\fsa" || data[4] != 0xc6 {
            return None;
        }
        let mut pos = 5;
        let flag_bits = u16::from_be_bytes([*data.get(pos)?, *data.get(pos + 1)?]);
        pos += 2;
        let has_numbers = flag_bits & (1 << 8) != 0;
        let label_mapping_size = *data.get(pos)? as usize;
        pos += 1;
        let label_mapping = data.get(pos..pos + label_mapping_size)?.to_vec();
        pos += label_mapping_size;
        Some(Self {
            arcs: data[pos..].to_vec(),
            label_mapping,
            has_numbers,
        })
    }

    fn first_arc(&self, node: usize) -> usize {
        if self.has_numbers {
            read_vint(&self.arcs, node).map_or(node, |(_, next)| next)
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
            read_vint(&self.arcs, start).map_or(0, |(value, _)| value)
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

    fn sequences(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut sequence: Vec<u8> = Vec::new();
        self.walk(self.root(), &mut sequence, &mut out);
        out
    }

    fn walk(&self, node: usize, sequence: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        let mut arc = Some(self.first_arc(node));
        while let Some(current) = arc {
            let label = self.arc_label(current);
            sequence.push(label);
            if self.is_arc_final(current) {
                out.push(sequence.clone());
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

impl MorfologikDictionary {
    pub fn load(dict_path: &Path, info_path: &Path) -> Option<Self> {
        let data = std::fs::read(dict_path).ok()?;
        let info = std::fs::read_to_string(info_path).ok()?;
        let separator = info
            .lines()
            .find_map(|l| l.trim().strip_prefix("fsa.dict.separator="))?
            .trim()
            .chars()
            .next()?;
        let encoder = info
            .lines()
            .find_map(|l| l.trim().strip_prefix("fsa.dict.encoder="))
            .map(|v| match v.trim().to_ascii_lowercase().as_str() {
                "suffix" => LemmaEncoder::Suffix,
                "prefix" => LemmaEncoder::PrefixAndSuffix,
                _ => LemmaEncoder::Raw,
            })
            .unwrap_or(LemmaEncoder::Raw);
        let encoding = info
            .lines()
            .find_map(|l| l.trim().strip_prefix("fsa.dict.encoding="))
            .map(str::trim)
            .unwrap_or("utf8")
            .to_string();
        let decode = |bytes: &[u8]| -> String {
            match encoding.as_str() {
                "koi8-r" => encoding_rs::KOI8_R.decode(bytes).0.into_owned(),
                "windows-1251" | "cp1251" => encoding_rs::WINDOWS_1251.decode(bytes).0.into_owned(),
                "iso-8859-1" | "iso8859-1" => {
                    encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned()
                }
                "iso-8859-15" | "iso8859-15" => {
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
                _ => String::from_utf8_lossy(bytes).into_owned(),
            }
        };

        enum AnyFsa {
            Cfsa2(Cfsa2),
            Fsa5(Fsa5),
            Cfsa1(Cfsa1),
        }
        let fsa = match Cfsa2::from_bytes(&data) {
            Some(f) => AnyFsa::Cfsa2(f),
            None => match Fsa5::from_bytes(&data) {
                Some(f) => AnyFsa::Fsa5(f),
                None => match Cfsa1::from_bytes(&data) {
                    Some(f) => AnyFsa::Cfsa1(f),
                    None => {
                        return None;
                    }
                },
            },
        };
        let sep = separator as u8;
        let mut interned: HashSet<&'static str> = HashSet::new();
        let mut map: HashMap<Box<str>, ReadingList> = HashMap::new();
        let sequences = match &fsa {
            AnyFsa::Cfsa2(f) => f.sequences(),
            AnyFsa::Fsa5(f) => f.sequences(),
            AnyFsa::Cfsa1(f) => f.sequences(),
        };
        for sequence in sequences {
            let mut fields = sequence.split(|b| *b == sep);
            let (Some(word_raw), Some(second), Some(tag_raw)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let lemma_bytes = match encoder {
                LemmaEncoder::Raw => second.to_vec(),
                LemmaEncoder::Suffix => match decode_suffix(word_raw, second) {
                    Some(decoded) => decoded,
                    None => continue,
                },
                LemmaEncoder::PrefixAndSuffix => match decode_prefix_and_suffix(word_raw, second) {
                    Some(decoded) => decoded,
                    None => continue,
                },
            };
            let word = decode(word_raw);
            let tag = intern(&mut interned, decode(tag_raw));
            let lemma = intern(&mut interned, decode(&lemma_bytes));
            map.entry(word.into_boxed_str())
                .or_default()
                .push((lemma, tag));
        }
        Some(Self {
            entries: map
                .into_iter()
                .map(|(word, readings)| (word, readings.into_boxed_slice()))
                .collect(),
            verb_forms: std::sync::OnceLock::new(),
            forms_by_lemma: std::sync::OnceLock::new(),
        })
    }

    pub fn word_list(&self) -> Vec<String> {
        self.entries.keys().map(|k| k.to_string()).collect()
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(|k| &**k)
    }

    pub fn forms_by_lemma(
        &self,
    ) -> &std::collections::HashMap<&'static str, Vec<(&'static str, &'static str)>> {
        self.forms_by_lemma.get_or_init(|| {
            let mut reverse: std::collections::HashMap<
                &'static str,
                Vec<(&'static str, &'static str)>,
            > = std::collections::HashMap::new();
            for (word, readings) in self.entries.iter() {
                for (lemma, tag) in readings.iter() {
                    let leaked: &'static str = Box::leak(word.clone());
                    reverse.entry(*lemma).or_default().push((*tag, leaked));
                }
            }
            reverse
        })
    }

    pub fn verb_forms_by_lemma(&self) -> &std::collections::HashMap<String, Vec<String>> {
        self.verb_forms.get_or_init(|| {
            let mut reverse: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for (word, readings) in self.entries.iter() {
                for (lemma, tag) in readings.iter() {
                    if tag.starts_with("VER:") {
                        let entry = reverse.entry(lemma.to_string()).or_default();
                        if !entry.iter().any(|w| w.as_str() == &**word) {
                            entry.push(word.to_string());
                        }
                    }
                }
            }
            reverse
        })
    }

    pub fn lookup(&self, word: &str) -> &[(&'static str, &'static str)] {
        self.entries.get(word).map(|v| &**v).unwrap_or(&[])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub struct SynthDictionary {
    fsa: Cfsa2,
    separator: u8,
}

impl SynthDictionary {
    pub fn load(dict_path: &Path, info_path: &Path) -> Option<Self> {
        let data = std::fs::read(dict_path).ok()?;
        let fsa = Cfsa2::from_bytes(&data)?;
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
        let Some(node) = self.fsa.seek(self.fsa.root(), key.as_bytes()) else {
            return Vec::new();
        };
        let mut encoded = Vec::new();
        let mut arc = Some(self.fsa.first_arc(node));
        while let Some(current) = arc {
            if self.fsa.arc_label(current) == self.separator {
                let dest = self.fsa.destination(current);
                if dest != 0 {
                    let mut sequence = Vec::new();
                    self.fsa.walk(dest, &mut sequence, &mut encoded);
                }
            }
            arc = self.fsa.next_arc(current);
        }
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
        let data = std::fs::read(dir.join("german.dict")).unwrap();
        let fsa = Cfsa2::from_bytes(&data).expect("cfsa2");
        for s in fsa
            .sequences()
            .iter()
            .filter(|s| s.starts_with(b"Obdachlos"))
        {
            eprintln!("RAW: {}", String::from_utf8_lossy(s));
        }
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
}
