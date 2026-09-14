use crate::{AnalyzedDocument, AnalyzedSentence, Category, IssueType, RuleContext, RuleMatch};

pub trait Rule: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
    fn is_default_on(&self) -> bool {
        true
    }
    fn category(&self) -> Category {
        Category::new("MISC", "Miscellaneous")
    }
    fn issue_type(&self) -> IssueType {
        IssueType::Grammar
    }
    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch>;

    /// Context-aware variant of [`Rule::match_sentence`].
    ///
    /// The [`Checker`](crate::Checker) pipeline calls this. The default
    /// implementation ignores the context and delegates to
    /// `match_sentence`, so rules that don't need language/picky state
    /// only implement `match_sentence`; rules that do should override
    /// this (and may leave `match_sentence` returning an empty vec).
    fn match_sentence_with_context(
        &self,
        sentence: &AnalyzedSentence,
        _ctx: &RuleContext,
    ) -> Vec<RuleMatch> {
        self.match_sentence(sentence)
    }

    /// Reset per-check mutable state (Java `Rule.reset()`).
    ///
    /// Called by the pipeline before each check when rule instances are
    /// reused across requests. Default: no-op.
    fn reset(&self) {}
}

pub trait TextLevelRule: Send + Sync {
    fn id(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
    fn is_default_on(&self) -> bool {
        true
    }
    fn category(&self) -> Category {
        Category::new("MISC", "Miscellaneous")
    }
    fn issue_type(&self) -> IssueType {
        IssueType::Grammar
    }
    fn match_text(&self, text: &str, sentences: &[AnalyzedSentence]) -> Vec<RuleMatch>;

    /// Document-level variant of [`TextLevelRule::match_text`].
    ///
    /// The pipeline calls this with the assembled [`AnalyzedDocument`] and
    /// per-check [`RuleContext`]. Default: delegate to `match_text` with the
    /// document's text and sentences, so existing text-level rules keep
    /// working unchanged.
    fn match_document(&self, doc: &AnalyzedDocument, _ctx: &RuleContext) -> Vec<RuleMatch> {
        self.match_text(doc.text(), doc.sentences())
    }

    /// Reset per-check mutable state. Default: no-op.
    fn reset(&self) {}
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuleId(String);

impl RuleId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
