//! `og-compat` CLI — the compatibility harness of plans/07.
//!
//! ```txt
//! og-compat check    --rust-url http://127.0.0.1:8081 --language en-US --input FILE
//! og-compat compare  --rust-url URL [--java-url URL | --java-lt JAR] --language en-US --input FILE
//! og-compat fixtures --rust-url URL [--java-url URL] [--fixtures-dir fixtures]
//! og-compat record   --rust-url URL [--fixtures-dir fixtures]
//! ```
//!
//! `check` runs one text through the Rust server and reports every match.
//! `compare` runs the same text through Java LT and the Rust server and diffs
//! them (rule id / message / offset / length / replacements / category).
//! `fixtures` replays every `fixtures/<lang>/*.txt` against the recorded
//! golden outputs (`*.expected_rust.json`, or `*.expected_java.json` when a
//! Java side is given). `record` (re)generates the `expected_rust.json`
//! golden files from a live Rust server.

use og_compat::harness::{Endpoint, JavaLtServer, load_fixtures};
use og_compat::{CompatReport, JavaMatch, compare_java_match_lists};
use std::path::PathBuf;
use std::process::ExitCode;

struct Args(std::collections::HashMap<String, String>);

impl Args {
    fn parse() -> Result<Self, String> {
        let mut map = std::collections::HashMap::new();
        let mut it = std::env::args().skip(1);
        let sub = it
            .next()
            .ok_or_else(|| "missing subcommand (check|compare|fixtures|record)".to_string())?;
        map.insert("subcommand".to_string(), sub);
        while let Some(flag) = it.next() {
            let value = it
                .next()
                .ok_or_else(|| format!("missing value for {flag}"))?;
            map.insert(flag.trim_start_matches('-').to_string(), value);
        }
        Ok(Args(map))
    }

    fn get(&self, name: &str) -> Result<&str, String> {
        self.0
            .get(name)
            .map(|s| s.as_str())
            .ok_or_else(|| format!("missing --{name}"))
    }

    fn opt(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(|s| s.as_str())
    }
}

fn main() -> ExitCode {
    let args = match Args::parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = match args.get("subcommand").unwrap() {
        "check" => cmd_check(&args),
        "compare" => cmd_compare(&args),
        "fixtures" => cmd_fixtures(&args),
        "record" => cmd_record(&args),
        other => Err(format!("unknown subcommand '{other}'")),
    };
    match result {
        Ok(all_passed) => {
            if all_passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_check(args: &Args) -> Result<bool, String> {
    let endpoint = Endpoint::parse(args.get("rust-url")?)?;
    let language = args.get("language")?;
    let input = read_input(args)?;
    let json = endpoint.check(language, &input)?;
    let matches = og_compat::harness::parse_matches(&json)?;
    println!("{} matches for language {language}:", matches.len());
    for m in &matches {
        println!(
            "  {} @ {}..{} ({}): {}",
            m.rule_id,
            m.offset,
            m.offset + m.length,
            m.category,
            m.message
        );
    }
    Ok(true)
}

fn cmd_compare(args: &Args) -> Result<bool, String> {
    let rust = Endpoint::parse(args.get("rust-url")?)?;
    let language = args.get("language")?.to_string();
    let input = read_input(args)?;

    let rust_json = rust.check(&language, &input)?;
    let rust_matches = og_compat::harness::parse_matches(&rust_json)?;

    let java_matches = match (args.opt("java-url"), args.opt("java-lt")) {
        (Some(url), _) => {
            let java = Endpoint::parse(url)?;
            let json = java.check(&language, &input)?;
            og_compat::harness::parse_matches(&json)?
        }
        (None, Some(jar)) => {
            let port: u16 = args
                .opt("java-port")
                .and_then(|p| p.parse().ok())
                .unwrap_or(8082);
            let server = JavaLtServer::spawn(&PathBuf::from(jar), port)?;
            let json = server.endpoint.check(&language, &input)?;
            og_compat::harness::parse_matches(&json)?
        }
        (None, None) => {
            return Err("compare needs --java-url or --java-lt".to_string());
        }
    };

    let results = compare_java_match_lists(&rust_matches, &java_matches);
    print!("{}", CompatReport::new(results).format_report());
    Ok(last_report_passed(&rust_matches, &java_matches))
}

/// fixtures/record share this: replay all fixtures and compare against the
/// golden files recorded for `side` ("rust" or "java").
fn cmd_fixtures(args: &Args) -> Result<bool, String> {
    let rust = Endpoint::parse(args.get("rust-url")?)?;
    let root = PathBuf::from(args.opt("fixtures-dir").unwrap_or("fixtures"));
    let side = if args.opt("java-url").is_some() || args.opt("java-lt").is_some() {
        "java"
    } else {
        "rust"
    };
    let java = args.opt("java-url").map(Endpoint::parse).transpose()?;
    let java_server = match args.opt("java-lt") {
        Some(jar) => {
            let port: u16 = args
                .opt("java-port")
                .and_then(|p| p.parse().ok())
                .unwrap_or(8082);
            Some(JavaLtServer::spawn(&PathBuf::from(jar), port)?)
        }
        None => None,
    };

    let fixtures = load_fixtures(&root);
    if fixtures.is_empty() {
        return Err(format!("no fixtures found under {}", root.display()));
    }

    let mut all_passed = true;
    for fixture in &fixtures {
        let language = fixture.language();
        let live = if side == "java" {
            let endpoint = java
                .as_ref()
                .or(java_server.as_ref().map(|s| &s.endpoint))
                .expect("java side configured");
            endpoint.check(&language, &fixture.input)?
        } else {
            rust.check(&language, &fixture.input)?
        };
        let live_matches = og_compat::harness::parse_matches(&live)?;
        let expected_path = fixture.expected_path(side);
        let expected_json = std::fs::read_to_string(&expected_path)
            .map_err(|e| format!("missing golden file {}: {}", expected_path.display(), e))?;
        let expected: Vec<JavaMatch> = og_compat::harness::parse_matches(&expected_json)?;
        let results = compare_java_match_lists(&live_matches, &expected);
        let report = CompatReport::new(results);
        let name = fixture.input_file.display();
        if report.all_passed() {
            println!("PASS: {name}");
        } else {
            all_passed = false;
            println!(
                "{}",
                report
                    .format_report()
                    .replace("Compatibility Report", &format!("FAIL: {name}"))
            );
        }
    }
    Ok(all_passed)
}

fn cmd_record(args: &Args) -> Result<bool, String> {
    let rust = Endpoint::parse(args.get("rust-url")?)?;
    let root = PathBuf::from(args.opt("fixtures-dir").unwrap_or("fixtures"));
    let fixtures = load_fixtures(&root);
    if fixtures.is_empty() {
        return Err(format!("no fixtures found under {}", root.display()));
    }
    for fixture in &fixtures {
        let json = rust.check(&fixture.language(), &fixture.input)?;
        // Keep the server's own LT-shaped `matches` array verbatim so the
        // golden file round-trips through `parse_matches` unchanged.
        let value: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| format!("server response not JSON: {}", e))?;
        let matches = value
            .get("matches")
            .cloned()
            .ok_or("server response missing 'matches'")?;
        let body = serde_json::json!({ "matches": matches });
        let path = fixture.expected_path("rust");
        std::fs::write(&path, serde_json::to_string_pretty(&body).unwrap())
            .map_err(|e| format!("write {}: {}", path.display(), e))?;
        let count = matches.as_array().map(Vec::len).unwrap_or(0);
        println!("recorded {} ({} matches)", path.display(), count);
    }
    Ok(true)
}

fn read_input(args: &Args) -> Result<String, String> {
    match (args.opt("input"), args.opt("text")) {
        (Some(path), _) => {
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path, e))
        }
        (None, Some(text)) => Ok(text.to_string()),
        (None, None) => Err("missing --input FILE (or --text)".to_string()),
    }
}

/// Whether the last compare actually passed (report prints FAIL lines).
fn last_report_passed(rust: &[JavaMatch], java: &[JavaMatch]) -> bool {
    let results = compare_java_match_lists(rust, java);
    CompatReport::new(results).all_passed()
}
