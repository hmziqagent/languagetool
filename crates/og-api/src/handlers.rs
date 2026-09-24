use crate::request::MergedCheckParams;
use crate::response::{InfoResponse, LanguagesResponse, MaxTextLengthResponse, VersionResponse};
use crate::server::{AppState, MAX_TEXT_LENGTH};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Json;
use og_core::{CheckRequest, CheckResult, Language};
use std::sync::Arc;

pub async fn handle_check(
    State(state): State<Arc<AppState>>,
    MergedCheckParams(params): MergedCheckParams,
) -> Result<Json<CheckResult>, (StatusCode, Json<serde_json::Value>)> {
    let text = match params.get_text() {
        Some(t) if !t.is_empty() => t,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing 'text' parameter"})),
            ));
        }
    };

    if text.len() > MAX_TEXT_LENGTH {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Text is too long. Maximum length is {} characters.", MAX_TEXT_LENGTH)
            })),
        ));
    }

    let language = match params.get_language() {
        Some(l) => l,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "Missing or invalid 'language' parameter"})),
            ));
        }
    };

    let engine = if let Some(e) = state.engine_for(language.code()) {
        e
    } else if let Some(e) = state.engine_for(language.short_code()) {
        e
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": format!("Unsupported language: {}", language.code())
            })),
        ));
    };

    let mut request = CheckRequest::new(text, language);
    request.enabled_rules = params.get_enabled_rules();
    request.disabled_rules = params.get_disabled_rules();
    request.enabled_categories = params.get_enabled_categories();
    request.disabled_categories = params.get_disabled_categories();
    request.level = params.level.clone();
    request.picky = params.picky;
    request.mother_tongue = params
        .mother_tongue
        .as_deref()
        .and_then(Language::from_code);

    let result = engine.check_with_request(&request);
    Ok(Json(result))
}

pub async fn handle_languages(State(state): State<Arc<AppState>>) -> Json<Vec<LanguagesResponse>> {
    let languages = state
        .language_codes()
        .into_iter()
        .filter_map(|code| {
            Language::from_code(code).map(|l| LanguagesResponse {
                name: l.name().to_string(),
                code: l.short_code().to_string(),
                long_code: l.code().to_string(),
            })
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
