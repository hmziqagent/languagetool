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
    "en", "en-US", "en-GB", "en-CA", "en-AU", "en-NZ", "en-ZA", "de", "de-DE", "de-AT", "de-CH",
    "fr", "es", "pt", "pt-BR", "pt-PT", "pt-AO", "pt-MZ", "nl", "pl", "ru", "it", "ar", "ast",
    "be", "br", "ca", "crh", "da", "el", "eo", "fa", "ga", "gl", "is", "ja", "km", "ro", "sv",
];

enum EngineSource {
    EnglishBase,
    EnglishVariant(&'static str),
    German,
    GermanVariant(&'static str),
    PortugueseVariant(&'static str),
    Generic(&'static str),
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
        "de-AT" => Some(EngineSource::GermanVariant("de-AT")),
        "de-CH" => Some(EngineSource::GermanVariant("de-CH")),
        "da" => Some(EngineSource::Generic("da")),
        "fr" => Some(EngineSource::Generic("fr")),
        "es" => Some(EngineSource::Generic("es")),
        "pt" | "pt-PT" => Some(EngineSource::PortugueseVariant("pt-PT")),
        "pt-BR" => Some(EngineSource::PortugueseVariant("pt-BR")),
        "pt-AO" => Some(EngineSource::PortugueseVariant("pt-AO")),
        "pt-MZ" => Some(EngineSource::PortugueseVariant("pt-MZ")),
        "nl" => Some(EngineSource::Generic("nl")),
        "pl" => Some(EngineSource::Generic("pl")),
        "ru" => Some(EngineSource::Generic("ru")),
        "it" => Some(EngineSource::Generic("it")),
        "ar" => Some(EngineSource::Generic("ar")),
        "ast" => Some(EngineSource::Generic("ast")),
        "be" => Some(EngineSource::Generic("be")),
        "br" => Some(EngineSource::Generic("br")),
        "ca" => Some(EngineSource::Generic("ca")),
        "crh" => Some(EngineSource::Generic("crh")),
        "el" => Some(EngineSource::Generic("el")),
        "eo" => Some(EngineSource::Generic("eo")),
        "fa" => Some(EngineSource::Generic("fa")),
        "ga" => Some(EngineSource::Generic("ga")),
        "gl" => Some(EngineSource::Generic("gl")),
        "is" => Some(EngineSource::Generic("is")),
        "ja" => Some(EngineSource::Generic("ja")),
        "km" => Some(EngineSource::Generic("km")),
        "ro" => Some(EngineSource::Generic("ro")),
        "sv" => Some(EngineSource::Generic("sv")),
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
        Some(EngineSource::Generic(short)) => shared_generic_engine(short),
        Some(EngineSource::PortugueseVariant(code)) => shared_portuguese_variant_engine(code),
        Some(EngineSource::GermanVariant(code)) => {
            info!("Loading {code} engine...");
            let variant = shared_german_engine().german_variant(code);
            info!("Loaded {code} engine with {} rules", variant.rule_count());
            Arc::new(variant)
        }
        Some(EngineSource::German) | None => shared_german_engine(),
    }
}

fn shared_generic_engine(short_code: &'static str) -> Arc<LanguageEngine> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<&'static str, Arc<LanguageEngine>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(hit) = guard.get(short_code) {
        return hit.clone();
    }
    info!("Loading {short_code} engine...");
    let engine = Arc::new(LanguageEngine::for_code(short_code).unwrap_or_else(|| {
        LanguageEngine::new(
            og_core::Language::from_code(short_code).unwrap_or(og_core::Language::English),
        )
    }));
    info!(
        "Loaded {short_code} engine with {} rules",
        engine.rule_count()
    );
    guard.insert(short_code, engine.clone());
    engine
}

fn shared_portuguese_variant_engine(code: &'static str) -> Arc<LanguageEngine> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<&'static str, Arc<LanguageEngine>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = cache.lock().unwrap();
    if let Some(hit) = guard.get(code) {
        return hit.clone();
    }
    info!("Loading {code} engine...");
    let engine = Arc::new(shared_generic_engine("pt").portuguese_variant(code));
    info!("Loaded {code} engine with {} rules", engine.rule_count());
    guard.insert(code, engine.clone());
    engine
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
        for code in [
            "en", "en-US", "en-GB", "de", "de-DE", "fr", "es", "pt", "nl", "pl", "ru", "ca",
        ] {
            assert!(codes.contains(&code), "missing {code}");
        }
    }
}
