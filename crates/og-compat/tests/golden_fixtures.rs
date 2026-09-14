//! Golden fixture tests (plans/07 "Full /v2/check golden tests").
//!
//! Replays every `fixtures/<lang>/*.txt` through the real og-server pipeline
//! (in-process via `og_api::server::build_app`, the same shared engines the
//! binary serves) and compares the matches against the recorded
//! `*.expected_rust.json` golden files.
//!
//! The goldens were recorded with the upstream LanguageTool resource checkout
//! present (the engine loads the real English grammar.xml from it), so this
//! test skips itself when `og_spell::resource::lt_resource_root()` is `None`.

use axum::body::Body;
use http::Request;
use http_body_util::BodyExt;
use og_api::server::{AppState, build_app};
use tower::ServiceExt;

fn fixtures_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn check(app: &axum::Router, language: &str, text: &str) -> String {
    let uri = format!(
        "/v2/check?language={}&text={}",
        percent_encode(language),
        percent_encode(text)
    );
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn golden_fixtures_match_expected_rust_output() {
    if og_spell::resource::lt_resource_root().is_none() {
        eprintln!("skipping: no LT resource root (goldens recorded with resources)");
        return;
    }

    let fixtures = og_compat::load_fixtures(&fixtures_root());
    assert!(
        !fixtures.is_empty(),
        "no fixtures found under {}",
        fixtures_root().display()
    );

    let app = build_app(AppState::new());
    for fixture in &fixtures {
        let live = check(&app, &fixture.language(), &fixture.input).await;
        let expected_path = fixture.expected_path("rust");
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|e| panic!("read {}: {}", expected_path.display(), e));
        let results = og_compat::compare_json_results(&live, &expected);
        let report = og_compat::CompatReport::new(results);
        assert!(
            report.all_passed(),
            "fixture {} diverged:\n{}",
            fixture.input_file.display(),
            report.format_report()
        );
    }
}
