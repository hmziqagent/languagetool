use axum::body::Body;
use axum::routing::get as axum_get;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use og_api::server::{AppState, build_app};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;

/// Full app with the shared language engines (built once per process).
fn app() -> axum::Router {
    build_app(AppState::new())
}

/// App without engines, for endpoints that do not check text
/// (`/v2/words`, `/v2/configinfo`, `/v2/users/me`).
fn bare_app() -> axum::Router {
    build_app(AppState::with_engines(HashMap::new()))
}

/// Send a one-shot request and return `(status, json body)`.
async fn send(app: &axum::Router, request: Request<Body>) -> (StatusCode, serde_json::Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("valid JSON body")
    };
    (status, json)
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, serde_json::Value) {
    send(
        app,
        Request::builder().uri(uri).body(Body::empty()).unwrap(),
    )
    .await
}

/// Rule IDs of every match in a `/v2/check` response.
fn rule_ids(body: &serde_json::Value) -> Vec<String> {
    body["matches"]
        .as_array()
        .map(|matches| {
            matches
                .iter()
                .map(|m| m["rule"]["id"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Text that the always-loaded `WHITESPACE_RULE` text-level rule matches
/// (duplicated space), independent of the LanguageTool resource checkout.
const WHITESPACE_TEXT: &str = "language=en-US&text=Word++word.";
const WHITESPACE_RULE: &str = "WHITESPACE_RULE";
/// One sentence with 31 words: over the engine's `LongSentenceRule::new(30)`
/// threshold. `TOO_LONG_SENTENCE` is default-off ("only on in picky mode"),
/// so it only runs with `picky=true` or `level=picky`.
const LONG_SENTENCE_TEXT: &str = "language=en-US&text=This+sentence+deliberately+contains+many+words+because+the+style+rule+about+excessive+sentence+length+only+runs+when+the+checker+counts+more+than+thirty+alphabetic+word+tokens+inside+one+single+long+sentence.";
const TOO_LONG_SENTENCE_RULE: &str = "TOO_LONG_SENTENCE";

#[tokio::test]
async fn test_v2_check_returns_json() {
    let app = app();
    let (status, json) = get(&app, "/v2/check?language=en-US&text=Hello+world").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["software"]["name"], "OpenGrammar");
    assert_eq!(json["language"]["code"], "en-US");
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_missing_text() {
    let app = app();
    let (status, _) = get(&app, "/v2/check?language=en-US").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_v2_check_missing_language() {
    let app = app();
    let (status, _) = get(&app, "/v2/check?text=Hello").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_v2_check_unsupported_language() {
    let app = app();
    let (status, json) = get(&app, "/v2/check?language=fr-FR&text=Bonjour").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(json["error"].as_str().unwrap().contains("Unsupported"));
}

#[tokio::test]
async fn test_v2_languages() {
    let app = app();
    let (status, json) = get(&app, "/v2/languages").await;

    assert_eq!(status, StatusCode::OK);
    let list = json.as_array().expect("languages array");
    assert!(!list.is_empty());
    assert!(list.iter().any(|l| l["longCode"] == "en-US"));
    assert!(list.iter().any(|l| l["longCode"] == "de-DE"));
    assert!(
        list.iter()
            .any(|l| l["code"] == "en" && l["longCode"] == "en-US")
    );
}

#[tokio::test]
async fn test_v2_version() {
    let app = app();
    let (status, json) = get(&app, "/v2/version").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["name"], "OpenGrammar");
    assert_eq!(json["apiVersion"], 2);
}

#[tokio::test]
async fn test_v2_check_response_shape() {
    let app = app();
    let (_, json) = get(&app, "/v2/check?language=en-US&text=This+is+a+test").await;

    // Verify LT-compatible response structure
    assert!(json["software"].is_object());
    assert!(json["software"]["name"].is_string());
    assert!(json["software"]["version"].is_string());
    assert!(json["software"]["apiVersion"].is_number());
    assert!(json["language"].is_object());
    assert!(json["language"]["code"].is_string());
    assert!(json["language"]["detected"].is_object());
    assert!(json["language"]["detected"]["code"].is_string());
    assert!(json["language"]["detected"]["name"].is_string());
    assert!(json["matches"].is_array());
    assert!(json["warnings"].is_object());
}

#[tokio::test]
async fn test_v2_check_disabled_rules_parameter() {
    let app = app();
    let (_, control) = get(&app, &format!("/v2/check?{WHITESPACE_TEXT}")).await;
    assert!(
        rule_ids(&control).contains(&WHITESPACE_RULE.to_string()),
        "control request should contain {WHITESPACE_RULE}: {:?}",
        rule_ids(&control)
    );

    let (status, filtered) = get(
        &app,
        &format!("/v2/check?{WHITESPACE_TEXT}&disabledRules={WHITESPACE_RULE}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !rule_ids(&filtered).contains(&WHITESPACE_RULE.to_string()),
        "disabledRules must suppress {WHITESPACE_RULE}: {:?}",
        rule_ids(&filtered)
    );
}

#[tokio::test]
async fn test_v2_check_disabled_categories_parameter() {
    let app = app();
    let (status, filtered) = get(
        &app,
        &format!("/v2/check?{WHITESPACE_TEXT}&disabledCategories=TYPOGRAPHY"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !rule_ids(&filtered).contains(&WHITESPACE_RULE.to_string()),
        "disabledCategories=TYPOGRAPHY must suppress {WHITESPACE_RULE}: {:?}",
        rule_ids(&filtered)
    );
}

#[tokio::test]
async fn test_v2_check_enabled_rules_parameter() {
    let app = app();
    let (status, filtered) = get(
        &app,
        &format!("/v2/check?{WHITESPACE_TEXT}&enabledRules={WHITESPACE_RULE}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ids = rule_ids(&filtered);
    assert!(!ids.is_empty(), "enabled rule should have run");
    assert!(
        ids.iter().all(|id| id == WHITESPACE_RULE),
        "only {WHITESPACE_RULE} may run: {ids:?}"
    );
}

#[tokio::test]
async fn test_v2_check_picky_and_level_parameters() {
    let app = app();
    let (_, plain) = get(&app, &format!("/v2/check?{LONG_SENTENCE_TEXT}")).await;
    assert!(
        !rule_ids(&plain).contains(&TOO_LONG_SENTENCE_RULE.to_string()),
        "default-off style rule must not run without picky: {:?}",
        rule_ids(&plain)
    );

    let (_, picky) = get(&app, &format!("/v2/check?{LONG_SENTENCE_TEXT}&picky=true")).await;
    assert!(
        rule_ids(&picky).contains(&TOO_LONG_SENTENCE_RULE.to_string()),
        "picky=true must enable the style rule: {:?}",
        rule_ids(&picky)
    );

    let (_, level) = get(&app, &format!("/v2/check?{LONG_SENTENCE_TEXT}&level=picky")).await;
    assert!(
        rule_ids(&level).contains(&TOO_LONG_SENTENCE_RULE.to_string()),
        "level=picky must enable the style rule: {:?}",
        rule_ids(&level)
    );
}

#[tokio::test]
async fn test_v2_check_mother_tongue_parameter_accepted() {
    let app = app();
    let (status, json) = get(
        &app,
        "/v2/check?language=en-US&motherTongue=de&text=Hello+world",
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["language"]["code"], "en-US");
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_post_form_body() {
    let app = app();
    let (status, json) = send(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/v2/check")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(WHITESPACE_TEXT))
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        rule_ids(&json).contains(&WHITESPACE_RULE.to_string()),
        "form body parameters must be honored: {:?}",
        rule_ids(&json)
    );
}

#[tokio::test]
async fn test_v2_check_post_query_params() {
    let app = app();
    let (status, json) = send(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/v2/check?language=en-US&text=Hello+world")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_words_lifecycle() {
    let app = bare_app();

    let (status, json) = get(&app, "/v2/words").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["words"].as_array().unwrap().len(), 0);

    // Add via form body (as LT clients do).
    let (status, json) = send(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/v2/words")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from("word=hello"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "added");
    assert_eq!(json["word"], "hello");

    let (_, json) = get(&app, "/v2/words").await;
    assert_eq!(json["words"][0], "hello");

    // Adding again reports "exists".
    let (status, json) = send(
        &app,
        Request::builder()
            .method(Method::POST)
            .uri("/v2/words?word=hello")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "exists");

    // Delete.
    let (status, json) = send(
        &app,
        Request::builder()
            .method(Method::DELETE)
            .uri("/v2/words?word=hello")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "deleted");

    // Deleting an absent word is 404.
    let (status, json) = send(
        &app,
        Request::builder()
            .method(Method::DELETE)
            .uri("/v2/words?word=hello")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(json["error"].as_str().unwrap().contains("not found"));

    // Missing word parameter is a 400.
    let (status, _) = send(
        &app,
        Request::builder()
            .method(Method::DELETE)
            .uri("/v2/words")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (_, json) = get(&app, "/v2/words").await;
    assert_eq!(json["words"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn test_v2_words_pagination() {
    let app = bare_app();

    for word in ["alpha", "beta", "gamma"] {
        let (status, _) = send(
            &app,
            Request::builder()
                .method(Method::POST)
                .uri(format!("/v2/words?word={word}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    let (_, json) = get(&app, "/v2/words?offset=1&limit=1").await;
    assert_eq!(json["words"], serde_json::json!(["beta"]));
    assert_eq!(json["offset"], 1);
    assert_eq!(json["limit"], 1);
}

#[tokio::test]
async fn test_v2_configinfo() {
    let app = bare_app();
    let (status, json) = get(&app, "/v2/configinfo").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["maxTextLength"], 100_000);
    assert_eq!(json["maxTextHardLength"], 100_000);
    assert_eq!(json["disabledRules"], serde_json::json!([]));
    assert_eq!(json["disabledCategories"], serde_json::json!([]));
    assert!(json["pipelineCaching"].as_bool().unwrap());
    assert_eq!(json["supportedLanguages"], serde_json::json!([]));
    assert_eq!(json["cacheSize"], 0);
}

#[tokio::test]
async fn test_v2_users_me() {
    let app = bare_app();
    let (status, json) = get(&app, "/v2/users/me").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["id"], 0);
    assert_eq!(json["firstName"], "Anonymous");
    assert_eq!(json["lastName"], "User");
    assert!(!json["isPremium"].as_bool().unwrap());
    assert!(json["isAnonymous"].as_bool().unwrap());
}

/// The engines behind `AppState::new()` are shared per process: building
/// two states must not duplicate the engine instance.
#[tokio::test]
async fn test_engines_shared_across_states() {
    let a = AppState::new();
    let b = AppState::new();
    assert!(
        Arc::ptr_eq(&a.engines["en-US"], &b.engines["en-US"]),
        "engine instances must be shared, not rebuilt"
    );

    let app = build_app(b);
    let (status, json) = get(&app, "/v2/check?language=en-GB&text=Hello").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["language"]["code"], "en-GB");
}

/// A minimal smoke test that the router wires GET and POST to handlers
/// with the expected method routing (GET on /v2/words must be routed,
/// not fall through to 405). The ping handler returns JSON because the
/// `send` helper parses every body as JSON.
#[tokio::test]
async fn test_router_methods() {
    let app = bare_app();
    let app = app.route(
        "/__ping",
        axum_get(|| async { axum::Json(serde_json::json!({ "pong": true })) }),
    );
    let (status, json) = get(&app, "/__ping").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["pong"], true);
}
