use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

const BUFFER_CAPACITY: usize = 1 << 16;

struct Cursor<R: Read + Seek> {
    reader: R,
    buf: Vec<u8>,
    pos: usize,
    base: u64,
}

impl<R: Read + Seek> Cursor<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buf: vec![0; BUFFER_CAPACITY],
            pos: BUFFER_CAPACITY,
            base: 0,
        }
    }

    fn file_pos(&self) -> u64 {
        self.base + self.pos as u64
    }

    fn ensure(&mut self, n: usize) -> Option<()> {
        if self.pos + n <= self.buf.len() {
            return Some(());
        }
        let keep = self.buf.len() - self.pos;
        self.buf.copy_within(self.pos.., 0);
        self.base += self.pos as u64;
        self.pos = 0;
        self.buf.resize(BUFFER_CAPACITY, 0);
        let mut filled = keep;
        while filled < self.buf.len() {
            let read = self.reader.read(&mut self.buf[filled..]).ok()?;
            if read == 0 {
                self.buf.truncate(filled);
                break;
            }
            filled += read;
        }
        if self.pos + n <= self.buf.len() {
            Some(())
        } else {
            None
        }
    }

    fn u8(&mut self) -> Option<u8> {
        self.ensure(1)?;
        let b = self.buf[self.pos];
        self.pos += 1;
        Some(b)
    }

    fn bytes(&mut self, out: &mut Vec<u8>, n: usize) -> Option<()> {
        self.ensure(n)?;
        out.extend_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Some(())
    }

    fn vint(&mut self) -> Option<u64> {
        let mut shift = 0;
        let mut value = 0u64;
        loop {
            let b = self.u8()?;
            value |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(value);
            }
            shift += 7;
        }
    }

    fn vlong(&mut self) -> Option<u64> {
        self.vint()
    }

    fn seek(&mut self, target: u64) -> Option<()> {
        if target >= self.base && (target - self.base) as usize <= self.buf.len() {
            self.pos = (target - self.base) as usize;
            return Some(());
        }
        self.reader.seek(SeekFrom::Start(target)).ok()?;
        self.base = target;
        self.buf.clear();
        self.pos = 0;
        Some(())
    }

    fn peek_magic(&mut self) -> Option<[u8; 4]> {
        self.ensure(4)?;
        let mut out = [0u8; 4];
        out.copy_from_slice(&self.buf[self.pos..self.pos + 4]);
        Some(out)
    }
}

fn skip_header_5x<R: Read + Seek>(cur: &mut Cursor<R>) -> Option<(u32, String)> {
    if cur.peek_magic()? != [0x3f, 0xd7, 0x6c, 0x17] {
        return None;
    }
    for _ in 0..4 {
        cur.u8()?;
    }
    let name = read_string_generic(cur)?;
    let mut scratch = Vec::new();
    cur.bytes(&mut scratch, 4);
    let version = u32::from_le_bytes([scratch[0], scratch[1], scratch[2], scratch[3]]);
    scratch.clear();
    cur.bytes(&mut scratch, 16);
    let suffix_len = cur.u8()? as usize;
    cur.bytes(&mut scratch, suffix_len);
    Some((version, name))
}

fn read_string_generic<R: Read + Seek>(cur: &mut Cursor<R>) -> Option<String> {
    let len = cur.vint()? as usize;
    let mut out = Vec::new();
    cur.bytes(&mut out, len)?;
    String::from_utf8(out).ok()
}

fn skip_header_4x<R: Read + Seek>(cur: &mut Cursor<R>) -> Option<(u32, String)> {
    if cur.peek_magic()? != [0x3f, 0xd7, 0x6c, 0x17] {
        return None;
    }
    for _ in 0..4 {
        cur.u8()?;
    }
    let name = read_string_generic(cur)?;
    let mut scratch = Vec::new();
    cur.bytes(&mut scratch, 4);
    let version = u32::from_le_bytes([scratch[0], scratch[1], scratch[2], scratch[3]]);
    Some((version, name))
}

#[derive(Clone, Copy)]
enum TermsFlavor {
    Lucene5,
    Lucene4,
}

fn footer_length(_flavor: TermsFlavor) -> u64 {
    16
}

fn read_codec_name<R: Read + Seek>(cur: &mut Cursor<R>) -> Option<String> {
    if cur.peek_magic()? != [0x3f, 0xd7, 0x6c, 0x17] {
        return None;
    }
    for _ in 0..4 {
        cur.u8()?;
    }
    let name = read_string_generic(cur)?;
    cur.seek(0)?;
    Some(name)
}

fn read_string<R: Read + Seek>(cur: &mut Cursor<R>) -> Option<String> {
    let len = cur.vint()? as usize;
    let mut out = Vec::new();
    cur.bytes(&mut out, len)?;
    String::from_utf8(out).ok()
}

fn parse_fnm(path: &PathBuf) -> Option<HashMap<String, usize>> {
    fn parse_at(path: &PathBuf, four_x: bool) -> Option<HashMap<String, usize>> {
        let file = File::open(path).ok()?;
        let mut cur = Cursor::new(BufReader::new(file));
        if four_x {
            skip_header_4x(&mut cur)?;
        } else {
            skip_header_5x(&mut cur)?;
        }
        let size = cur.vint()? as usize;
        let mut fields = HashMap::new();
        for _ in 0..size {
            let name = read_string(&mut cur)?;
            let field_number = cur.vint()? as usize;
            cur.u8()?;
            cur.u8()?;
            let mut scratch = Vec::new();
            cur.bytes(&mut scratch, 8);
            if four_x {
                cur.bytes(&mut scratch, 3);
            }
            let attr_count = cur.vint()? as usize;
            for _ in 0..attr_count {
                read_string(&mut cur)?;
                read_string(&mut cur)?;
            }
            fields.insert(name, field_number);
        }
        Some(fields)
    }

    let as5 = parse_at(path, false);
    if as5
        .as_ref()
        .is_some_and(|f| f.contains_key("ngram") && f.contains_key("count"))
    {
        return as5;
    }
    parse_at(path, true)
}

struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn vlong(&mut self) -> Option<u64> {
        let mut shift = 0;
        let mut value = 0u64;
        loop {
            let b = self.u8()?;
            value |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return Some(value);
            }
            shift += 7;
        }
    }

    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.data.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(out)
    }
}

fn parse_fdt(
    path: &PathBuf,
    count_field: u32,
    total_field: Option<u32>,
) -> Option<(Vec<u64>, u64)> {
    let file = File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let mut cur = Cursor::new(BufReader::with_capacity(1 << 20, file));
    let codec = read_codec_name(&mut cur)?;
    let four_x = codec.contains("41");
    let mut chunk_size: usize = 0;
    cur.seek(0)?;
    if four_x {
        skip_header_4x(&mut cur)?;
        chunk_size = cur.vint()? as usize;
        cur.vint()?;
    } else {
        let v = skip_header_5x(&mut cur)?.0;
        if v >= 1 {
            cur.vint()?;
            cur.vint()?;
        }
    }
    let mut doc_counts: Vec<u64> = Vec::new();
    let mut total_token_count: u64 = 0;
    while cur.file_pos() + 16 < file_len {
        if cur.peek_magic()? == [0xc0, 0x28, 0x93, 0xe8] {
            break;
        }
        let _doc_base = cur.vint()?;
        let (chunk_docs, pre_sliced) = if four_x {
            (cur.vint()? as usize, false)
        } else {
            let token = cur.vint()?;
            ((token >> 1) as usize, (token & 1) != 0)
        };
        let num_stored_fields: Vec<u64>;
        let mut offsets: Vec<u64> = vec![0];
        if chunk_docs == 1 {
            num_stored_fields = vec![cur.vint()?];
            offsets.push(cur.vint()?);
        } else {
            let bits = cur.vint()? as u8;
            if bits == 0 {
                num_stored_fields = vec![cur.vint()?; chunk_docs];
            } else {
                let count = chunk_docs;
                let mut values = Vec::with_capacity(count);
                let mut buffer: u64 = 0;
                let mut buffered: u32 = 0;
                for _ in 0..count {
                    while buffered < bits as u32 {
                        let b = cur.u8()?;
                        buffer = (buffer << 8) | b as u64;
                        buffered += 8;
                    }
                    values.push(buffer >> (buffered - bits as u32));
                    buffered -= bits as u32;
                    buffer &= (1u64 << buffered).saturating_sub(1);
                }
                num_stored_fields = values;
            }
            let bits_len = cur.vint()? as u8;
            if bits_len == 0 {
                let length = cur.vint()?;
                for i in 0..chunk_docs {
                    offsets.push((1 + i) as u64 * length);
                }
            } else {
                let mut values = Vec::with_capacity(chunk_docs);
                let mut buffer: u64 = 0;
                let mut buffered: u32 = 0;
                for _ in 0..chunk_docs {
                    while buffered < bits_len as u32 {
                        let b = cur.u8()?;
                        buffer = (buffer << 8) | b as u64;
                        buffered += 8;
                    }
                    values.push(buffer >> (buffered - bits_len as u32));
                    buffered -= bits_len as u32;
                    buffer &= (1u64 << buffered).saturating_sub(1);
                }
                for i in 0..chunk_docs {
                    offsets.push(offsets[i] + values[i]);
                }
            }
        }
        let _ = num_stored_fields;
        let total_len = *offsets.last()? as usize;
        let sliced = pre_sliced || (four_x && chunk_size > 0 && total_len >= 2 * chunk_size);
        let compressed = if sliced {
            let mut decompressed = Vec::with_capacity(total_len);
            let mut remaining = total_len;
            while remaining > 0 {
                let chunk = remaining.min(61440);
                let block = decompress_block(&mut cur, chunk)?;
                decompressed.extend_from_slice(&block);
                remaining -= chunk;
            }
            decompressed
        } else {
            decompress_block(&mut cur, total_len)?
        };
        for (i, _) in offsets.iter().take(offsets.len() - 1).enumerate() {
            let start = offsets[i] as usize;
            let end = offsets[i + 1] as usize;
            let mut reader = ByteReader {
                data: &compressed[start..end],
                pos: 0,
            };
            for _ in 0..num_stored_fields[i] {
                let info_and_bits = reader.vlong()?;
                let field_number = (info_and_bits >> 3) as u32;
                let bits = (info_and_bits & 7) as u8;
                let mut numeric: Option<u64> = None;
                match bits {
                    0x00 => {
                        let len = reader.vlong()? as usize;
                        let text = reader.bytes(len)?;
                        let text = String::from_utf8_lossy(text).into_owned();
                        if let Ok(count) = text.parse::<u64>() {
                            if field_number == count_field {
                                numeric = Some(count);
                            } else if Some(field_number) == total_field {
                                total_token_count += count;
                            }
                        }
                    }
                    0x01 => {
                        let len = reader.vlong()? as usize;
                        reader.bytes(len)?;
                    }
                    0x02 => {
                        let zig = reader.vlong()?;
                        numeric = Some(((zig >> 1) as u64).wrapping_sub(zig & 1));
                    }
                    0x03 => {
                        let raw = reader.bytes(4)?;
                        numeric = Some(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as u64);
                    }
                    0x04 => {
                        let raw = reader.bytes(8)?;
                        numeric = Some(u64::from_be_bytes([
                            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
                        ]));
                    }
                    0x05 => {
                        let raw = reader.bytes(8)?;
                        numeric = Some(f64::from_le_bytes([
                            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
                        ]) as u64);
                    }
                    _ => {
                        reader.bytes(8);
                    }
                }
                if let Some(count) = numeric
                    && field_number == count_field
                {
                    let doc_id = _doc_base as usize + i;
                    if doc_id >= doc_counts.len() {
                        doc_counts.resize(doc_id + 1, 0);
                    }
                    doc_counts[doc_id] = count;
                }
            }
        }
    }
    Some((doc_counts, total_token_count))
}

fn decompress_block<R: Read + Seek>(cur: &mut Cursor<R>, dest_len: usize) -> Option<Vec<u8>> {
    let mut compressed = Vec::with_capacity(dest_len + dest_len / 8 + 64);
    let mut out: Vec<u8> = Vec::with_capacity(dest_len);
    loop {
        let mut token = [0u8; 1];
        let start = cur.file_pos();
        token[0] = cur.u8()?;
        let mut literal_len = (token[0] >> 4) as usize;
        compressed.push(token[0]);
        if literal_len == 0x0f {
            loop {
                let b = cur.u8()?;
                compressed.push(b);
                literal_len += b as usize;
                if b != 0xff {
                    break;
                }
            }
        }
        for _ in 0..literal_len {
            let b = cur.u8()?;
            compressed.push(b);
            out.push(b);
        }
        if out.len() >= dest_len {
            break;
        }
        let mut offset_bytes = [0u8; 2];
        offset_bytes[0] = cur.u8()?;
        offset_bytes[1] = cur.u8()?;
        compressed.extend_from_slice(&offset_bytes);
        let offset = u16::from_le_bytes(offset_bytes) as usize;
        let mut match_len = (token[0] & 0x0f) as usize;
        if match_len == 0x0f {
            loop {
                let b = cur.u8()?;
                compressed.push(b);
                match_len += b as usize;
                if b != 0xff {
                    break;
                }
            }
        }
        match_len += 4;
        let start_pos = out.len().checked_sub(offset)?;
        for k in 0..match_len {
            let b = out[start_pos + k];
            out.push(b);
        }
        let _ = compressed;
        let _ = start;
    }
    Some(out)
}

#[allow(clippy::too_many_arguments)]
fn dump_terms(
    tim_path: &PathBuf,
    doc_counts: &[u64],
    index_options_docs_only: bool,
    out_path: &PathBuf,
) -> Option<(usize, u64)> {
    let file = File::open(tim_path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let mut cur = Cursor::new(BufReader::with_capacity(1 << 20, file));
    let (flavor, old_version) = if read_codec_name(&mut cur)?.to_lowercase().contains("dict") {
        let (v, _) = skip_header_4x(&mut cur)?;
        skip_header_4x(&mut cur)?;
        cur.vint()?;
        (TermsFlavor::Lucene4, v)
    } else {
        skip_header_5x(&mut cur)?;
        skip_header_5x(&mut cur)?;
        cur.vint()?;
        (TermsFlavor::Lucene5, 0u32)
    };
    let auto_prefix = matches!(flavor, TermsFlavor::Lucene5);
    cur.seek(file_len - footer_length(flavor) - 8)?;
    let dir_offset = {
        let mut b = [0u8; 8];
        let mut scratch = Vec::new();
        cur.bytes(&mut scratch, 8);
        b.copy_from_slice(&scratch);
        u64::from_be_bytes(b)
    };
    cur.seek(dir_offset)?;
    let num_fields = cur.vint()? as usize;
    eprintln!("numFields: {num_fields} at dir_offset {dir_offset}");
    let mut target_field: Option<(u64, Vec<u8>, u64, usize)> = None;
    for _ in 0..num_fields {
        let _field_number = cur.vint()?;
        let _num_terms = cur.vlong()?;
        let root_len = cur.vint()? as usize;
        let mut root_code = Vec::new();
        cur.bytes(&mut root_code, root_len)?;
        if !index_options_docs_only {
            cur.vlong()?;
        }
        let sum_doc_freq = cur.vlong()?;
        let _doc_count = cur.vint()?;
        let longs_size = cur.vint()? as usize;
        let mut scratch = Vec::new();
        if !matches!(flavor, TermsFlavor::Lucene4) || old_version >= 4 {
            let min_len = cur.vint()? as usize;
            cur.bytes(&mut scratch, min_len);
            let max_len = cur.vint()? as usize;
            cur.bytes(&mut scratch, max_len);
        }
        eprintln!(
            "field {_field_number} numTerms {_num_terms} sumDocFreq {sum_doc_freq} longsSize {longs_size} rootCode {root_code:02x?}"
        );
        if _field_number == 0 {
            target_field = Some((sum_doc_freq, root_code, 0, longs_size));
        }
    }
    let (sum_doc_freq, root_code, _, longs_size) = target_field?;
    let mut root_reader = ByteReader {
        data: &root_code,
        pos: 0,
    };
    let code = root_reader.vlong()?;
    let root_fp = code >> 2;
    let is_floor = (code & 1) != 0;
    eprintln!("root fp: {root_fp} isFloor: {is_floor}");

    let sink = std::fs::File::create(out_path).ok()?;
    let mut out = std::io::BufWriter::with_capacity(1 << 20, sink);
    let mut term_count: u64 = 0;
    let mut sum_doc_freq_seen: u64 = 0;
    let mut multi_doc_terms: u64 = 0;
    walk_blocks(
        &mut cur,
        root_fp,
        Vec::new(),
        doc_counts,
        longs_size,
        auto_prefix,
        &mut out,
        &mut term_count,
        &mut sum_doc_freq_seen,
        &mut multi_doc_terms,
    );
    eprintln!(
        "terms: {term_count}, multi-doc terms skipped: {multi_doc_terms}, sumDocFreq seen: {sum_doc_freq_seen}, expected: {sum_doc_freq}"
    );
    let _ = &mut out;
    out.flush().ok();
    Some((term_count as usize, sum_doc_freq))
}

#[allow(clippy::too_many_arguments)]
fn walk_blocks<R: Read + Seek>(
    cur: &mut Cursor<R>,
    fp: u64,
    prefix: Vec<u8>,
    doc_counts: &[u64],
    longs_size: usize,
    auto_prefix: bool,
    out: &mut impl Write,
    term_count: &mut u64,
    sum_doc_freq_seen: &mut u64,
    multi_doc_terms: &mut u64,
) -> Option<()> {
    let mut current_fp = fp;
    let current_prefix = prefix;
    'floor: loop {
        cur.seek(current_fp)?;
        let code = cur.vint()?;
        let ent_count = (code >> 1) as usize;
        let is_last_in_floor = (code & 1) != 0;
        let code2 = cur.vint()?;
        let is_leaf_block = (code2 & 1) != 0;
        let suffix_bytes_len = (code2 >> 1) as usize;
        let mut suffix_bytes = Vec::with_capacity(suffix_bytes_len);
        cur.bytes(&mut suffix_bytes, suffix_bytes_len)?;
        let stats_len = cur.vint()? as usize;
        let mut stats_bytes = Vec::with_capacity(stats_len);
        cur.bytes(&mut stats_bytes, stats_len)?;
        let meta_len = cur.vint()? as usize;
        let mut meta_bytes = Vec::with_capacity(meta_len);
        cur.bytes(&mut meta_bytes, meta_len)?;
        let fp_end = cur.file_pos();

        let mut suffix_reader = ByteReader {
            data: &suffix_bytes,
            pos: 0,
        };
        let mut stats_reader = ByteReader {
            data: &stats_bytes,
            pos: 0,
        };
        let mut meta_reader = ByteReader {
            data: &meta_bytes,
            pos: 0,
        };

        for _ in 0..ent_count {
            let (suffix, kind) = if is_leaf_block {
                let len = suffix_reader.vlong()? as usize;
                let suffix = suffix_reader.bytes(len)?.to_vec();
                (suffix, 0u8)
            } else {
                let entry_code = suffix_reader.vlong()?;
                if auto_prefix {
                    if entry_code & 1 != 0 {
                        let len = (entry_code >> 2) as usize;
                        let suffix = suffix_reader.bytes(len)?.to_vec();
                        (suffix, 1u8)
                    } else {
                        let len = (entry_code >> 2) as usize;
                        let suffix = suffix_reader.bytes(len)?.to_vec();
                        let kind = (entry_code & 3) as u8;
                        if kind >= 2 {
                            suffix_reader.u8()?;
                        }
                        (suffix, kind)
                    }
                } else if entry_code & 1 != 0 {
                    let len = (entry_code >> 1) as usize;
                    let suffix = suffix_reader.bytes(len)?.to_vec();
                    (suffix, 1u8)
                } else {
                    let len = (entry_code >> 1) as usize;
                    let suffix = suffix_reader.bytes(len)?.to_vec();
                    (suffix, 0u8)
                }
            };
            if kind == 1 {
                let sub_code = {
                    let mut reader = ByteReader {
                        data: &suffix_bytes,
                        pos: suffix_reader.pos,
                    };
                    let sub = reader.vlong()?;
                    suffix_reader.pos = reader.pos;
                    sub
                };
                let sub_fp = current_fp.checked_sub(sub_code)?;
                let mut child_prefix = current_prefix.clone();
                child_prefix.extend_from_slice(&suffix);
                walk_blocks(
                    cur,
                    sub_fp,
                    child_prefix,
                    doc_counts,
                    longs_size,
                    auto_prefix,
                    out,
                    term_count,
                    sum_doc_freq_seen,
                    multi_doc_terms,
                )?;
            } else {
                let doc_freq = stats_reader.vlong()?;
                *sum_doc_freq_seen += doc_freq;
                for _ in 0..longs_size {
                    meta_reader.vlong()?;
                }
                if doc_freq == 1 {
                    let singleton = meta_reader.vlong()?;
                    if kind == 0 {
                        let mut term = current_prefix.clone();
                        term.extend_from_slice(&suffix);
                        let count = doc_counts.get(singleton as usize).copied().unwrap_or(0);
                        let text = String::from_utf8_lossy(&term).into_owned();
                        writeln!(out, "{text}\t{count}").ok();
                        *term_count += 1;
                    }
                } else {
                    *multi_doc_terms += 1;
                }
            }
        }
        if is_last_in_floor {
            break 'floor;
        }
        cur.seek(fp_end)?;
        current_fp = fp_end;
    }
    Some(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!(
            "usage: lucene_dump <ngram-dir> (>Ngrams.tsv per n on stdout not supported; writes <dir>/<n>grams.tsv)"
        );
        std::process::exit(1);
    }
    let dir = PathBuf::from(&args[1]);
    let only: Option<usize> = args.get(2).and_then(|a| a.parse().ok());
    for n in 1..=3usize {
        if only.is_some_and(|o| o != n) {
            continue;
        }
        let grams_dir = dir.join(format!("{n}grams"));
        let entries: Vec<PathBuf> = std::fs::read_dir(&grams_dir)
            .ok()
            .map(|rd| {
                rd.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| {
                        p.extension().is_some_and(|e| e == "tim")
                            && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                                n.ends_with("_Lucene50_0.tim") || n.ends_with("_Lucene41_0.tim")
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if entries.is_empty() {
            eprintln!("no .tim in {}", grams_dir.display());
            continue;
        }
        let mut segs: Vec<PathBuf> = entries;
        segs.sort();
        let out_path = dir.join(format!("{n}grams.tsv"));
        let sink = std::fs::File::create(&out_path).ok();
        let mut grand_total: u64 = 0;
        let mut grand_terms: u64 = 0;
        for tim_path in &segs {
            let seg_prefix = tim_path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| {
                    s.strip_suffix("_Lucene50_0")
                        .or_else(|| s.strip_suffix("_Lucene41_0"))
                })
                .unwrap_or_default()
                .to_string();
            let fdt_path = grams_dir.join(format!("{seg_prefix}.fdt"));
            let fnm_path = grams_dir.join(format!("{seg_prefix}.fnm"));
            let fields = parse_fnm(&fnm_path).unwrap_or_default();
            let count_field = fields.get("count").copied().unwrap_or(1) as u32;
            let total_field = fields.get("totalTokenCount").copied().map(|f| f as u32);
            let (doc_counts, total) =
                parse_fdt(&fdt_path, count_field, total_field).unwrap_or_default();
            grand_total += total;
            eprintln!(
                "{n}grams/{seg_prefix}: docs {} totalTokenCount {total}",
                doc_counts.len()
            );
            if sink.is_none() {
                continue;
            }
            let seg_out = if segs.len() == 1 {
                out_path.clone()
            } else {
                dir.join(format!("{n}grams.{seg_prefix}.part"))
            };
            if let Some((terms, _)) = dump_terms(tim_path, &doc_counts, true, &seg_out) {
                grand_terms += terms as u64;
            }
        }
        if segs.len() > 1 && sink.is_some() {
            let mut merged = std::io::BufWriter::with_capacity(
                1 << 20,
                std::fs::File::create(&out_path).ok().unwrap(),
            );
            for part in &segs {
                let seg_prefix = part
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| {
                        s.strip_suffix("_Lucene50_0")
                            .or_else(|| s.strip_suffix("_Lucene41_0"))
                    })
                    .unwrap_or_default()
                    .to_string();
                let part_path = dir.join(format!("{n}grams.{seg_prefix}.part"));
                if let Ok(bytes) = std::fs::read(&part_path) {
                    use std::io::Write;
                    merged.write_all(&bytes).ok();
                }
                let _ = std::fs::remove_file(&part_path);
            }
            use std::io::Write;
            merged.flush().ok();
        }
        if grand_total == 0
            && let Ok(one) = std::fs::read_to_string(dir.join("1grams.meta"))
            && let Some(v) = one
                .lines()
                .find_map(|l| l.strip_prefix("total_token_count\t"))
                .and_then(|v| v.trim().parse::<u64>().ok())
        {
            grand_total = v;
        }
        eprintln!("{n}grams merged: {grand_terms} terms, totalTokenCount {grand_total}");
        eprintln!("wrote {}", out_path.display());
        std::fs::write(
            dir.join(format!("{n}grams.meta")),
            format!("total_token_count\t{grand_total}\n"),
        )
        .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut b = (v & 0x7f) as u8;
            v >>= 7;
            if v != 0 {
                b |= 0x80;
            }
            out.push(b);
            if v == 0 {
                return out;
            }
        }
    }

    fn vstr(s: &str) -> Vec<u8> {
        let mut out = vint(s.len() as u64);
        out.extend_from_slice(s.as_bytes());
        out
    }

    fn cursor_over(data: Vec<u8>) -> Cursor<std::io::Cursor<Vec<u8>>> {
        let mut cur = Cursor::new(std::io::Cursor::new(data));
        cur.u8();
        cur.seek(0);
        cur
    }

    #[test]
    fn cursor_parses_vints_bytes_and_tracks_file_pos() {
        let mut data = Vec::new();
        data.push(0xaa);
        data.extend_from_slice(&vint(0));
        data.extend_from_slice(&vint(127));
        data.extend_from_slice(&vint(128));
        data.extend_from_slice(&vint(300));
        data.extend_from_slice(&vint(16383));
        data.extend_from_slice(&vint(16384));
        data.extend_from_slice(&vint(u64::MAX));
        data.extend_from_slice(b"tail");
        let mut cur = cursor_over(data);
        assert_eq!(cur.file_pos(), 0);
        assert_eq!(cur.u8(), Some(0xaa));
        assert_eq!(cur.vint(), Some(0));
        assert_eq!(cur.vint(), Some(127));
        assert_eq!(cur.vint(), Some(128));
        assert_eq!(cur.vint(), Some(300));
        assert_eq!(cur.vint(), Some(16383));
        assert_eq!(cur.vint(), Some(16384));
        assert_eq!(cur.vlong(), Some(u64::MAX));
        let mut out = Vec::new();
        cur.bytes(&mut out, 4);
        assert_eq!(out, b"tail");
        assert_eq!(cur.file_pos(), 26);
        assert_eq!(cur.u8(), None);
    }

    #[test]
    fn cursor_refills_across_the_buffer_boundary() {
        let mut data = vec![0x5a; BUFFER_CAPACITY - 1];
        data.push(0xac);
        data.push(0x02);
        data.extend_from_slice(&vint(1 << 20));
        let mut cur = cursor_over(data.clone());
        let mut out = Vec::new();
        cur.bytes(&mut out, BUFFER_CAPACITY - 1);
        assert_eq!(out, vec![0x5a; BUFFER_CAPACITY - 1]);
        assert_eq!(cur.vint(), Some(300));
        assert_eq!(cur.vlong(), Some(1 << 20));
        assert_eq!(cur.file_pos() as usize, data.len());
        assert_eq!(cur.vint(), None);
    }

    #[test]
    fn cursor_rejects_reads_larger_than_the_buffer() {
        let mut cur = cursor_over(vec![1u8; 8]);
        let mut out = Vec::new();
        cur.bytes(&mut out, BUFFER_CAPACITY + 1);
        assert!(out.is_empty());
        assert_eq!(cur.file_pos(), 0);
    }

    #[test]
    fn cursor_seek_repositions_inside_and_outside_the_buffer() {
        let mut cur = cursor_over(b"abcdef".to_vec());
        let mut out = Vec::new();
        cur.bytes(&mut out, 4);
        assert_eq!(out, b"abcd");
        cur.seek(1);
        assert_eq!(cur.file_pos(), 1);
        cur.bytes(&mut out, 2);
        assert_eq!(&out[4..], b"bc");
        cur.seek(100);
        assert_eq!(cur.u8(), None);
        cur.seek(4);
        cur.bytes(&mut out, 2);
        assert_eq!(&out[6..], b"ef");
    }

    #[test]
    fn cursor_peek_magic_does_not_consume() {
        let mut cur = cursor_over(vec![0x3f, 0xd7, 0x6c, 0x17, 0x99]);
        assert_eq!(cur.peek_magic(), Some([0x3f, 0xd7, 0x6c, 0x17]));
        assert_eq!(cur.peek_magic(), Some([0x3f, 0xd7, 0x6c, 0x17]));
        assert_eq!(cur.file_pos(), 0);
        assert_eq!(cur.u8(), Some(0x3f));
        let mut cur = cursor_over(vec![1, 2]);
        assert_eq!(cur.peek_magic(), None);
    }

    #[test]
    fn byte_reader_walks_scalars_and_slices() {
        let mut data = Vec::new();
        data.push(0x42);
        data.extend_from_slice(&vint(70000));
        data.extend_from_slice(b"xyz");
        let mut reader = ByteReader {
            data: &data,
            pos: 0,
        };
        assert_eq!(reader.u8(), Some(0x42));
        assert_eq!(reader.vlong(), Some(70000));
        assert_eq!(reader.bytes(3), Some(&b"xyz"[..]));
        assert_eq!(reader.bytes(1), None);
        assert_eq!(reader.u8(), None);
    }

    #[test]
    fn decompress_block_reads_pure_literals() {
        let mut cur = cursor_over(vec![0x30, b'a', b'b', b'c']);
        assert_eq!(decompress_block(&mut cur, 3), Some(b"abc".to_vec()));
    }

    #[test]
    fn decompress_block_expands_literal_lengths() {
        let mut stream = vec![0xf0, 0xff, 0x04];
        stream.extend_from_slice(&vec![b'q'; 15 + 255 + 4]);
        let mut cur = cursor_over(stream);
        let out = decompress_block(&mut cur, 274).unwrap();
        assert_eq!(out.len(), 274);
        assert!(out.iter().all(|&b| b == b'q'));
    }

    #[test]
    fn decompress_block_copies_overlapping_matches() {
        let mut cur = cursor_over(vec![0x11, b'x', 0x01, 0x00, 0x00]);
        let out = decompress_block(&mut cur, 6).unwrap();
        assert_eq!(out, b"xxxxxx");
    }

    #[test]
    fn decompress_block_expands_match_lengths() {
        let mut cur = cursor_over(vec![0x1f, b'x', 0x01, 0x00, 0x03, 0x00]);
        let out = decompress_block(&mut cur, 23).unwrap();
        assert_eq!(out, vec![b'x'; 23]);
    }

    #[test]
    fn decompress_block_chains_sequences() {
        let mut cur = cursor_over(vec![0x20, b'a', b'b', 0x01, 0x00, 0x00]);
        assert_eq!(decompress_block(&mut cur, 6), Some(b"abbbbb".to_vec()));
    }

    #[test]
    fn decompress_block_rejects_offset_past_the_window() {
        let mut cur = cursor_over(vec![0x10, b'a', 0x05, 0x00]);
        assert_eq!(decompress_block(&mut cur, 6), None);
    }

    #[test]
    fn decompress_block_rejects_truncated_streams() {
        let mut cur = cursor_over(vec![0x30, b'a']);
        assert_eq!(decompress_block(&mut cur, 3), None);
    }
}
