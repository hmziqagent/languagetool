use crate::request::{CheckParams, WordParams};
use crate::response::{
    ConfigInfoResponse, InfoResponse, LanguagesResponse, MaxTextLengthResponse, UserInfoResponse,
    VersionResponse, WordMutationResponse, WordsResponse,
};
use crate::server::{AppState, MAX_TEXT_LENGTH};
use axum::extract::{Form, Query, State, rejection::FormRejection};
use axum::http::StatusCode;
use axum::response::Json;
use og_core::{CheckRequest, CheckResult, Language};
use std::collections::HashSet;
use std::sync::Arc;

/// LT-shaped JSON error body with an HTTP status.
type ApiError = (StatusCode, Json<serde_json::Value>);

fn api_error(status: StatusCode, message: impl Into<String>) -> ApiError {
    (status, Json(serde_json::json!({ "error": message.into() })))
}

/// `GET /v2/check` — parameters come from the query string.
pub async fn handle_check_get(
    State(state): State<Arc<AppState>>,
    Query(params): Query<CheckParams>,
) -> Result<Json<CheckResult>, ApiError> {
    perform_check(&state, params).map(Json)
}

/// `POST /v2/check` — parameters may come from the query string, a
/// `application/x-www-form-urlencoded` body (as LanguageTool clients send),
/// or both; query-string values win over body values.
pub async fn handle_check_post(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CheckParams>,
    form: Result<Form<CheckParams>, FormRejection>,
) -> Result<Json<CheckResult>, ApiError> {
    let params = match form {
        Ok(Form(body)) => query.or(body),
        Err(_) => query,
    };
    perform_check(&state, params).map(Json)
}

/// Shared `/v2/check` logic: validates required parameters, resolves the
/// language engine, and runs the check with all LT parameters wired into a
/// [`CheckRequest`] (rule/category enable+disable lists, `level`, `picky`,
/// `motherTongue`).
fn perform_check(state: &AppState, params: CheckParams) -> Result<CheckResult, ApiError> {
    let text = match params.get_text() {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                "Missing 'text' parameter",
            ));
        }
    };

    if text.len() > MAX_TEXT_LENGTH {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            format!(
                "Text is too long. Maximum length is {} characters.",
                MAX_TEXT_LENGTH
            ),
        ));
    }

    let language = match params.get_language() {
        Some(l) => l,
        None => {
            // Distinguish "no language at all" from "a language we cannot
            // serve" — both are 400s, but the latter must name the language.
            let message = match params.language.as_deref() {
                Some(code) => format!("Unsupported language: {code}"),
                None => "Missing 'language' parameter".to_string(),
            };
            return Err(api_error(StatusCode::BAD_REQUEST, message));
        }
    };

    // `motherTongue` is optional, but a non-blank value that is not a
    // known language code is rejected (mirroring `language` handling).
    let mother_tongue = match params.get_mother_tongue() {
        Some(l) => Some(l),
        None if params
            .mother_tongue
            .as_deref()
            .is_some_and(|m| !m.trim().is_empty()) =>
        {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!(
                    "Unsupported motherTongue: {}",
                    params.mother_tongue.as_deref().unwrap_or_default()
                ),
            ));
        }
        None => None,
    };

    // Find the appropriate language engine: exact code first, then the
    // short code (e.g. "en-US" -> "en").
    let engine = {
        let engines = &state.engines;
        if let Some(e) = engines.get(language.code()) {
            e
        } else if let Some(e) = engines.get(language.short_code()) {
            e
        } else {
            return Err(api_error(
                StatusCode::BAD_REQUEST,
                format!("Unsupported language: {}", language.code()),
            ));
        }
    };

    // LanguageTool semantics for the enabled*/disabled* parameters (per
    // LT's HTTP API docs): `enabledRules`/`enabledCategories` are ADDITIVE
    // — they turn the listed rules/categories on IN ADDITION to the
    // defaults — and become RESTRICTIVE only when `enabledOnly=true`.
    // `disabledRules`/`disabledCategories` always subtract. og-core's
    // `CheckRequest` treats non-empty enabled lists as restrictive, so
    // additive mode is emulated with two passes: the defaults pass (empty
    // enabled lists) plus the restricted pass, merged and deduplicated.
    let mut base = CheckRequest::new(text, language);
    base.mother_tongue = mother_tongue;
    base.disabled_rules = params.get_disabled_rules();
    base.disabled_categories = params.get_disabled_categories();
    base.level = params.level.clone();
    base.picky = params.picky;

    let enabled_rules = params.get_enabled_rules();
    let enabled_categories = params.get_enabled_categories();

    if enabled_rules.is_empty() && enabled_categories.is_empty() {
        // Nothing beyond the defaults to enable (and an empty enabled
        // list restricts to nothing, so enabledOnly is moot): one pass.
        return Ok(engine.check_with_request(&base));
    }

    let mut restricted = base.clone();
    restricted.enabled_rules = enabled_rules;
    restricted.enabled_categories = enabled_categories;
    let restricted_result = engine.check_with_request(&restricted);

    if params.enabled_only {
        return Ok(restricted_result);
    }

    // Additive mode: union of the default and restricted rule sets.
    let mut result = engine.check_with_request(&base);
    merge_matches(&mut result, restricted_result);
    Ok(result)
}

/// Merge `extra` matches into `result`, dropping matches a rule already
/// produced in the base pass (same rule, position, and message). Used to
/// union the defaults pass with the restricted pass in additive mode.
fn merge_matches(result: &mut CheckResult, extra: CheckResult) {
    let mut seen: HashSet<(String, usize, usize, String)> = result
        .matches
        .iter()
        .map(|m| {
            (
                m.rule().id().to_string(),
                m.offset(),
                m.length(),
                m.message().to_string(),
            )
        })
        .collect();
    for m in extra.matches {
        let key = (
            m.rule().id().to_string(),
            m.offset(),
            m.length(),
            m.message().to_string(),
        );
        if seen.insert(key) {
            result.matches.push(m);
        }
    }
    result.matches.sort_by_key(|m| m.offset());
}

pub async fn handle_languages() -> Json<Vec<LanguagesResponse>> {
    let languages = Language::all_languages()
        .into_iter()
        .map(|l| LanguagesResponse {
            name: l.name().to_string(),
            code: l.short_code().to_string(),
            long_code: l.code().to_string(),
        })
        .collect();
    Json(languages)
}

pub async fn handle_version() -> Json<VersionResponse> {
    Json(VersionResponse {
        name: "OpenGrammar".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        api_version: 2,
    })
}

pub async fn handle_info() -> Json<InfoResponse> {
    Json(InfoResponse {
        name: "OpenGrammar".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        api_version: 2,
        max_text_length: MAX_TEXT_LENGTH,
        premium: false,
    })
}

pub async fn handle_max_text_length() -> Json<MaxTextLengthResponse> {
    Json(MaxTextLengthResponse {
        max_text_length: MAX_TEXT_LENGTH,
    })
}

/// `GET /v2/configinfo` — server configuration the API actually honors.
pub async fn handle_config_info(State(state): State<Arc<AppState>>) -> Json<ConfigInfoResponse> {
    Json(ConfigInfoResponse {
        max_text_length: MAX_TEXT_LENGTH,
        max_text_hard_length: MAX_TEXT_LENGTH,
        disabled_rules: Vec::new(),
        disabled_categories: Vec::new(),
        supported_languages: {
            let mut codes: Vec<String> = state.engines.keys().cloned().collect();
            codes.sort();
            codes
        },
        // Engines are shared process-wide, so every request reuses them.
        pipeline_caching: true,
        // Distinct cached engine instances by pointer identity (all three
        // codes share one Arc).
        cache_size: state
            .engines
            .values()
            .map(Arc::as_ptr)
            .collect::<HashSet<_>>()
            .len(),
    })
}

/// `GET /v2/words` — list the user dictionary (sorted, with optional
/// `offset`/`limit` pagination).
pub async fn handle_words_list(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WordParams>,
) -> Json<WordsResponse> {
    let all: Vec<String> = state
        .user_words
        .read()
        .expect("user words lock poisoned")
        .iter()
        .cloned()
        .collect();
    let offset = params.offset.unwrap_or(0).min(all.len());
    let end = match params.limit {
        Some(limit) => offset.saturating_add(limit).min(all.len()),
        None => all.len(),
    };
    Json(WordsResponse {
        words: all[offset..end].to_vec(),
        offset,
        limit: params.limit,
    })
}

/// `POST /v2/words` — add a word to the user dictionary. The word may be
/// supplied in the query string or the form body.
pub async fn handle_words_add(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WordParams>,
    form: Result<Form<WordParams>, FormRejection>,
) -> Result<Json<WordMutationResponse>, ApiError> {
    let params = match form {
        Ok(Form(body)) => query.or(body),
        Err(_) => query,
    };
    let word = params
        .get_word()
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "Missing 'word' parameter"))?;

    let mut words = state.user_words.write().expect("user words lock poisoned");
    let status = if words.insert(word.clone()) {
        "added"
    } else {
        "exists"
    };
    Ok(Json(WordMutationResponse {
        status: status.to_string(),
        word,
    }))
}

/// `DELETE /v2/words?word=...` — remove a word from the user dictionary.
pub async fn handle_words_remove(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WordParams>,
) -> Result<Json<WordMutationResponse>, ApiError> {
    let word = params
        .get_word()
        .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "Missing 'word' parameter"))?;

    let removed = state
        .user_words
        .write()
        .expect("user words lock poisoned")
        .remove(&word);
    if !removed {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            format!("Word not found: {}", word),
        ));
    }
    Ok(Json(WordMutationResponse {
        status: "deleted".to_string(),
        word,
    }))
}

/// `GET /v2/users/me` — the local server has no accounts; it reports a
/// single anonymous, non-premium user.
pub async fn handle_users_me() -> Json<UserInfoResponse> {
    Json(UserInfoResponse {
        id: 0,
        first_name: "Anonymous".to_string(),
        last_name: "User".to_string(),
        email: None,
        is_premium: false,
        is_anonymous: true,
    })
}
