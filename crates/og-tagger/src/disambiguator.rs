use og_core::AnalyzedSentence;
use og_core::checker::Disambiguator;
use std::sync::Arc;

pub struct NoOpDisambiguator;

impl Disambiguator for NoOpDisambiguator {
    fn disambiguate(&self, _sentence: &mut AnalyzedSentence) {}
}

pub struct ChainedDisambiguator {
    stages: Vec<Arc<dyn Disambiguator>>,
}

impl ChainedDisambiguator {
    pub fn new(stages: Vec<Arc<dyn Disambiguator>>) -> Self {
        Self { stages }
    }
}

impl Disambiguator for ChainedDisambiguator {
    fn disambiguate(&self, sentence: &mut AnalyzedSentence) {
        for stage in &self.stages {
            stage.disambiguate(sentence);
        }
    }
}

pub struct ContextualDisambiguator;

impl ContextualDisambiguator {
    fn collect_removals(sentence: &AnalyzedSentence) -> Vec<(usize, &'static str)> {
        let tokens = sentence.tokens();
        let n = tokens.len();
        let nw: Vec<usize> = (0..n).filter(|&i| !tokens[i].is_whitespace()).collect();
        if nw.len() < 3 {
            return Vec::new();
        }

        let nw_text: Vec<&str> = nw.iter().map(|&i| tokens[i].token().token()).collect();
        let nw_tags: Vec<Vec<&str>> = nw.iter().map(|&i| tokens[i].all_pos_tags()).collect();

        let mut removals: Vec<(usize, &'static str)> = Vec::new();
        let has = |i: usize, tag: &str| nw_tags[i].contains(&tag);
        let has_prefixed =
            |i: usize, prefix: &str| nw_tags[i].iter().any(|t| t.starts_with(prefix));

        for i in 0..nw.len() {
            let text = nw_text[i];
            let lower = text.to_lowercase();

            if i >= 1 {
                let prev_lower = nw_text[i - 1].to_lowercase();
                let prev_is_do_verb = matches!(prev_lower.as_str(), "do" | "does" | "did")
                    && has_prefixed(i - 1, "VB");
                if prev_is_do_verb {
                    if has(i, "VBZ") && has_prefixed(i, "NN") {
                        removals.push((i, "VBZ"));
                    }
                    if has(i, "VB") && has_prefixed(i, "NN") && !has(i, "NN:UN") && !has(i, "NN:U")
                    {
                        removals.push((i, "VB"));
                    }
                }
                if i >= 2 && nw_text[i - 1].eq_ignore_ascii_case("not") {
                    let do_idx = i - 2;
                    let do_lower = nw_text[do_idx].to_lowercase();
                    let do_is_verb = matches!(do_lower.as_str(), "do" | "does" | "did")
                        && has_prefixed(do_idx, "VB");
                    if do_is_verb && has(i, "VBZ") && has_prefixed(i, "NN") {
                        removals.push((i, "VBZ"));
                    }
                }
            }

            if lower == "does" && has(i, "VBZ") && has(i, "NNS") && i >= 1 {
                let prev_lower = nw_text[i - 1].to_lowercase();
                if matches!(prev_lower.as_str(), "the" | "these" | "those") && i + 1 < nw.len() {
                    let next_tags = &nw_tags[i + 1];
                    if next_tags.contains(&"VBD") || next_tags.contains(&"VBN") {
                        removals.push((i, "VBZ"));
                    }
                }
            }

            if i >= 1 {
                let prev_tags = &nw_tags[i - 1];
                let prev_is_det = prev_tags
                    .iter()
                    .any(|t| *t == "DT" || *t == "PRP$" || *t == "PRP_S");
                if prev_is_det && has(i, "VBZ") && has_prefixed(i, "NN") {
                    removals.push((i, "VBZ"));
                }
            }

            if i >= 1 {
                let prev_tags = &nw_tags[i - 1];
                let prev_is_subject =
                    prev_tags.contains(&"PRP") || prev_tags.iter().any(|t| t.starts_with("NNP"));
                if prev_is_subject && has(i, "IN") && has(i, "VB") {
                    let verb_preps = ["like", "love", "want", "need", "hope", "try"];
                    if verb_preps.contains(&lower.as_str()) {
                        removals.push((i, "IN"));
                    }
                }
            }

            if i >= 2 {
                let prev2_tags = &nw_tags[i - 2];
                let prev2_is_det = prev2_tags
                    .iter()
                    .any(|t| *t == "DT" || *t == "PRP$" || *t == "PRP_S");
                let prev1_is_singular_noun = has_prefixed(i - 1, "NN") && !has(i - 1, "NNS");
                let next_starts_complement = i + 1 < nw.len()
                    && (nw_tags[i + 1].contains(&"IN")
                        || nw_tags[i + 1].contains(&"TO")
                        || nw_tags[i + 1].contains(&"DT")
                        || nw_tags[i + 1].contains(&"PRP$")
                        || nw_tags[i + 1].contains(&"NNS"));
                let current_is_plural = has(i, "NNS");
                if prev2_is_det
                    && prev1_is_singular_noun
                    && has_prefixed(i, "NN")
                    && !next_starts_complement
                    && !current_is_plural
                {
                    if has(i, "VBP") {
                        removals.push((i, "VBP"));
                    }
                    if has(i, "VBZ") {
                        removals.push((i, "VBZ"));
                    }
                }
            }

            if i >= 2 {
                let prev2_lower = nw_text[i - 2].to_lowercase();
                let prev1_lower = nw_text[i - 1].to_lowercase();
                let is_do_nt = matches!(prev2_lower.as_str(), "does" | "do" | "did")
                    && matches!(prev1_lower.as_str(), "n't" | "not");
                if is_do_nt && has(i, "IN") && has(i, "VB") {
                    removals.push((i, "IN"));
                }
            }

            if i >= 1 && nw_tags[i - 1].contains(&"MD") && has(i, "IN") && has(i, "VB") {
                removals.push((i, "IN"));
            }

            if i >= 1
                && (nw_tags[i - 1].contains(&"TO") || nw_tags[i - 1].contains(&"IN"))
                && nw_text[i - 1].eq_ignore_ascii_case("to")
                && has(i, "IN")
                && has(i, "VB")
            {
                removals.push((i, "IN"));
            }

            if i >= 1 {
                let prev_tags = &nw_tags[i - 1];
                let prev_is_det = prev_tags
                    .iter()
                    .any(|t| *t == "DT" || *t == "PRP$" || *t == "PRP_S");
                if prev_is_det && has_prefixed(i, "NN") && has(i, "VBZ") {
                    removals.push((i, "VBZ"));
                }
            }

            if i >= 1 {
                let prev_lower = nw_text[i - 1].to_lowercase();
                let prev_is_have =
                    matches!(prev_lower.as_str(), "have" | "has" | "had" | "'ve" | "'s")
                        && has_prefixed(i - 1, "VB");
                if prev_is_have && has(i, "VBN") && has(i, "VBD") {
                    removals.push((i, "VBD"));
                }
            }

            if i >= 1 {
                let prev_lower = nw_text[i - 1].to_lowercase();
                let prev_is_quant = matches!(
                    prev_lower.as_str(),
                    "many"
                        | "more"
                        | "most"
                        | "few"
                        | "several"
                        | "some"
                        | "these"
                        | "those"
                        | "various"
                        | "numerous"
                );
                if prev_is_quant && has_prefixed(i, "NN") && has(i, "VBZ") {
                    removals.push((i, "VBZ"));
                }
            }

            if i >= 1 && i + 1 < nw.len() {
                let prev_tags = &nw_tags[i - 1];
                let prev_is_subject = prev_tags.contains(&"PRP");
                if prev_is_subject && has(i, "VB") && has_prefixed(i, "NN") {
                    let next_tags = &nw_tags[i + 1];
                    let next_is_verb = next_tags.iter().any(|t| t.starts_with("VB") || *t == "MD");
                    let next_is_det = next_tags
                        .iter()
                        .any(|t| *t == "DT" || *t == "PRP$" || *t == "IN");
                    if !next_is_verb && !next_is_det && nw_text[i + 1] != "to" {
                        for nn_tag in ["NN", "NNS"] {
                            if has(i, nn_tag) {
                                removals.push((i, nn_tag));
                            }
                        }
                    }
                }
            }

            if i >= 1
                && nw_tags[i - 1].contains(&"TO")
                && nw_text[i - 1].eq_ignore_ascii_case("to")
                && has(i, "VB")
            {
                for nn_tag in ["NN", "NNS"] {
                    if has(i, nn_tag) {
                        removals.push((i, nn_tag));
                    }
                }
            }

            if i >= 1 && nw_tags[i - 1].contains(&"MD") && has(i, "VB") {
                for nn_tag in ["NN", "NNS"] {
                    if has(i, nn_tag) {
                        removals.push((i, nn_tag));
                    }
                }
            }

            if i >= 1 {
                let prev_lower = nw_text[i - 1].to_lowercase();
                let prev_is_have =
                    matches!(prev_lower.as_str(), "have" | "has" | "had" | "'ve" | "'s")
                        && has_prefixed(i - 1, "VB");
                let prev_is_have_or_rb = prev_is_have
                    || (i >= 2 && {
                        let pp_lower = nw_text[i - 2].to_lowercase();
                        let pp_is_have =
                            matches!(pp_lower.as_str(), "have" | "has" | "had" | "'ve" | "'s")
                                && has_prefixed(i - 2, "VB");
                        pp_is_have && nw_tags[i - 1].contains(&"RB")
                    });
                if prev_is_have_or_rb && (has(i, "VBN") || has(i, "VBD")) {
                    for nn_tag in ["NN", "NNS"] {
                        if has(i, nn_tag) {
                            removals.push((i, nn_tag));
                        }
                    }
                }
            }

            if i >= 1 && i + 1 < nw.len() {
                let prev_lower = nw_text[i - 1].to_lowercase();
                let prev_is_have =
                    matches!(prev_lower.as_str(), "have" | "has" | "had" | "'ve" | "'s")
                        && has_prefixed(i - 1, "VB");
                if prev_is_have {
                    let has_jj = has(i, "JJ");
                    let has_vbn = has(i, "VBN");
                    let has_vbd = has(i, "VBD");
                    if has_jj && (has_vbn || has_vbd) {
                        let next_tags = &nw_tags[i + 1];
                        let next_is_noun_or_det = next_tags.iter().any(|t| {
                            t.starts_with("NN") || *t == "DT" || *t == "PRP$" || *t == "PRP_S"
                        });
                        if next_is_noun_or_det {
                            if has_vbn {
                                removals.push((i, "VBN"));
                            }
                            if has_vbd {
                                removals.push((i, "VBD"));
                            }
                        }
                    }
                }
            }

            if i + 1 < nw.len() && nw_tags[i + 1].contains(&"DT") && has_prefixed(i, "NN") {
                if has(i, "JJR") {
                    removals.push((i, "JJR"));
                }
                if has(i, "JJS") {
                    removals.push((i, "JJS"));
                }
            }

            if i >= 2 {
                let prev2_tags = &nw_tags[i - 2];
                let prev1_tags = &nw_tags[i - 1];
                let prev2_is_in = prev2_tags.contains(&"IN");
                let prev1_is_det = prev1_tags.contains(&"DT") || prev1_tags.contains(&"PRP$");
                if prev2_is_in && prev1_is_det && has_prefixed(i, "NN") {
                    removals.push((i, "VB"));
                    removals.push((i, "VBP"));
                    removals.push((i, "VBZ"));
                }
            }

            if i >= 1 {
                let prev_tags = &nw_tags[i - 1];
                let prev_text_lower = nw_text[i - 1].to_lowercase();
                let prev_is_a_an = prev_text_lower == "a" || prev_text_lower == "an";
                if prev_is_a_an && prev_tags.contains(&"DT") && has_prefixed(i, "NN") {
                    if has(i, "VB") {
                        removals.push((i, "VB"));
                    }
                    if has(i, "VBP") {
                        removals.push((i, "VBP"));
                    }
                }
            }

            if i == 0 && has(i, "VB") && has_prefixed(i, "NN") && nw.len() > 1 {
                let next_tags = &nw_tags[1];
                let next_suggests_noun_subject = next_tags.iter().any(|t| {
                    *t == "DT" || *t == "IN" || t.starts_with("NN") || *t == "CC" || *t == "MD"
                });
                if next_suggests_noun_subject {
                    removals.push((i, "VB"));
                    removals.push((i, "VBP"));
                }
            }

            if i >= 1 {
                let prev_text_lower = nw_text[i - 1].to_lowercase();
                if matches!(prev_text_lower.as_str(), "," | ":" | ";") && has_prefixed(i, "NN") {
                    if has(i, "VBZ") {
                        removals.push((i, "VBZ"));
                    }
                    if has(i, "VBP") {
                        removals.push((i, "VBP"));
                    }
                }
            }

            if i + 1 < nw.len() {
                let next_text_lower = nw_text[i + 1].to_lowercase();
                let next_is_to = next_text_lower == "to" && nw_tags[i + 1].contains(&"TO");
                if next_is_to && i >= 1 {
                    let prev_is_subject = nw_tags[i - 1]
                        .iter()
                        .any(|t| *t == "PRP" || t.starts_with("PRP_S") || t.starts_with("PRP_O"));
                    let prev_text_lower = nw_text[i - 1].to_lowercase();
                    let prev_is_aux = matches!(
                        prev_text_lower.as_str(),
                        "have"
                            | "has"
                            | "had"
                            | "'ve"
                            | "'s"
                            | "be"
                            | "been"
                            | "being"
                            | "am"
                            | "is"
                            | "are"
                            | "was"
                            | "were"
                            | "do"
                            | "does"
                            | "did"
                    );
                    let prev_is_lexical_verb = !prev_is_aux && has_prefixed(i - 1, "VB");
                    if !prev_is_subject && !prev_is_lexical_verb && has_prefixed(i, "NN") {
                        removals.push((i, "VB"));
                        removals.push((i, "VBP"));
                    }
                }
            }

            if i + 2 < nw.len() {
                let next_is_cc = nw_tags[i + 1].contains(&"CC");
                let next2_is_nn = nw_tags[i + 2].iter().any(|t| t.starts_with("NN"));
                if next_is_cc && next2_is_nn && has_prefixed(i, "NN") {
                    removals.push((i, "VB"));
                    removals.push((i, "VBP"));
                    removals.push((i, "VBZ"));
                }
            }
        }

        let mut resolved: Vec<(usize, &'static str)> = Vec::with_capacity(removals.len());
        for (nw_idx, tag) in removals {
            if let Some(&real_idx) = nw.get(nw_idx) {
                resolved.push((real_idx, tag));
            }
        }
        resolved
    }
}

impl Disambiguator for ContextualDisambiguator {
    fn disambiguate(&self, sentence: &mut AnalyzedSentence) {
        let removals = Self::collect_removals(sentence);
        if removals.is_empty() {
            return;
        }
        let tokens_mut = sentence.tokens_mut();
        for (idx, tag_to_remove) in removals {
            let token = &mut tokens_mut[idx];
            for reading in token.readings_mut() {
                reading.set_pos_tags(
                    reading
                        .pos_tags()
                        .iter()
                        .filter(|t| t.as_str() != tag_to_remove)
                        .cloned()
                        .collect(),
                );
            }
            let base = token.token_mut();
            base.set_pos_tags(
                base.pos_tags()
                    .iter()
                    .filter(|t| t.as_str() != tag_to_remove)
                    .cloned()
                    .collect(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::AnalyzedToken;
    use og_core::AnalyzedTokenReadings;

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
        let mut sentence = AnalyzedSentence::new("", 0, offset);
        sentence.set_tokens(tokens);
        sentence
    }

    #[test]
    fn removes_verb_reading_after_determiner() {
        let mut s = sentence(&[
            ("The", &["DT"]),
            ("runs", &["VBZ", "NNS"]),
            ("fast", &["RB"]),
        ]);
        ContextualDisambiguator.disambiguate(&mut s);
        let tokens = s.tokens();
        assert!(!tokens[2].has_pos_tag("VBZ"));
        assert!(tokens[2].has_pos_tag("NNS"));
    }

    #[test]
    fn leaves_clean_sentences_alone() {
        let mut s = sentence(&[
            ("She", &["PRP"]),
            ("runs", &["VBZ"]),
            ("fast", &["RB"]),
            (".", &["."]),
        ]);
        ContextualDisambiguator.disambiguate(&mut s);
        assert!(s.tokens()[2].has_pos_tag("VBZ"));
    }

    #[test]
    fn chained_disambiguator_runs_all_stages() {
        let mut s = sentence(&[
            ("The", &["DT"]),
            ("runs", &["VBZ", "NNS"]),
            ("fast", &["RB"]),
        ]);
        let chained = ChainedDisambiguator::new(vec![
            std::sync::Arc::new(NoOpDisambiguator),
            std::sync::Arc::new(ContextualDisambiguator),
        ]);
        chained.disambiguate(&mut s);
        assert!(!s.tokens()[2].has_pos_tag("VBZ"));
    }
}
