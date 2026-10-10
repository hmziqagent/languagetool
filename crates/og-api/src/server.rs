use axum::Router;
use axum::routing::{get, post};
use og_langs::engine::LanguageEngine;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};
use tower_http::cors::CorsLayer;
use tracing::info;

pub const MAX_TEXT_LENGTH: usize = 100_000;

const DEFAULT_ENGINE_CACHE_CAPACITY: usize = 3;

struct EngineCache {
    entries: HashMap<String, Arc<LanguageEngine>>,
    order: VecDeque<String>,
    capacity: usize,
}

impl EngineCache {
    fn touch(&mut self, code: &str) {
        if let Some(pos) = self.order.iter().position(|c| c == code) {
            self.order.remove(pos);
            self.order.push_front(code.to_string());
        }
    }

    fn insert(&mut self, code: String, engine: Arc<LanguageEngine>) {
        if self.entries.insert(code.clone(), engine).is_some() {
            if let Some(pos) = self.order.iter().position(|c| *c == code) {
                self.order.remove(pos);
            }
        }
        self.order.push_front(code);
        while self.order.len() > self.capacity
            && let Some(evicted) = self.order.pop_back()
        {
            self.entries.remove(&evicted);
        }
    }
}

pub struct AppState {
    engines: Mutex<EngineCache>,
    building: Mutex<HashMap<String, Arc<OnceLock<Arc<LanguageEngine>>>>>,
}

impl AppState {
    pub fn new() -> Self {
        let capacity = std::env::var("OG_ENGINE_CACHE_CAPACITY")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|cap| *cap > 0)
            .unwrap_or(DEFAULT_ENGINE_CACHE_CAPACITY);
        Self {
            engines: Mutex::new(EngineCache {
                entries: HashMap::new(),
                order: VecDeque::new(),
                capacity,
            }),
            building: Mutex::new(HashMap::new()),
        }
    }

    pub fn engine_for(&self, code: &str) -> Option<Arc<LanguageEngine>> {
        let canonical = canonical_engine_key(code)?;
        {
            let mut cache = self.engines.lock().unwrap();
            if let Some(engine) = cache.entries.get(&canonical).cloned() {
                cache.touch(&canonical);
                return Some(engine);
            }
        }
        let slot = {
            let mut building = self.building.lock().unwrap();
            building
                .entry(canonical.clone())
                .or_insert_with(|| Arc::new(OnceLock::new()))
                .clone()
        };
        let built = slot.get_or_init(|| build_engine_for(&canonical)).clone();
        if let Some(mut cache) = self.engines.lock().ok() {
            cache.insert(canonical.clone(), built.clone());
        }
        if let Ok(mut building) = self.building.lock()
            && let Some(existing) = building.get(&canonical)
            && Arc::ptr_eq(existing, &slot)
        {
            building.remove(&canonical);
        }
        Some(built)
    }

    pub fn language_codes(&self) -> Vec<&str> {
        let mut codes: Vec<&str> = ENGINE_CODES.to_vec();
        codes.sort_unstable();
        codes
    }
}

pub const ENGINE_CODES: &[&str] = &[
    "en", "en-US", "en-GB", "en-CA", "en-AU", "en-NZ", "en-ZA", "de", "de-DE", "de-AT", "de-CH",
    "fr", "es", "pt", "pt-BR", "pt-PT", "pt-AO", "pt-MZ", "nl", "pl", "ru", "it", "ar", "ast",
    "be", "br", "ca", "crh", "da", "el", "eo", "fa", "ga", "gl", "is", "ja", "km", "lt", "ml",
    "ro", "sk", "sl", "sr", "sv", "ta", "tl", "uk", "zh",
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
        "lt" => Some(EngineSource::Generic("lt")),
        "ml" => Some(EngineSource::Generic("ml")),
        "ro" => Some(EngineSource::Generic("ro")),
        "sk" => Some(EngineSource::Generic("sk")),
        "sl" => Some(EngineSource::Generic("sl")),
        "sr" => Some(EngineSource::Generic("sr")),
        "sv" => Some(EngineSource::Generic("sv")),
        "ta" => Some(EngineSource::Generic("ta")),
        "tl" => Some(EngineSource::Generic("tl")),
        "uk" => Some(EngineSource::Generic("uk")),
        "zh" => Some(EngineSource::Generic("zh")),
        _ => None,
    }
}

fn canonical_engine_key(code: &str) -> Option<String> {
    match engine_source(code)? {
        EngineSource::EnglishBase => Some("en".to_string()),
        EngineSource::EnglishVariant(code) => Some(code.to_string()),
        EngineSource::German => Some("de".to_string()),
        EngineSource::GermanVariant(code) => Some(code.to_string()),
        EngineSource::PortugueseVariant(code) => Some(code.to_string()),
        EngineSource::Generic(short) => Some(short.to_string()),
    }
}

fn build_engine_for(code: &str) -> Arc<LanguageEngine> {
    match engine_source(code) {
        Some(EngineSource::EnglishBase) => {
            info!("Loading English language engine...");
            let engine = Arc::new(LanguageEngine::english());
            info!("English engine loaded with {} rules", engine.rule_count());
            engine
        }
        Some(EngineSource::EnglishVariant(code)) => {
            info!("Loading {code} engine...");
            let variant = LanguageEngine::english().variant(code);
            info!("Loaded {code} engine with {} rules", variant.rule_count());
            Arc::new(variant)
        }
        Some(EngineSource::Generic(short)) => {
            info!("Loading {short} engine...");
            let engine = Arc::new(LanguageEngine::for_code(short).unwrap_or_else(|| {
                LanguageEngine::new(
                    og_core::Language::from_code(short).unwrap_or(og_core::Language::English),
                )
            }));
            info!("Loaded {short} engine with {} rules", engine.rule_count());
            engine
        }
        Some(EngineSource::PortugueseVariant(code)) => {
            info!("Loading {code} engine...");
            let base = LanguageEngine::for_code("pt").unwrap_or_else(|| {
                LanguageEngine::new(
                    og_core::Language::from_code("pt").unwrap_or(og_core::Language::PortuguesePt),
                )
            });
            let engine = Arc::new(base.portuguese_variant(code));
            info!("Loaded {code} engine with {} rules", engine.rule_count());
            engine
        }
        Some(EngineSource::GermanVariant(code)) => {
            info!("Loading {code} engine...");
            let variant = LanguageEngine::german().german_variant(code);
            info!("Loaded {code} engine with {} rules", variant.rule_count());
            Arc::new(variant)
        }
        Some(EngineSource::German) | None => {
            info!("Loading German language engine...");
            let engine = Arc::new(LanguageEngine::german());
            info!("German engine loaded with {} rules", engine.rule_count());
            engine
        }
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
        for code in [
            "en", "en-US", "en-GB", "de", "de-DE", "fr", "es", "pt", "nl", "pl", "ru", "ca",
        ] {
            assert!(codes.contains(&code), "missing {code}");
        }
    }

    #[test]
    fn unknown_codes_have_no_engine_source_or_canonical_key() {
        for code in ["", "xx", "en-IN", "de-DE ", "PT"] {
            assert!(engine_source(code).is_none(), "{code}");
            assert!(canonical_engine_key(code).is_none(), "{code}");
        }
    }

    #[test]
    fn canonical_keys_collapse_families_and_keep_variants_apart() {
        let mut canonical: Vec<String> = ENGINE_CODES
            .iter()
            .filter_map(|code| canonical_engine_key(code))
            .collect();
        assert_eq!(canonical_engine_key("en"), canonical_engine_key("en-US"));
        assert_eq!(canonical_engine_key("de"), canonical_engine_key("de-DE"));
        assert_eq!(canonical_engine_key("pt"), canonical_engine_key("pt-PT"));
        canonical.sort_unstable();
        canonical.dedup();
        for unique in ["en", "en-GB", "de", "de-CH", "pt-BR", "pt-MZ", "fr", "zh"] {
            assert!(canonical.contains(&unique.to_string()), "missing {unique}");
        }
        assert_eq!(canonical.len(), ENGINE_CODES.len() - 3);
    }

    #[test]
    fn language_codes_are_sorted_engine_codes() {
        let state = AppState::new();
        let codes = state.language_codes();
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        assert_eq!(codes, sorted);
        assert_eq!(codes.len(), ENGINE_CODES.len());
    }

    fn cache_with_capacity(capacity: usize) -> EngineCache {
        EngineCache {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn engine(code: &str) -> Arc<LanguageEngine> {
        Arc::new(LanguageEngine::new(
            og_core::Language::from_code(code).unwrap_or(og_core::Language::English),
        ))
    }

    #[test]
    fn engine_cache_evicts_least_recently_used_beyond_capacity() {
        let mut cache = cache_with_capacity(2);
        cache.insert("en".to_string(), engine("en"));
        cache.insert("de".to_string(), engine("de"));
        cache.touch("en");
        cache.insert("fr".to_string(), engine("fr"));
        assert!(cache.entries.contains_key("en"));
        assert!(cache.entries.contains_key("fr"));
        assert!(
            !cache.entries.contains_key("de"),
            "LRU entry must be evicted"
        );
        assert_eq!(cache.order, ["fr".to_string(), "en".to_string()]);
    }

    #[test]
    fn engine_cache_touch_moves_known_code_to_front_only() {
        let mut cache = cache_with_capacity(3);
        cache.insert("en".to_string(), engine("en"));
        cache.insert("de".to_string(), engine("de"));
        cache.touch("en");
        assert_eq!(cache.order, ["en".to_string(), "de".to_string()]);
        cache.touch("fr");
        assert_eq!(
            cache.order,
            ["en".to_string(), "de".to_string()],
            "touching an absent code must not enqueue it"
        );
    }

    #[test]
    fn engine_cache_reinsert_keeps_a_single_order_entry() {
        let mut cache = cache_with_capacity(3);
        cache.insert("en".to_string(), engine("en"));
        cache.insert("de".to_string(), engine("de"));
        cache.touch("fr");
        cache.insert("fr".to_string(), engine("fr"));
        assert_eq!(
            cache.order,
            ["fr".to_string(), "de".to_string(), "en".to_string()]
        );
        cache.insert("fr".to_string(), engine("fr"));
        assert_eq!(
            cache.order,
            ["fr".to_string(), "de".to_string(), "en".to_string()],
            "reinserting a present code must not duplicate its order entry"
        );
    }
}
