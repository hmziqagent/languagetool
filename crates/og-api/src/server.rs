use axum::Router;
use axum::routing::get;
use og_langs::engine::LanguageEngine;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, OnceLock, RwLock};
use tower_http::cors::CorsLayer;
use tracing::info;

pub const MAX_TEXT_LENGTH: usize = 100_000;
pub const DEFAULT_PORT: u16 = 8081;

/// Process-wide shared English engine.
///
/// Building a `LanguageEngine` means compiling thousands of XML rules, so
/// engines are built once per process and shared (cheap `Arc` clones) by
/// every request and test. `LanguageEngine` is `Send + Sync` (its rules,
/// tagger, and disambiguator live behind `Arc<dyn ...>` of thread-safe
/// traits), and all per-check state is reset by `Checker::check`.
static ENGLISH_ENGINE: OnceLock<Arc<LanguageEngine>> = OnceLock::new();

/// The shared English engine, building it on first use.
pub fn shared_english_engine() -> Arc<LanguageEngine> {
    ENGLISH_ENGINE
        .get_or_init(|| {
            info!("Loading English language engine...");
            let engine = LanguageEngine::english();
            info!("English engine loaded with {} rules", engine.rule_count());
            Arc::new(engine)
        })
        .clone()
}

/// Map of language codes to shared engines. `en`, `en-US`, and `en-GB`
/// all resolve to the same English engine instance.
pub fn build_engines() -> HashMap<String, Arc<LanguageEngine>> {
    let english = shared_english_engine();
    let mut engines = HashMap::new();
    engines.insert("en".to_string(), english.clone());
    engines.insert("en-US".to_string(), english.clone());
    engines.insert("en-GB".to_string(), english);
    engines
}

pub struct AppState {
    pub engines: HashMap<String, Arc<LanguageEngine>>,
    /// User dictionary backing `/v2/words` (sorted for deterministic
    /// responses).
    pub user_words: Arc<RwLock<BTreeSet<String>>>,
}

impl AppState {
    /// Application state with the shared language engines.
    pub fn new() -> Arc<Self> {
        Self::with_engines(build_engines())
    }

    /// Application state over a custom engine map (used by tests that do
    /// not need any engine).
    pub fn with_engines(engines: HashMap<String, Arc<LanguageEngine>>) -> Arc<Self> {
        Arc::new(Self {
            engines,
            user_words: Arc::new(RwLock::new(BTreeSet::new())),
        })
    }
}

pub fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/v2/check",
            get(crate::handlers::handle_check_get).post(crate::handlers::handle_check_post),
        )
        .route("/v2/languages", get(crate::handlers::handle_languages))
        .route("/v2/version", get(crate::handlers::handle_version))
        .route("/v2/info", get(crate::handlers::handle_info))
        .route(
            "/v2/maxtextlength",
            get(crate::handlers::handle_max_text_length),
        )
        .route("/v2/configinfo", get(crate::handlers::handle_config_info))
        .route(
            "/v2/words",
            get(crate::handlers::handle_words_list)
                .post(crate::handlers::handle_words_add)
                .delete(crate::handlers::handle_words_remove),
        )
        .route("/v2/users/me", get(crate::handlers::handle_users_me))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn run_server(port: u16) -> anyhow::Result<()> {
    info!("Loading language engines (first build can take a while)...");
    let state = AppState::new();

    let app = build_app(state);

    let addr = format!("0.0.0.0:{port}");
    info!("OpenGrammar server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("OpenGrammar server listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
