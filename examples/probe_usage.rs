//! One-shot probe binary: fetch `/api/oauth/usage` with the user's local OAuth
//! token and print the raw JSON body.
//!
//! Goal: confirm the real response shape so T01 can decide whether the
//! existing `ApiUsageResponse` schema covers everything (it widens with
//! `#[serde(flatten)] extra` regardless, but we want a sample to inspect).
//!
//! Usage:
//!     cargo run --example probe_usage
//!     cargo run --example probe_usage -- --out docs/api-samples/usage-response.json
//!     cargo run --example probe_usage -- --pretty
//!
//! The token is read via the same credential path the statusline uses
//! (macOS Keychain or `~/.claude/.credentials.json`).
//!
//! When `--out` is provided, the body is written to the given path and a
//! short summary is printed to stderr. Otherwise it goes to stdout.
//!
//! Remember to redact `resets_at` timestamps before committing a sample.

use std::time::Duration;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const USER_AGENT: &str = "claude-code";
const ANTHROPIC_BETA: &str = "oauth-2025-04-20";

fn main() {
    let (out_path, pretty) = parse_args();

    let token = ccometixline::utils::credentials::get_oauth_token()
        .unwrap_or_else(|| die("no OAuth token found via utils::credentials"));

    let body = fetch_body(&token);

    let formatted = if pretty {
        match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or(body),
            Err(_) => body,
        }
    } else {
        body
    };

    match out_path {
        Some(path) => {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&path, &formatted)
                .unwrap_or_else(|e| die(&format!("write {}: {}", path, e)));
            eprintln!("wrote {} bytes to {}", formatted.len(), path);
        }
        None => println!("{}", formatted),
    }
}

fn parse_args() -> (Option<String>, bool) {
    let mut out: Option<String> = None;
    let mut pretty = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = Some(args.next().unwrap_or_else(|| die("--out needs a path"))),
            "--pretty" => pretty = true,
            "-h" | "--help" => {
                eprintln!("usage: cargo run --example probe_usage -- [--out PATH] [--pretty]");
                std::process::exit(0);
            }
            other => die(&format!("unknown argument: {}", other)),
        }
    }
    (out, pretty)
}

fn fetch_body(token: &str) -> String {
    let agent = ureq::Agent::new_with_defaults();
    let response = agent
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {}", token))
        .header("anthropic-beta", ANTHROPIC_BETA)
        .header("User-Agent", USER_AGENT)
        .config()
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .call()
        .unwrap_or_else(|e| die(&format!("HTTP call failed: {}", e)));

    response
        .into_body()
        .read_to_string()
        .unwrap_or_else(|e| die(&format!("read body failed: {}", e)))
}

fn die(msg: &str) -> ! {
    eprintln!("probe_usage: {}", msg);
    std::process::exit(1);
}
