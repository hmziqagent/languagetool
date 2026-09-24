use std::collections::HashMap;
use std::path::Path;

type DictionaryEntries = HashMap<Box<str>, Box<[(Box<str>, Box<str>)]>>;
type ReadingList = Vec<(Box<str>, Box<str>)>;

pub struct MorfologikDictionary {
    entries: DictionaryEntries,
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
            if self.arcs[current] & 0x20 != 0 {
                sequence.push(label);
                out.push(sequence.clone());
                sequence.pop();
            } else {
                sequence.push(label);
                self.walk(self.destination(current), sequence, out);
                sequence.pop();
            }
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
        let encoder_suffix = info.lines().any(|l| l.contains("encoder=SUFFIX"));

        let fsa = match Cfsa2::from_bytes(&data) {
            Some(f) => f,
            None => {
                return None;
            }
        };
        let sep = separator as u8;
        let mut map: HashMap<Box<str>, ReadingList> = HashMap::new();
        let sequences = fsa.sequences();
        for _s in sequences.iter().take(3) {}
        for sequence in sequences {
            let mut fields = sequence.split(|b| *b == sep);
            let (Some(word), Some(second), Some(tag)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let word = String::from_utf8_lossy(word).into_owned();
            let tag = String::from_utf8_lossy(tag).into_owned();
            let lemma = if encoder_suffix {
                match decode_suffix(word.as_bytes(), second) {
                    Some(decoded) => String::from_utf8_lossy(&decoded).into_owned(),
                    None => continue,
                }
            } else {
                String::from_utf8_lossy(second).into_owned()
            };
            map.entry(word.into_boxed_str())
                .or_default()
                .push((lemma.into_boxed_str(), tag.into_boxed_str()));
        }
        Some(Self {
            entries: map
                .into_iter()
                .map(|(word, readings)| (word, readings.into_boxed_slice()))
                .collect(),
        })
    }

    pub fn word_list(&self) -> Vec<String> {
        self.entries.keys().map(|k| k.to_string()).collect()
    }

    pub fn lookup(&self, word: &str) -> &[(Box<str>, Box<str>)] {
        self.entries.get(word).map(|v| &**v).unwrap_or(&[])
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn decodes_suffix_encoded_lemmas() {
        let dir = match dict_path() {
            Some(d) => d,
            None => return,
        };
        let dict = MorfologikDictionary::load(&dir.join("german.dict"), &dir.join("german.info"));
        let dict = dict.expect("load german dict");
        assert!(dict.len() > 100_000, "got {}", dict.len());

        let haus = dict.lookup("Haus");
        assert!(
            haus.iter().any(|(lemma, tag)| lemma.as_ref() == "Haus"
                && tag.starts_with("SUB:")
                && tag.contains(":SIN:")
                && tag.ends_with(":NEU")),
            "Haus readings: {haus:?}"
        );

        let form = "Abgeordnetenhäusern";
        let got = dict.lookup(form);
        assert!(
            got.iter()
                .any(|(lemma, tag)| lemma.as_ref() == "Abgeordnetenhaus" && tag.contains("DAT:PLU")),
            "{form} readings: {got:?}"
        );
    }
}
