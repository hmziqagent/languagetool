use axum::body::Body;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use og_api::server::{build_app, shared_state};
use serde_json::Value;
use tower::ServiceExt;

fn app() -> axum::Router {
    build_app(shared_state())
}

async fn check_response(uri: &str) -> Value {
    let response = app()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn has_match_with_rule(json: &Value, rule_id: &str) -> bool {
    json["matches"]
        .as_array()
        .map(|matches| {
            matches
                .iter()
                .any(|m| m["rule"]["id"].as_str() == Some(rule_id))
        })
        .unwrap_or(false)
}

#[tokio::test]
async fn test_v2_check_returns_json() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=en-US&text=Hello+world")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["software"]["name"], "OpenGrammar");
    assert_eq!(json["language"]["code"], "en-US");
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_missing_text() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=en-US")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_v2_check_missing_language() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?text=Hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_v2_check_post_form_body() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v2/check")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("language=en-US&text=I+have+a+apple+here."))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["matches"].is_array());
    assert!(has_match_with_rule(&json, "EN_A_VS_AN"));
}

#[tokio::test]
async fn test_v2_check_post_query_params_override_body() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v2/check?text=I+have+a+apple+here.")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("language=en-US&text=ignored+text"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(has_match_with_rule(&json, "EN_A_VS_AN"));
}

#[tokio::test]
async fn test_v2_check_post_form_body_without_text_is_rejected() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v2/check?language=en-US")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(""))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_v2_check_spelling_match_is_unknown_word_type() {
    let json = check_response("/v2/check?language=en-US&text=I+have+one+appple+here.").await;
    let spelling = json["matches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["rule"]["id"] == "MORFOLOGIK_RULE_EN_US")
        .expect("spelling rule should fire on 'appple'");
    assert_eq!(spelling["type"]["typeName"], "UnknownWord");
    assert_eq!(spelling["rule"]["issueType"], "misspelling");
}

#[tokio::test]
async fn test_v2_check_style_match_is_hint_type() {
    let json =
        check_response("/v2/check?language=en-US&text=This+is+absolutely+essential+work.").await;
    let m = json["matches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["rule"]["id"] == "EN_REDUNDANCY")
        .expect("redundancy rule should fire on 'absolutely essential'");
    assert_eq!(m["type"]["typeName"], "Other");
    assert_eq!(m["rule"]["issueType"], "style");
}

#[tokio::test]
async fn test_v2_check_disabled_rules_suppress_matches() {
    let baseline = check_response("/v2/check?language=en-US&text=I+have+a+apple+here.").await;
    assert!(
        has_match_with_rule(&baseline, "EN_A_VS_AN"),
        "baseline should flag 'a apple'"
    );
    let m = baseline["matches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["rule"]["id"] == "EN_A_VS_AN")
        .unwrap();
    assert!(m["type"].is_object());
    assert_eq!(m["type"]["typeName"], "Other");
    assert_eq!(m["rule"]["category"]["id"], "MISC");
    assert_eq!(m["rule"]["category"]["name"], "Miscellaneous");
    assert!(m["rule"]["category"].get("description").is_none());
    assert_eq!(m["rule"]["issueType"], "misspelling");

    let filtered = check_response(
        "/v2/check?language=en-US&text=I+have+a+apple+here.&disabledRules=EN_A_VS_AN",
    )
    .await;
    assert!(
        !has_match_with_rule(&filtered, "EN_A_VS_AN"),
        "disabledRules must suppress the rule"
    );
}

#[tokio::test]
async fn test_v2_check_enabled_categories_restrict_matches() {
    let filtered = check_response(
        "/v2/check?language=en-US&text=I+have+a+apple+here.&enabledCategories=GRAMMAR",
    )
    .await;
    assert!(
        !has_match_with_rule(&filtered, "EN_A_VS_AN"),
        "EN_A_VS_AN is in category MISC and must not run with enabledCategories=GRAMMAR"
    );

    let kept =
        check_response("/v2/check?language=en-US&text=I+have+a+apple+here.&enabledCategories=MISC")
            .await;
    assert!(
        has_match_with_rule(&kept, "EN_A_VS_AN"),
        "enabledCategories=MISC must keep the rule"
    );
}

#[tokio::test]
async fn test_v2_check_level_and_mother_tongue_accepted() {
    let json = check_response(
        "/v2/check?language=en-US&text=Hello+world&level=picky&motherTongue=de&picky=true",
    )
    .await;
    assert!(json["matches"].is_array());
    assert_eq!(json["language"]["code"], "en-US");
}

#[tokio::test]
async fn test_v2_languages_lists_only_implementable_languages() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/languages")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert!(!json.is_empty());
    assert!(json.iter().any(|l| l["longCode"] == "en-US"));
    assert!(json.iter().any(|l| l["longCode"] == "en-GB"));
    assert!(
        json.iter()
            .any(|l| l["code"] == "en" && l["longCode"] == "en-US")
    );
    assert!(
        json.iter()
            .any(|l| l["code"] == "de" && l["longCode"] == "de-DE"),
        "/v2/languages must advertise the implemented German engine"
    );
    assert!(
        json.iter()
            .any(|l| l["code"] == "fr" && l["longCode"] == "fr"),
        "/v2/languages must advertise the implemented French engine"
    );
    assert!(
        json.iter()
            .any(|l| l["code"] == "es" && l["longCode"] == "es"),
        "/v2/languages must advertise the implemented Spanish engine"
    );
    assert!(
        json.iter()
            .any(|l| l["code"] == "pt" && l["longCode"] == "pt-PT"),
        "/v2/languages must advertise the implemented Portuguese engine"
    );
    assert!(
        json.iter()
            .any(|l| l["code"] == "pt" && l["longCode"] == "pt-BR"),
        "/v2/languages must advertise the implemented pt-BR engine"
    );
    assert!(
        json.iter()
            .any(|l| l["code"] == "ru" && l["longCode"] == "ru"),
        "/v2/languages must advertise the implemented Russian engine"
    );
    assert!(
        json.iter()
            .any(|l| l["code"] == "nl" && l["longCode"] == "nl"),
        "/v2/languages must advertise the implemented Dutch engine"
    );
    assert!(
        json.iter()
            .any(|l| l["code"] == "ca" && l["longCode"] == "ca"),
        "/v2/languages must advertise the implemented Catalan engine"
    );
}

#[tokio::test]
async fn test_v2_version() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["name"], "OpenGrammar");
    assert_eq!(json["apiVersion"], 2);
}

#[tokio::test]
async fn test_v2_check_response_shape() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=en-US&text=This+is+a+test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["software"].is_object());
    assert!(json["software"]["name"].is_string());
    assert!(json["software"]["version"].is_string());
    assert_eq!(json["software"]["apiVersion"], 1);
    assert!(json["language"].is_object());
    assert!(json["language"]["code"].is_string());
    assert!(json["language"]["detectedLanguage"].is_object());
    assert!(json["language"]["detectedLanguage"]["code"].is_string());
    assert!(json["language"]["detectedLanguage"]["name"].is_string());
    assert!(json["language"]["detectedLanguage"]["confidence"].is_number());
    assert!(json["language"].get("detected").is_none());
    assert!(json["matches"].is_array());
    assert_eq!(json["warnings"]["incompleteResults"], false);
}

#[tokio::test]
async fn test_v2_check_auto_detects_german() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=auto&text=Der+schnelle+braune+Fuchs+springt+%C3%BCber+den+faulen+Hund+und+rennt+schnell+davon.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["language"]["detectedLanguage"]["code"].as_str(),
        Some("de-DE"),
        "expected German detection: {json}"
    );
    assert!(
        json["language"]["detectedLanguage"]["confidence"]
            .as_f64()
            .unwrap_or(0.0)
            > 0.9
    );
}

#[tokio::test]
async fn test_v2_check_auto_detects_english() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=auto&text=The+quick+brown+fox+jumps+over+the+lazy+dog+and+then+it+runs+away+quickly.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["language"]["detectedLanguage"]["code"].as_str(),
        Some("en-US"),
        "expected English detection: {json}"
    );
}

#[tokio::test]
async fn test_v2_check_french_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=fr&text=Bonjour+le+monde.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("fr"));
}

#[tokio::test]
async fn test_v2_check_italian_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=it&text=Buongiorno+il+mondo.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("it"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_arabic_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=ar&text=%D9%85%D8%B1%D8%AD%D8%A8%D8%A7+%D8%A8%D8%A7%D9%84%D8%B9%D8%A7%D9%84%D9%85.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("ar"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_asturian_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=ast&text=La+casa+ye+grande.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("ast"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_belarusian_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=be&text=%D0%9F%D1%80%D1%8B%D0%B2%D1%96%D1%82%D0%B0%D0%BD%D0%BD%D0%B5+%D1%81%D1%83%D1%81%D0%B2%D0%B5%D1%82.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("be"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_breton_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=br&text=Lakait+aman%C3%B1+ho+testenn.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("br"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_catalan_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=ca&text=La+casa+%C3%A9s+gran.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("ca"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_crimean_tatar_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=crh&text=%C3%96z%C3%BC+bir+%C3%A7aqr%C4%B1m+uzaqta%2C+demiryol+ke%C3%A7idi+yan%C4%B1nda+ya%C5%9Fay.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("crh"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_danish_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=da&text=Inds%C3%A6t+din+egen+tekst+her.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("da"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_greek_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri(
                    "/v2/check?language=el&text=%CE%91%CF%85%CF%84%CE%AE+%CE%B5%CE%AF%CE%BD%CE%B1%\
                     CE%B9+%CE%BC%CE%B9%CE%B1+%CE%B4%CE%BF%CE%BA%CE%B9%CE%BC%CE%AE.",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("el"));
    assert!(json["matches"].is_array());
}

#[tokio::test]
async fn test_v2_check_esperanto_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=eo&text=La+Mondaj+Ludoj+de+2013+estis+plur-sporta+evento.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("eo"));
    assert!(json["matches"].is_array());
    let flagged: Vec<&str> = json["matches"]
        .as_array()
        .map(|ms| ms.iter().filter_map(|m| m["rule"]["id"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        flagged.contains(&"HUNSPELL_RULE"),
        "EsperantoTest.java:41-44 expects the hunspell speller to flag 'plur-sporta', got \
         {flagged:?}"
    );
}

#[tokio::test]
async fn test_v2_check_persian_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri(
                    "/v2/check?language=fa&text=%D9%88%DB%8C+%D8%AD%D8%A7%D8%B8%D8%B1+%D8%A8%D9%87\
                     +%D9%87%D9%85%DA%A9%D8%A7%D8%B1%DB%8C+%D8%B4%D8%AF.",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("fa"));
    assert!(json["matches"].is_array());
    let flagged: Vec<&str> = json["matches"]
        .as_array()
        .map(|ms| ms.iter().filter_map(|m| m["rule"]["id"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        flagged.contains(&"FA_SIMPLE_REPLACE"),
        "SimpleReplaceRule must flag 'حاظر', got {flagged:?}"
    );
}

#[tokio::test]
async fn test_v2_check_irish_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=ga&text=Seo%20dh%C3%A1%20ab%20d%C3%A9ag.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("ga"));
    assert!(json["matches"].is_array());
    let flagged: Vec<&str> = json["matches"]
        .as_array()
        .map(|ms| ms.iter().filter_map(|m| m["rule"]["id"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        flagged.contains(&"GA_DHA_NO_BEIRT"),
        "DhaNoBeirtRuleTest.java:35 expects GA_DHA_NO_BEIRT on 'dhá ab déag', got {flagged:?}"
    );
}

#[tokio::test]
async fn test_v2_check_galician_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=gl&text=O+problema+%C3%A9+acuciante.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("gl"));
    assert!(json["matches"].is_array());
    let flagged: Vec<&str> = json["matches"]
        .as_array()
        .map(|ms| ms.iter().filter_map(|m| m["rule"]["id"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        flagged.contains(&"GL_CAST_WORDS"),
        "GalicianSimpleReplaceRule must flag the castelanismo 'acuciante', got {flagged:?}"
    );
}

#[tokio::test]
async fn test_v2_check_icelandic_engine_registered() {
    let app = app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v2/check?language=is&text=%C3%9Eetta+er+qvist+setning.")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["language"]["code"].as_str(), Some("is"));
    assert!(json["matches"].is_array());
    let speller: Vec<&serde_json::Value> = json["matches"]
        .as_array()
        .map(|ms| {
            ms.iter()
                .filter(|m| m["rule"]["id"] == "HUNSPELL_NO_SUGGEST_RULE")
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        speller.len(),
        1,
        "HunspellNoSuggestionRule must flag the non-word 'qvist', got {json:?}"
    );
    assert!(
        speller[0]["replacements"]
            .as_array()
            .is_none_or(|r| r.is_empty()),
        "HunspellNoSuggestionRule.java:73-76 never offers replacements"
    );
}

#[tokio::test]
async fn test_v2_languages_lists_all_registered_engines() {
    let json = check_response("/v2/languages").await;
    let codes: Vec<&str> = json
        .as_array()
        .map(|langs| {
            langs
                .iter()
                .filter_map(|l| l["longCode"].as_str())
                .collect()
        })
        .unwrap_or_default();
    for code in [
        "en-US", "de-DE", "fr", "es", "pt-BR", "pt-PT", "pt-AO", "pt-MZ", "nl", "pl", "ru", "it",
        "ar", "ast", "be", "br", "ca", "crh", "da", "el", "eo", "fa", "ga", "gl", "is", "ro", "sv",
    ] {
        assert!(codes.contains(&code), "missing {code} in {codes:?}");
    }
}
