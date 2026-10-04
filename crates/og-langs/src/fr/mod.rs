pub mod compound;
#[cfg(test)]
pub(crate) mod diagnose;
#[cfg(test)]
pub(crate) mod example_suite;
pub mod question_whitespace;
pub mod repeated_words;
#[cfg(test)]
pub(crate) mod residue_map;
pub mod simple_replace;
pub mod suppress_misspelled;
pub mod tagger;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
pub(crate) mod trace;

use og_rules::postponed_adjective::LangSynthesizer;
use og_tagger::GenericTagger;
use std::sync::Arc;

pub use compound::CompoundRule;
pub use question_whitespace::QuestionWhitespaceRule;
pub use repeated_words::FrenchRepeatedWordsRule;
pub use simple_replace::SimpleReplaceRule;

pub fn build_fr_disambiguator(
    resource_dir: &std::path::Path,
) -> Option<Arc<dyn og_core::checker::Disambiguator>> {
    let xml = std::fs::read_to_string(resource_dir.join("disambiguation.xml")).ok()?;
    let xml_disambiguator = og_tagger::XmlDisambiguator::from_xml(&xml).ok()?;
    let core = crate::engine::find_lt_core_resource_dir()?;
    let chain = og_tagger::hybrid_disambiguator("fr", resource_dir, &core, xml_disambiguator);
    let global = std::fs::read_to_string(core.join("disambiguation-global.xml"))
        .ok()
        .and_then(|global_xml| {
            let expanded =
                og_xml::parser::XmlRuleParser::expand_entities_with_base(&global_xml, Some(&core));
            og_tagger::XmlDisambiguator::from_xml(&expanded).ok()
        });
    Some(match global {
        Some(global) => Arc::new(og_tagger::ChainedDisambiguator::new(vec![
            chain,
            Arc::new(global),
        ])),
        None => chain,
    })
}

pub fn french_compound_rule(tagger: Option<Arc<GenericTagger>>) -> CompoundRule {
    CompoundRule::french(tagger)
}

pub fn french_simple_replace_rule() -> SimpleReplaceRule {
    SimpleReplaceRule::french()
}

pub fn french_question_whitespace_rule() -> QuestionWhitespaceRule {
    QuestionWhitespaceRule::new()
}

pub fn french_question_whitespace_strict_rule() -> QuestionWhitespaceRule {
    QuestionWhitespaceRule::strict()
}

pub fn french_repeated_words_rule(
    synthesizer: Option<Arc<LangSynthesizer>>,
) -> FrenchRepeatedWordsRule {
    FrenchRepeatedWordsRule::new(synthesizer)
}

fn java_trim_whitespace(s: &str) -> String {
    let trimmed = s.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    let mut out = String::with_capacity(trimmed.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c <= ' ' && (chars.get(i + 1).is_some_and(|n| *n <= ' ') || i > 1 && chars[i - 1] <= ' ')
        {
            i += 1;
            continue;
        }
        if c != '\n' && c != '\t' && c != '\r' {
            out.push(c);
        }
        i += 1;
    }
    out
}

fn normalize_pattern_text(text: &str) -> std::borrow::Cow<'_, str> {
    let trimmed = java_trim_whitespace(text);
    if trimmed.contains("(?-)") {
        trimmed.replace("(?-)", "").into()
    } else {
        trimmed.into()
    }
}

pub fn normalize_fr_rules_xml(xml: &str) -> std::borrow::Cow<'_, str> {
    let mut out = String::new();
    let mut rest = xml;
    let mut stack: Vec<&str> = Vec::new();
    let mut changed = false;
    while let Some(lt) = rest.find('<') {
        let text = &rest[..lt];
        let top = stack.last().copied();
        if matches!(top, Some("token") | Some("exception")) && !text.contains('<') {
            let normalized = normalize_pattern_text(text);
            if normalized != text {
                changed = true;
            }
            out.push_str(&normalized);
        } else {
            out.push_str(text);
        }
        let (verbatim_until, tag_end) = if rest[lt..].starts_with("<!--") {
            match rest[lt..].find("-->") {
                Some(end) => (lt + end + 3, None),
                None => (rest.len(), None),
            }
        } else if rest[lt..].starts_with("<![CDATA[") {
            match rest[lt..].find("]]>") {
                Some(end) => (lt + end + 3, None),
                None => (rest.len(), None),
            }
        } else {
            (0, find_tag_end(&rest[lt..]))
        };
        if let Some(tag_end) = tag_end {
            let tag = &rest[lt..=lt + tag_end];
            let is_close = tag.starts_with("</");
            let name = if is_close { &tag[2..] } else { &tag[1..] };
            let name_end = name
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .unwrap_or(name.len());
            let name = &name[..name_end];
            let self_closing = tag.ends_with("/>");
            if is_close {
                if stack.last().copied() == Some(name) {
                    stack.pop();
                }
            } else if !self_closing && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            {
                stack.push(name);
            }
            out.push_str(tag);
            rest = &rest[lt + tag_end + 1..];
        } else if verbatim_until > lt {
            out.push_str(&rest[lt..verbatim_until]);
            rest = &rest[verbatim_until..];
        } else {
            out.push_str(rest);
            rest = "";
        }
    }
    if !changed {
        return xml.into();
    }
    out.push_str(rest);
    out.into()
}

fn find_tag_end(fragment: &str) -> Option<usize> {
    let mut in_quote = None;
    for (i, b) in fragment.bytes().enumerate() {
        match in_quote {
            Some(q) if b == q => in_quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => in_quote = Some(b),
                b'>' => return Some(i),
                _ => {}
            },
        }
    }
    None
}

pub(crate) fn context_window(text: &str, text_start: usize, from: usize, to: usize) -> String {
    let mut rel_lo = from.saturating_sub(40).max(text_start) - text_start;
    let mut rel_hi = (to + 40).min(text_start + text.len()) - text_start;
    while rel_lo > 0 && !text.is_char_boundary(rel_lo) {
        rel_lo -= 1;
    }
    while rel_hi < text.len() && !text.is_char_boundary(rel_hi) {
        rel_hi += 1;
    }
    text[rel_lo..rel_hi].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::rule::{Rule as _, TextLevelRule as _};

    #[test]
    fn java_trim_whitespace_drops_control_chars_and_runs_keeps_single_spaces() {
        assert_eq!(
            java_trim_whitespace("frais\n            |froid"),
            "frais|froid"
        );
        assert_eq!(java_trim_whitespace("New York"), "New York");
        assert_eq!(java_trim_whitespace("a\tb\rc"), "abc");
        assert_eq!(java_trim_whitespace("  bord  "), "bord");
        assert_eq!(java_trim_whitespace("a  b"), "ab");
    }

    #[test]
    fn normalize_rules_xml_only_touches_token_and_exception_text() {
        let xml = "<rules>\n  <rule id=\"X\">\n    <token regexp=\"yes\">(?-i)[A-Z].*|(?-)[A-Z]</token>\n\
                   <token>chaud\n                    <exception>froid</exception></token>\n\
                   <message>garde\n   la ligne</message>\n  </rule>\n</rules>\n";
        let out = normalize_fr_rules_xml(xml);
        let out = out.as_ref();
        assert!(out.contains("(?-i)[A-Z].*|[A-Z]"), "{out}");
        assert!(out.contains("chaud<exception>froid</exception>"), "{out}");
        assert!(
            out.contains("<message>garde\n   la ligne</message>"),
            "text outside token/exception must stay verbatim: {out}"
        );
    }

    #[test]
    fn normalize_rules_xml_leaves_comments_verbatim() {
        let xml = "<!-- <token>foo\n  bar</token> --><token>baz</token>";
        let out = normalize_fr_rules_xml(xml);
        assert_eq!(out.as_ref(), xml);
    }

    #[test]
    fn normalize_rules_xml_returns_borrowed_when_nothing_changes() {
        let xml = "<rules><rule><token>abc</token></rule></rules>";
        assert!(matches!(
            normalize_fr_rules_xml(xml),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn french_native_rules_use_java_ids() {
        assert_eq!(french_compound_rule(None).id(), "FR_COMPOUNDS");
        assert_eq!(
            french_simple_replace_rule().id(),
            "FR_SIMPLE_REPLACE_SIMPLE"
        );
        assert_eq!(french_question_whitespace_rule().id(), "FRENCH_WHITESPACE");
        assert_eq!(
            french_question_whitespace_strict_rule().id(),
            "FRENCH_WHITESPACE_STRICT"
        );
        assert_eq!(french_repeated_words_rule(None).id(), "FR_REPEATEDWORDS");
    }

    #[test]
    fn strict_whitespace_rule_is_picky_and_temp_off() {
        let strict = french_question_whitespace_strict_rule();
        assert!(strict.is_picky());
        assert!(!strict.is_default_on());
        assert!(!french_question_whitespace_rule().is_picky());
    }
}
