use og_api::server::{DEFAULT_PORT, run_server};

/// Resolve the listen port: `--port N` (or `--port=N`) command-line
/// argument wins, then the `OG_API_PORT` environment variable, then the
/// LanguageTool-compatible default 8081.
fn resolve_port() -> u16 {
    let mut port = std::env::var("OG_API_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT);

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--port=") {
            if let Ok(parsed) = value.parse::<u16>() {
                port = parsed;
            }
        } else if arg == "--port"
            && let Some(value) = args.next()
            && let Ok(parsed) = value.parse::<u16>()
        {
            port = parsed;
        }
    }
    port
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let port = resolve_port();
    if let Err(e) = run_server(port).await {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    }
}
