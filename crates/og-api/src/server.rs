use axum::Router;
use axum::routing::{get, post};
use og_langs::engine::LanguageEngine;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tower_http::cors::CorsLayer;
use tracing::info;

pub const MAX_TEXT_LENGTH: usize = 100_000;

pub struct AppState {
    engines: Arc<HashMap<String, Arc<LanguageEngine>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            engines: Arc::new(build_engines()),
        }
    }

    pub fn engine_for(&self, code: &str) -> Option<&Arc<LanguageEngine>> {
        self.engines.get(code)
    }

    pub fn language_codes(&self) -> Vec<&str> {
        let mut codes: Vec<&str> = self.engines.keys().map(String::as_str).collect();
        codes.sort_unstable();
        codes
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

pub fn build_engines() -> HashMap<String, Arc<LanguageEngine>> {
    info!("Loading English language engine...");
    let en_engine = Arc::new(LanguageEngine::english());
    info!(
        "English engine loaded with {} rules",
        en_engine.rule_count()
    );
    let mut engines = engines_for_variants(en_engine);

    info!("Loading German language engine...");
    let de_engine = Arc::new(LanguageEngine::german());
    info!("German engine loaded with {} rules", de_engine.rule_count());
    engines.insert("de".to_string(), de_engine.clone());
    engines.insert("de-DE".to_string(), de_engine);
    engines
}

fn engines_for_variants(engine: Arc<LanguageEngine>) -> HashMap<String, Arc<LanguageEngine>> {
    let mut engines = HashMap::new();
    engines.insert("en".to_string(), engine.clone());
    engines.insert("en-US".to_string(), engine.clone());
    for code in ["en-GB", "en-CA", "en-AU", "en-NZ", "en-ZA"] {
        let variant = engine.variant(code);
        info!("Loaded {} engine with {} rules", code, variant.rule_count());
        engines.insert(code.to_string(), Arc::new(variant));
    }
    engines
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
    use og_core::Language;

    #[test]
    fn engine_variants_default_to_one_shared_instance() {
        let engines = engines_for_variants(Arc::new(LanguageEngine::new(Language::English)));
        let en = engines.get("en").expect("en engine");
        assert!(Arc::ptr_eq(en, engines.get("en-US").expect("en-US engine")));
    }

    #[test]
    fn regional_variants_get_their_own_engine() {
        let engines = engines_for_variants(Arc::new(LanguageEngine::new(Language::English)));
        let en = engines.get("en").expect("en engine");
        for code in ["en-GB", "en-CA", "en-AU", "en-NZ", "en-ZA"] {
            assert!(!Arc::ptr_eq(en, engines.get(code).unwrap_or(en)));
        }
    }

    #[test]
    fn app_state_resolves_variant_codes() {
        let engines = engines_for_variants(Arc::new(LanguageEngine::new(Language::English)));
        let state = AppState {
            engines: Arc::new(engines),
        };
        assert!(state.engine_for("en-US").is_some());
        assert!(state.engine_for("en-GB").is_some());
        assert!(state.engine_for("de-DE").is_none());
    }
}
