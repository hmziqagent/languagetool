use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct LanguagesResponse {
    pub name: String,
    pub code: String,
    #[serde(rename = "longCode")]
    pub long_code: String,
}

#[derive(Debug, Serialize)]
pub struct VersionResponse {
    pub name: String,
    pub version: String,
    #[serde(rename = "apiVersion")]
    pub api_version: i32,
}

#[derive(Debug, Serialize)]
pub struct InfoResponse {
    pub name: String,
    pub version: String,
    #[serde(rename = "apiVersion")]
    pub api_version: i32,
    #[serde(rename = "maxTextLength")]
    pub max_text_length: usize,
    pub premium: bool,
}

#[derive(Debug, Serialize)]
pub struct MaxTextLengthResponse {
    #[serde(rename = "maxTextLength")]
    pub max_text_length: usize,
}

/// Response of `GET /v2/configinfo`: server-level configuration, LT-shaped
/// field names. Only settings the server actually honors are reported.
#[derive(Debug, Serialize)]
pub struct ConfigInfoResponse {
    #[serde(rename = "maxTextLength")]
    pub max_text_length: usize,
    #[serde(rename = "maxTextHardLength")]
    pub max_text_hard_length: usize,
    #[serde(rename = "disabledRules")]
    pub disabled_rules: Vec<String>,
    #[serde(rename = "disabledCategories")]
    pub disabled_categories: Vec<String>,
    #[serde(rename = "supportedLanguages")]
    pub supported_languages: Vec<String>,
    #[serde(rename = "pipelineCaching")]
    pub pipeline_caching: bool,
    #[serde(rename = "cacheSize")]
    pub cache_size: usize,
}

/// Response of `GET /v2/words`: the user dictionary, sorted.
#[derive(Debug, Serialize)]
pub struct WordsResponse {
    pub words: Vec<String>,
    pub offset: usize,
    pub limit: Option<usize>,
}

/// Response of `POST`/`DELETE /v2/words`: outcome of one mutation.
#[derive(Debug, Serialize)]
pub struct WordMutationResponse {
    pub status: String,
    pub word: String,
}

/// Response of `GET /v2/users/me`. The local server has no accounts, so it
/// reports a single anonymous non-premium user (LT cloud requires auth here;
/// this server intentionally does not).
#[derive(Debug, Serialize)]
pub struct UserInfoResponse {
    pub id: u64,
    #[serde(rename = "firstName")]
    pub first_name: String,
    #[serde(rename = "lastName")]
    pub last_name: String,
    pub email: Option<String>,
    #[serde(rename = "isPremium")]
    pub is_premium: bool,
    #[serde(rename = "isAnonymous")]
    pub is_anonymous: bool,
}
