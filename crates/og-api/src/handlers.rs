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

    let mut request = CheckRequest::new(text, language);
    request.mother_tongue = params.get_mother_tongue();
    request.enabled_rules = params.get_enabled_rules();
    request.disabled_rules = params.get_disabled_rules();
    request.enabled_categories = params.get_enabled_categories();
    request.disabled_categories = params.get_disabled_categories();
    request.level = params.level.clone();
    request.picky = params.picky;

    Ok(engine.check_with_request(&request))
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
        cache_size: state.engines.len(),
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
