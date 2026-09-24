use axum::body::Body;
use axum::extract::FromRequest;
use axum::http::{Method, Request};
use og_core::Language;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct CheckParams {
    pub text: Option<String>,
    pub data: Option<String>,
    pub language: String,
    pub mother_tongue: Option<String>,
    #[serde(rename = "enabledRules")]
    pub enabled_rules: Option<String>,
    #[serde(rename = "disabledRules")]
    pub disabled_rules: Option<String>,
    #[serde(rename = "enabledCategories")]
    pub enabled_categories: Option<String>,
    #[serde(rename = "disabledCategories")]
    pub disabled_categories: Option<String>,
    pub level: Option<String>,
    #[serde(default)]
    pub picky: bool,
}

impl CheckParams {
    pub fn get_text(&self) -> Option<&str> {
        self.text.as_deref().or(self.data.as_deref())
    }

    pub fn get_language(&self) -> Option<Language> {
        Language::from_code(&self.language)
    }

    pub fn get_enabled_rules(&self) -> Vec<String> {
        Self::split_list(self.enabled_rules.as_deref())
    }

    pub fn get_disabled_rules(&self) -> Vec<String> {
        Self::split_list(self.disabled_rules.as_deref())
    }

    pub fn get_enabled_categories(&self) -> Vec<String> {
        Self::split_list(self.enabled_categories.as_deref())
    }

    pub fn get_disabled_categories(&self) -> Vec<String> {
        Self::split_list(self.disabled_categories.as_deref())
    }

    fn split_list(value: Option<&str>) -> Vec<String> {
        value
            .map(|s| {
                s.split(',')
                    .map(|r| r.trim().to_string())
                    .filter(|r| !r.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn from_map(map: &HashMap<String, String>) -> Self {
        let get = |key: &str| map.get(key).cloned();
        Self {
            text: get("text"),
            data: get("data"),
            language: get("language").unwrap_or_default(),
            mother_tongue: get("motherTongue"),
            enabled_rules: get("enabledRules"),
            disabled_rules: get("disabledRules"),
            enabled_categories: get("enabledCategories"),
            disabled_categories: get("disabledCategories"),
            level: get("level"),
            picky: matches!(
                map.get("picky").map(String::as_str),
                Some("true") | Some("yes")
            ),
        }
    }
}

pub fn parse_urlencoded_into(map: &mut HashMap<String, String>, input: &[u8]) {
    for pair in input.split(|&b| b == b'&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.iter().position(|&b| b == b'=') {
            Some(pos) => (&pair[..pos], &pair[pos + 1..]),
            None => (pair, [].as_slice()),
        };
        map.insert(percent_decode(key), percent_decode(value));
    }
}

fn percent_decode(input: &[u8]) -> String {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < input.len()
                && input[i + 1].is_ascii_hexdigit()
                && input[i + 2].is_ascii_hexdigit() =>
            {
                let hi = (input[i + 1] as char).to_digit(16).unwrap_or(0);
                let lo = (input[i + 2] as char).to_digit(16).unwrap_or(0);
                out.push((hi * 16 + lo) as u8);
                i += 2;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct MergedCheckParams(pub CheckParams);

impl<S> FromRequest<S> for MergedCheckParams
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request(req: Request<Body>, _state: &S) -> Result<Self, Self::Rejection> {
        let (parts, body) = req.into_parts();
        let mut map = HashMap::new();
        if parts.method == Method::POST {
            let bytes = axum::body::to_bytes(body, crate::server::MAX_TEXT_LENGTH * 4 + 8192)
                .await
                .unwrap_or_default();
            parse_urlencoded_into(&mut map, &bytes);
        }
        if let Some(query) = parts.uri.query() {
            parse_urlencoded_into(&mut map, query.as_bytes());
        }
        Ok(MergedCheckParams(CheckParams::from_map(&map)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plus_and_percent_sequences() {
        let mut map = HashMap::new();
        parse_urlencoded_into(&mut map, b"text=Hello+world&language=en-US&extra=a%20b%2Fc");
        assert_eq!(map.get("text").map(String::as_str), Some("Hello world"));
        assert_eq!(map.get("language").map(String::as_str), Some("en-US"));
        assert_eq!(map.get("extra").map(String::as_str), Some("a b/c"));
    }

    #[test]
    fn later_inserts_override_earlier_ones() {
        let mut map = HashMap::new();
        parse_urlencoded_into(&mut map, b"text=from+body");
        parse_urlencoded_into(&mut map, b"text=from+query");
        assert_eq!(map.get("text").map(String::as_str), Some("from query"));
    }

    #[test]
    fn keeps_lone_percent_and_valueless_keys() {
        let mut map = HashMap::new();
        parse_urlencoded_into(&mut map, b"flag&text=100%&x=%E2%82%AC");
        assert_eq!(map.get("flag").map(String::as_str), Some(""));
        assert_eq!(map.get("text").map(String::as_str), Some("100%"));
        assert_eq!(map.get("x").map(String::as_str), Some("€"));
    }

    #[test]
    fn builds_params_from_map() {
        let mut map = HashMap::new();
        map.insert("text".to_string(), "hi".to_string());
        map.insert("language".to_string(), "en-GB".to_string());
        map.insert("disabledRules".to_string(), "A,B".to_string());
        map.insert("picky".to_string(), "true".to_string());
        let params = CheckParams::from_map(&map);
        assert_eq!(params.get_text(), Some("hi"));
        assert_eq!(
            params.get_language().map(|l| l.code().to_string()),
            Some("en-GB".to_string())
        );
        assert_eq!(
            params.get_disabled_rules(),
            vec!["A".to_string(), "B".to_string()]
        );
        assert!(params.picky);
    }
}
