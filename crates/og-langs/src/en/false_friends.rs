//! Rust port of LanguageTool's L2 false-friend rules
//! (`EnglishForGermansFalseFriendRule`, `EnglishForFrenchFalseFriendRule`,
//! `EnglishForDutchmenFalseFriendRule`, `EnglishForSpaniardsFalseFriendRule`).
//!
//! Java subclasses `ConfusionProbabilityRule` with the directional
//! `confusion_sets_l2_*.txt` data and gates on the request's mother tongue.
//! The Rust port reuses [`super::confusion_probability::ConfusionCore`] and
//! checks `RuleContext::mother_tongue()` — matches are produced only when the
//! check request names the corresponding L1 (documented divergence: Java
//! instantiates one rule per mother tongue, here one rule self-gates).

use super::confusion_probability::{ConfusionCore, parse_confusion_pairs};
use og_core::{AnalyzedSentence, Category, IssueType, RuleContext, RuleMatch, rule::Rule};
use og_languagemodel::LanguageModel;
use std::sync::Arc;

pub struct FalseFriendRule {
    id: String,
    description: String,
    mother_tongue_code: String,
    core: ConfusionCore,
}

impl FalseFriendRule {
    fn new(
        id: &str,
        description: &str,
        mother_tongue_code: &str,
        data_file: &str,
        lm: Arc<LanguageModel>,
    ) -> Self {
        let data = og_spell::read_lt_resource(&format!(
            "en/src/main/resources/org/languagetool/resource/en/{data_file}"
        ))
        .unwrap_or_default();
        Self {
            id: id.to_string(),
            description: description.to_string(),
            mother_tongue_code: mother_tongue_code.to_string(),
            core: ConfusionCore::new(lm, parse_confusion_pairs(&data)),
        }
    }

    /// False friends for German native speakers
    /// (Java `EnglishForGermansFalseFriendRule`, id
    /// `EN_FOR_DE_SPEAKERS_FALSE_FRIENDS`, confusion_sets_l2_de.txt).
    pub fn for_germans(lm: Arc<LanguageModel>) -> Self {
        Self::new(
            "EN_FOR_DE_SPEAKERS_FALSE_FRIENDS",
            "Statistics-based false friends for German speakers",
            "de",
            "confusion_sets_l2_de.txt",
            lm,
        )
    }

    /// Java `EnglishForFrenchFalseFriendRule` (confusion_sets_l2_fr.txt).
    pub fn for_french(lm: Arc<LanguageModel>) -> Self {
        Self::new(
            "EN_FOR_FR_SPEAKERS_FALSE_FRIENDS",
            "Statistics-based false friends for French speakers",
            "fr",
            "confusion_sets_l2_fr.txt",
            lm,
        )
    }

    /// Java `EnglishForDutchmenFalseFriendRule` (confusion_sets_l2_nl.txt).
    pub fn for_dutch(lm: Arc<LanguageModel>) -> Self {
        Self::new(
            "EN_FOR_NL_SPEAKERS_FALSE_FRIENDS",
            "Statistics-based false friends for Dutch speakers",
            "nl",
            "confusion_sets_l2_nl.txt",
            lm,
        )
    }

    /// Java `EnglishForSpaniardsFalseFriendRule` (confusion_sets_l2_es.txt).
    pub fn for_spaniards(lm: Arc<LanguageModel>) -> Self {
        Self::new(
            "EN_FOR_ES_SPEAKERS_FALSE_FRIENDS",
            "Statistics-based false friends for Spanish speakers",
            "es",
            "confusion_sets_l2_es.txt",
            lm,
        )
    }
}

impl Rule for FalseFriendRule {
    fn id(&self) -> &str {
        &self.id
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn is_default_on(&self) -> bool {
        // Java does not setDefaultOff; these only exist when motherTongue
        // is set (here: self-gated in match_sentence_with_context).
        true
    }

    fn category(&self) -> Category {
        Category::new("FALSE_FRIENDS", "False Friends")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Other("nonconformance".to_string())
    }

    fn match_sentence(&self, _sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        // Needs the request context (mother tongue) — see
        // match_sentence_with_context.
        Vec::new()
    }

    fn match_sentence_with_context(
        &self,
        sentence: &AnalyzedSentence,
        ctx: &RuleContext,
    ) -> Vec<RuleMatch> {
        let mother_tongue_matches = ctx
            .mother_tongue()
            .is_some_and(|l| l.code().split('-').next() == Some(&*self.mother_tongue_code));
        if !mother_tongue_matches {
            return Vec::new();
        }
        self.core
            .match_sentence(sentence, &self.id, &self.description, self.category())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use og_core::{AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, Language};

    fn sentence_from_tokens(words: &[&str]) -> AnalyzedSentence {
        let text = words.join(" ");
        let mut s = AnalyzedSentence::new(text.clone(), 0, text.len());
        let mut pos = 0usize;
        let tokens = words
            .iter()
            .map(|w| {
                let t = AnalyzedToken::new(*w, pos, pos + w.len());
                pos += w.len() + 1;
                AnalyzedTokenReadings::new(t)
            })
            .collect();
        s.set_tokens(tokens);
        s
    }

    fn trained_lm() -> Arc<LanguageModel> {
        let mut lm = LanguageModel::new(3);
        for _ in 0..20 {
            lm.add_text("my phone is broken my phone works");
        }
        Arc::new(lm)
    }

    #[test]
    fn no_match_without_matching_mother_tongue() {
        let rule = FalseFriendRule::for_germans(trained_lm());
        let s = sentence_from_tokens(&["my", "handy", "is", "broken"]);
        let ctx = RuleContext::new(Language::English).with_mother_tongue(None);
        assert!(rule.match_sentence_with_context(&s, &ctx).is_empty());
        let ctx_fr = RuleContext::new(Language::English)
            .with_mother_tongue(Some(Language::from_code("fr").unwrap()));
        assert!(rule.match_sentence_with_context(&s, &ctx_fr).is_empty());
    }

    #[test]
    fn match_sentence_without_context_is_noop() {
        let rule = FalseFriendRule::for_germans(trained_lm());
        let s = sentence_from_tokens(&["my", "handy", "is", "broken"]);
        assert!(rule.match_sentence(&s).is_empty());
    }

    #[test]
    fn data_backed_match_with_german_mother_tongue() {
        let lt_root = match og_spell::resource::lt_resource_root() {
            Some(p) => p,
            None => {
                eprintln!("Skipping: LT resource dir not found");
                return;
            }
        };
        let data_file = lt_root
            .join("en/src/main/resources/org/languagetool/resource/en/confusion_sets_l2_de.txt");
        if !data_file.exists() {
            eprintln!("Skipping: confusion_sets_l2_de.txt not found");
            return;
        }
        // Confusion pair from the real data: "handy -> phone" (German
        // "Handy" = mobile phone). Train so "my phone is" is well attested.
        let mut lm = LanguageModel::new(3);
        for _ in 0..20 {
            lm.add_text("my phone is broken");
        }
        let rule = FalseFriendRule::for_germans(Arc::new(lm));
        let s = sentence_from_tokens(&["my", "handy", "is", "broken"]);
        let ctx = RuleContext::new(Language::English)
            .with_mother_tongue(Some(Language::from_code("de").unwrap()));
        let matches = rule.match_sentence_with_context(&s, &ctx);
        assert!(
            matches.iter().any(|m| m
                .replacements()
                .iter()
                .any(|r| r.value().eq_ignore_ascii_case("phone"))),
            "expected 'handy' -> 'phone' false-friend suggestion"
        );
    }
}
