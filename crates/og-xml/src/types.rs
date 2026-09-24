use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlFilter {
    pub class: String,
    #[serde(default)]
    pub args: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlRule {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub sub_id: Option<String>,
    #[serde(default)]
    pub description: String,
    pub category: XmlCategory,
    #[serde(default)]
    pub antipatterns: Vec<XmlPattern>,
    #[serde(default)]
    pub pattern: XmlPattern,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub short_message: Option<String>,
    #[serde(default)]
    pub suggestions: Vec<XmlSuggestion>,
    #[serde(default)]
    pub examples: Vec<XmlExample>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub default_on: bool,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub filter: Option<XmlFilter>,
    #[serde(default)]
    pub issue_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlCategory {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default_on: bool,
    #[serde(default)]
    pub issue_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlPattern {
    pub elements: Vec<XmlPatternElement>,
    pub case_sensitive: bool,
    pub marker_start: Option<usize>,
    pub marker_end: Option<usize>,
    #[serde(skip)]
    pub marker_elem_start: Option<usize>,
    #[serde(skip)]
    pub marker_elem_end: Option<usize>,
    pub tokens: Vec<XmlPatternToken>,
    #[serde(skip)]
    pub unify_groups: Vec<XmlUnifyGroup>,
}

#[derive(Debug, Clone, Default)]
pub struct XmlUnifyGroup {
    pub negate: bool,
    pub features: Vec<(String, Vec<String>)>,
    pub token_start: usize,
    pub token_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum XmlPatternElement {
    Token(XmlPatternToken),
    OrGroup(XmlOrGroup),
    AndGroup(XmlAndGroup),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlOrGroup {
    pub alternatives: Vec<XmlPatternToken>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlAndGroup {
    pub constraints: Vec<XmlPatternToken>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlPatternToken {
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub regexp: Option<String>,
    #[serde(default)]
    pub postag: Option<String>,
    #[serde(default)]
    pub postag_regexp: Option<String>,
    #[serde(default)]
    pub negate: bool,
    #[serde(default)]
    pub negate_pos: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub inflected: bool,
    #[serde(default)]
    pub min: Option<i32>,
    #[serde(default)]
    pub max: Option<i32>,
    #[serde(default)]
    pub skip: i32,
    #[serde(default)]
    pub exceptions: Vec<XmlException>,
    #[serde(skip)]
    pub unify_neutral: bool,
    #[serde(default)]
    pub space_before: Option<String>,
    #[serde(default)]
    pub chunk: Option<String>,
    #[serde(default)]
    pub chunk_re: Option<String>,
    #[serde(default)]
    pub match_no: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlException {
    pub text: Option<String>,
    pub regexp: Option<String>,
    pub postag: Option<String>,
    pub postag_regexp: Option<String>,
    pub negate: bool,
    pub negate_pos: bool,
    pub inflected: bool,
    pub case_sensitive: bool,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlMatch {
    pub no: i32,
    pub case_conversion: Option<String>,
    pub regexp_match: Option<String>,
    pub regexp_replace: Option<String>,
    pub include_inflected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestionPart {
    Text(String),
    Match(XmlMatch),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlSuggestion {
    pub text: String,
    #[serde(default)]
    pub parts: Vec<SuggestionPart>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlExample {
    pub example_type: XmlExampleType,
    pub text: String,
    #[serde(default)]
    pub corrections: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum XmlExampleType {
    #[default]
    Correct,
    Incorrect,
    TriggersError,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlRuleGroup {
    pub id: Option<String>,
    pub name: Option<String>,
    pub default_on: Option<bool>,
    pub antipatterns: Vec<XmlPattern>,
    pub rules: Vec<XmlRule>,
    #[serde(default)]
    pub issue_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct XmlRuleFile {
    #[serde(skip)]
    pub unifications: Vec<crate::disambig_types::UnificationFeature>,
    pub categories: Vec<XmlCategory>,
    pub rule_groups: Vec<XmlRuleGroup>,
    pub rules: Vec<XmlRule>,
}
