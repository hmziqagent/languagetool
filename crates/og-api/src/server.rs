use axum::Router;
use axum::routing::{get, post};
use og_langs::engine::LanguageEngine;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tower_http::cors::CorsLayer;
use tracing::info;

pub const MAX_TEXT_LENGTH: usize = 100_000;

pub struct AppState {
    engines: HashMap<String, OnceLock<Arc<LanguageEngine>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            engines: ENGINE_CODES
                .iter()
                .map(|code| (code.to_string(), OnceLock::new()))
                .collect(),
        }
    }

    pub fn engine_for(&self, code: &str) -> Option<&Arc<LanguageEngine>> {
        let lock = self.engines.get(code)?;
        Some(lock.get_or_init(|| build_engine_for(code)))
    }

    pub fn language_codes(&self) -> Vec<&str> {
        let mut codes: Vec<&str> = self.engines.keys().map(String::as_str).collect();
        codes.sort_unstable();
        codes
    }
}

pub const ENGINE_CODES: &[&str] = &[
    "en", "en-US", "en-GB", "en-CA", "en-AU", "en-NZ", "en-ZA", "de", "de-DE",
];

enum EngineSource {
    EnglishBase,
    EnglishVariant(&'static str),
    German,
}

fn engine_source(code: &str) -> Option<EngineSource> {
    match code {
        "en" | "en-US" => Some(EngineSource::EnglishBase),
        "en-GB" => Some(EngineSource::EnglishVariant("en-GB")),
        "en-CA" => Some(EngineSource::EnglishVariant("en-CA")),
        "en-AU" => Some(EngineSource::EnglishVariant("en-AU")),
        "en-NZ" => Some(EngineSource::EnglishVariant("en-NZ")),
        "en-ZA" => Some(EngineSource::EnglishVariant("en-ZA")),
        "de" | "de-DE" => Some(EngineSource::German),
        _ => None,
    }
}

fn shared_english_engine() -> Arc<LanguageEngine> {
    static ENGINE: OnceLock<Arc<LanguageEngine>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            info!("Loading English language engine...");
            let engine = Arc::new(LanguageEngine::english());
            info!("English engine loaded with {} rules", engine.rule_count());
            engine
        })
        .clone()
}

fn shared_german_engine() -> Arc<LanguageEngine> {
    static ENGINE: OnceLock<Arc<LanguageEngine>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            info!("Loading German language engine...");
            let engine = Arc::new(LanguageEngine::german());
            info!("German engine loaded with {} rules", engine.rule_count());
            engine
        })
        .clone()
}

fn build_engine_for(code: &str) -> Arc<LanguageEngine> {
    match engine_source(code) {
        Some(EngineSource::EnglishBase) => shared_english_engine(),
        Some(EngineSource::EnglishVariant(code)) => {
            info!("Loading {code} engine...");
            let variant = shared_english_engine().variant(code);
            info!("Loaded {code} engine with {} rules", variant.rule_count());
            Arc::new(variant)
        }
        Some(EngineSource::German) | None => shared_german_engine(),
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub fn shared_state() -> Arc<AppState> {
    static STATE: OnceLock<Arc<AppState>> = OnceLock::new();
    STATE.get_or_init(|| Arc::new(AppState::new())).clone()
}

pub fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v2/check", get(crate::handlers::handle_check))
        .route("/v2/check", post(crate::handlers::handle_check))
        .route("/v2/languages", get(crate::handlers::handle_languages))
        .route("/v2/version", get(crate::handlers::handle_version))
        .route("/v2/info", get(crate::handlers::handle_info))
        .route(
            "/v2/maxtextlength",
            get(crate::handlers::handle_max_text_length),
        )
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn run_server(port: u16) -> anyhow::Result<()> {
    let app = build_app(shared_state());

    let addr = format!("0.0.0.0:{port}");
    info!("OpenGrammar server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_variants_default_to_one_shared_instance() {
        assert!(matches!(
            engine_source("en"),
            Some(EngineSource::EnglishBase)
        ));
        assert!(matches!(
            engine_source("en-US"),
            Some(EngineSource::EnglishBase)
        ));
    }

    #[test]
    fn regional_variants_get_their_own_engine() {
        for code in ["en-GB", "en-CA", "en-AU", "en-NZ", "en-ZA"] {
            assert!(matches!(
                engine_source(code),
                Some(EngineSource::EnglishVariant(_))
            ));
        }
    }

    #[test]
    fn app_state_resolves_variant_codes() {
        let state = AppState::new();
        let codes = state.language_codes();
        for code in ["en", "en-US", "en-GB", "de", "de-DE"] {
            assert!(codes.contains(&code), "missing {code}");
        }
        assert!(!codes.contains(&"fr"));
    }
}
