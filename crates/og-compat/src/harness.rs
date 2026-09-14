//! Compatibility harness: drives a running Rust OpenGrammar server (and,
//! optionally, a Java LanguageTool server) over HTTP and compares their
//! `/v2/check` outputs (plans/07 "Compatibility harness CLI").
//!
//! The HTTP client is a minimal dependency-free GET over `std::net::TcpStream`
//! — the harness only ever talks to plain HTTP on localhost.

use crate::compare::JavaMatch;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A server endpoint the harness can query.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
}

impl Endpoint {
    /// Parse `http://host:port` or `host:port`.
    pub fn parse(url: &str) -> Result<Self, String> {
        let rest = url.strip_prefix("http://").unwrap_or(url);
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let (host, port) = rest
            .rsplit_once(':')
            .ok_or_else(|| format!("invalid endpoint '{}': expected host:port", url))?;
        let port: u16 = port
            .parse()
            .map_err(|_| format!("invalid port in '{}'", url))?;
        Ok(Endpoint {
            host: host.to_string(),
            port,
        })
    }

    /// Wait until a TCP connection to the endpoint succeeds (bind-then-log
    /// ready behavior of `og-server`), up to `timeout`.
    pub fn wait_ready(&self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            if TcpStream::connect((self.host.as_str(), self.port)).is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "endpoint {}:{} not reachable within {:?}",
                    self.host, self.port, timeout
                ));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Run a `/v2/check` request and return the raw response body.
    pub fn check(&self, language: &str, text: &str) -> Result<String, String> {
        let path = format!(
            "/v2/check?language={}&text={}",
            percent_encode(language),
            percent_encode(text)
        );
        self.get(&path)
    }

    fn get(&self, path: &str) -> Result<String, String> {
        let mut stream = TcpStream::connect((self.host.as_str(), self.port))
            .map_err(|e| format!("connect {}:{}: {}", self.host, self.port, e))?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\nAccept: application/json\r\n\r\n",
            path, self.host, self.port
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| format!("write: {}", e))?;
        let mut body = String::new();
        stream
            .read_to_string(&mut body)
            .map_err(|e| format!("read: {}", e))?;
        split_http_body(&body)
    }
}

/// Split headers from the body of a raw HTTP/1.x response.
fn split_http_body(raw: &str) -> Result<String, String> {
    match raw.find("\r\n\r\n") {
        Some(idx) => Ok(raw[idx + 4..].to_string()),
        None => Err("malformed HTTP response (no header/body separator)".to_string()),
    }
}

/// Percent-encode everything outside the unreserved set.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// A Java LanguageTool server spawned as a subprocess
/// (`java -cp <jar> org.languagetool.server.HTTPServer --port <port>`).
/// Killed on drop.
pub struct JavaLtServer {
    child: Child,
    pub endpoint: Endpoint,
}

impl JavaLtServer {
    pub fn spawn(jar: &Path, port: u16) -> Result<Self, String> {
        let child = Command::new("java")
            .arg("-cp")
            .arg(jar)
            .arg("org.languagetool.server.HTTPServer")
            .arg("--port")
            .arg(port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn java (is java installed?): {}", e))?;
        let endpoint = Endpoint {
            host: "127.0.0.1".to_string(),
            port,
        };
        endpoint.wait_ready(Duration::from_secs(60))?;
        Ok(Self { child, endpoint })
    }
}

impl Drop for JavaLtServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Parse the `matches` array of a `/v2/check`-shaped JSON body into
/// [`JavaMatch`]es (shared by the golden-fixture comparison and the CLI).
pub fn parse_matches(json: &str) -> Result<Vec<JavaMatch>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {}", e))?;
    let arr = value
        .get("matches")
        .and_then(|m| m.as_array())
        .ok_or_else(|| "JSON missing 'matches' array".to_string())?;
    Ok(arr
        .iter()
        .filter_map(|m| {
            Some(JavaMatch {
                rule_id: m.get("rule")?.get("id")?.as_str()?.to_string(),
                message: m.get("message")?.as_str()?.to_string(),
                offset: m.get("offset")?.as_u64()? as usize,
                length: m.get("length")?.as_u64()? as usize,
                replacements: m
                    .get("replacements")
                    .and_then(|r| r.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.get("value")?.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                category: m
                    .get("rule")?
                    .get("category")?
                    .get("id")?
                    .as_str()?
                    .to_string(),
                issue_type: m
                    .get("rule")?
                    // LT uses camelCase (`issueType`); our server currently
                    // serializes snake_case (`issue_type`) — accept both.
                    .get("issueType")
                    .or_else(|| m.get("rule")?.get("issue_type"))?
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect())
}

/// One golden fixture: an input text file plus its expected-matches JSON.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// Directory of the input file, e.g. `fixtures/en`.
    pub dir: PathBuf,
    /// Input text (already read from `<stem>.txt`).
    pub input: String,
    /// `<stem>.txt` file name, e.g. `common_errors.txt`.
    pub input_file: PathBuf,
}

impl Fixture {
    /// Expected-output JSON file for a recording side (`rust` or `java`).
    pub fn expected_path(&self, side: &str) -> PathBuf {
        let mut name = self
            .input_file
            .file_stem()
            .unwrap_or_default()
            .to_os_string();
        name.push(format!(".expected_{side}.json"));
        self.dir.join(name)
    }

    /// Language code = fixture directory name (`fixtures/en` -> `en`).
    pub fn language(&self) -> String {
        self.dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    }
}

/// Discover all `<lang>/*.txt` fixtures under `root` (skips `*.json`).
pub fn load_fixtures(root: &Path) -> Vec<Fixture> {
    let mut fixtures = Vec::new();
    let Ok(langs) = std::fs::read_dir(root) else {
        return fixtures;
    };
    for lang_dir in langs.flatten() {
        let dir = lang_dir.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().is_none_or(|e| e != "txt") {
                continue;
            }
            if let Ok(input) = std::fs::read_to_string(&path) {
                fixtures.push(Fixture {
                    dir: dir.clone(),
                    input,
                    input_file: path,
                });
            }
        }
    }
    fixtures.sort_by(|a, b| a.input_file.cmp(&b.input_file));
    fixtures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_parsing() {
        let e = Endpoint::parse("http://127.0.0.1:8081").unwrap();
        assert_eq!((e.host.as_str(), e.port), ("127.0.0.1", 8081));
        let e = Endpoint::parse("localhost:1234/").unwrap();
        assert_eq!((e.host.as_str(), e.port), ("localhost", 1234));
        assert!(Endpoint::parse("no-port").is_err());
    }

    #[test]
    fn percent_encoding() {
        assert_eq!(percent_encode("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(percent_encode("Word++word."), "Word%2B%2Bword.");
    }

    #[test]
    fn http_body_split() {
        let raw = "HTTP/1.1 200 OK\r\nContent-Type: json\r\n\r\n{\"matches\": []}";
        assert_eq!(split_http_body(raw).unwrap(), "{\"matches\": []}");
    }

    #[test]
    fn parse_matches_extracts_lt_fields() {
        let json = r#"{"matches":[{
            "rule": {"id": "R1", "category": {"id": "GRAMMAR"}, "issueType": "grammar"},
            "message": "msg",
            "offset": 3, "length": 4,
            "replacements": [{"value": "fix"}]
        }]}"#;
        let matches = parse_matches(json).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].rule_id, "R1");
        assert_eq!(matches[0].category, "GRAMMAR");
        assert_eq!(matches[0].issue_type, "grammar");
        assert_eq!(matches[0].replacements, vec!["fix".to_string()]);
    }

    #[test]
    fn parse_matches_empty() {
        assert!(parse_matches(r#"{"matches":[]}"#).unwrap().is_empty());
        assert!(parse_matches("{}").is_err());
    }

    #[test]
    fn fixture_discovery_and_paths() {
        let root = std::env::temp_dir().join("og_compat_fixture_test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("en")).unwrap();
        std::fs::write(root.join("en/common_errors.txt"), "Their is a problem.").unwrap();
        let fixtures = load_fixtures(&root);
        assert_eq!(fixtures.len(), 1);
        let f = &fixtures[0];
        assert_eq!(f.language(), "en");
        assert_eq!(
            f.expected_path("rust"),
            root.join("en/common_errors.expected_rust.json")
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
