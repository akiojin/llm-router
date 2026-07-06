//! 安全な curl プロキシ機構: コマンドサニタイズ・URL/ホスト検証・
//! 認証ヘッダ注入・サブプロセス実行・出力パース
//!
//! arch-review [H6] round2: cli/assistant.rs から curl 実行機構を分離。

use super::{AssistantConfig, CurlArgs, MAX_TIMEOUT_SECS, MIN_TIMEOUT_SECS};
use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Url;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;

const STATUS_MARKER: &str = "__STATUS_CODE__";

const LOCALHOST_HOSTNAMES: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

static FORBIDDEN_OPTIONS: [&str; 21] = [
    "-o",
    "--output",
    "-O",
    "--remote-name",
    "-K",
    "--config",
    "-q",
    "--disable",
    "-u",
    "--user",
    "--netrc",
    "--netrc-file",
    "--netrc-optional",
    "--delegation",
    "--libcurl",
    "--trace",
    "--trace-ascii",
    "--trace-time",
    "--proto",
    "--proto-default",
    "--proto-redir",
];

static FORBIDDEN_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"[;&|`]").expect("valid regex"),
        Regex::new(r"\$\(").expect("valid regex"),
        Regex::new(r"\$\{").expect("valid regex"),
        Regex::new(r">\s*[/~]").expect("valid regex"),
        Regex::new(r">>").expect("valid regex"),
        Regex::new(r"\|\s*\w+").expect("valid regex"),
        Regex::new(r"<\s*[/~]").expect("valid regex"),
        Regex::new(r"\\n").expect("valid regex"),
    ]
});

pub(super) fn sanitize_command(command: &str) -> std::result::Result<(), String> {
    let trimmed = command.trim();

    if !(trimmed.starts_with("curl ") || trimmed == "curl") {
        return Err("Command must start with \"curl \"".to_string());
    }

    for pattern in FORBIDDEN_PATTERNS.iter() {
        if pattern.is_match(trimmed) {
            return Err("Forbidden pattern detected: potential shell injection".to_string());
        }
    }

    let tokens = tokenize(trimmed);
    for token in tokens {
        for opt in FORBIDDEN_OPTIONS {
            if token == opt {
                return Err(format!("Forbidden option: {opt}"));
            }

            if opt.starts_with('-') && !opt.starts_with("--") && token.starts_with(opt) {
                return Err(format!("Forbidden option: {opt}"));
            }

            if opt.starts_with("--") && token.starts_with(&format!("{opt}=")) {
                return Err(format!("Forbidden option: {opt}"));
            }
        }
    }

    Ok(())
}

pub(super) fn extract_url(command: &str) -> Option<String> {
    let tokens = tokenize(command);
    let no_value_long_options: HashSet<&str> = [
        "--compressed",
        "--silent",
        "-s",
        "-S",
        "-i",
        "--include",
        "-v",
        "--verbose",
        "-L",
        "--location",
    ]
    .into_iter()
    .collect();

    let mut skip_next = false;
    for token in tokens {
        if skip_next {
            skip_next = false;
            continue;
        }

        if token.starts_with('-') && !token.starts_with("--") && token.len() == 2 {
            skip_next = true;
            continue;
        }

        if token.starts_with("--")
            && !token.contains('=')
            && !no_value_long_options.contains(token.as_str())
        {
            skip_next = true;
            continue;
        }

        if !token.starts_with('-')
            && token != "curl"
            && (token.starts_with("http://") || token.starts_with("https://"))
        {
            return Some(token);
        }
    }

    None
}

pub(super) fn validate_host(target_url: &str, router_url: &Url) -> std::result::Result<(), String> {
    let url = Url::parse(target_url).map_err(|_| format!("Invalid URL: {target_url}"))?;

    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "Invalid protocol: {other}. Only http/https allowed."
            ));
        }
    }

    if let Some(host) = url.host_str() {
        if LOCALHOST_HOSTNAMES.iter().any(|name| name == &host) {
            return Ok(());
        }
    }

    let allowed_host = host_with_optional_port(router_url)
        .ok_or_else(|| "Failed to resolve allowed router host".to_string())?;
    let target_host =
        host_with_optional_port(&url).ok_or_else(|| format!("Invalid URL: {target_url}"))?;

    if target_host != allowed_host {
        return Err(format!(
            "Host not allowed: {target_host}. Allowed: {allowed_host}, {}, {}, {}",
            LOCALHOST_HOSTNAMES[0], LOCALHOST_HOSTNAMES[1], LOCALHOST_HOSTNAMES[2]
        ));
    }

    Ok(())
}

pub(super) fn host_with_optional_port(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let value = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    Some(value)
}

pub(super) fn safe_pathname(url: &str) -> String {
    Url::parse(url)
        .map(|parsed| parsed.path().to_string())
        .unwrap_or_else(|_| url.to_string())
}

pub(super) fn inject_auth_headers(command: &str, url: &str, config: &AssistantConfig) -> String {
    if command.contains("Authorization:") || command.contains("X-API-Key:") {
        return command.to_string();
    }

    let pathname = safe_pathname(url);
    let is_auth_endpoint = pathname.starts_with("/api/auth/");
    let is_management_endpoint = pathname.starts_with("/api/");
    let is_inference_endpoint = pathname.starts_with("/v1/");

    let auth_header = if is_auth_endpoint {
        config
            .jwt_token
            .as_ref()
            .map(|token| format!("-H \"Authorization: Bearer {token}\""))
    } else if is_management_endpoint {
        config
            .admin_api_key
            .as_ref()
            .map(|key| format!("-H \"X-API-Key: {key}\""))
            .or_else(|| {
                config
                    .jwt_token
                    .as_ref()
                    .map(|token| format!("-H \"Authorization: Bearer {token}\""))
            })
            .or_else(|| {
                config
                    .api_key
                    .as_ref()
                    .map(|key| format!("-H \"X-API-Key: {key}\""))
            })
    } else if is_inference_endpoint {
        config
            .api_key
            .as_ref()
            .map(|key| format!("-H \"X-API-Key: {key}\""))
            .or_else(|| {
                config
                    .admin_api_key
                    .as_ref()
                    .map(|key| format!("-H \"X-API-Key: {key}\""))
            })
    } else {
        None
    };

    match auth_header {
        Some(header) => command.replacen("curl ", &format!("curl {header} "), 1),
        None => command.to_string(),
    }
}

pub(super) async fn execute_curl_command(
    command: &str,
    timeout_secs: u64,
) -> Result<(Option<u16>, Option<Value>, bool)> {
    let mut args = parse_curl_args(command)?;
    args.push("-s".to_string());
    args.push("-S".to_string());
    args.push("-w".to_string());
    args.push(format!("\\n{STATUS_MARKER}:%{{http_code}}"));

    let child = Command::new("curl")
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn curl")?;

    let output = timeout(Duration::from_secs(timeout_secs), child.wait_with_output())
        .await
        .map_err(|_| anyhow!("curl timed out after {timeout_secs} seconds"))?
        .context("failed to read curl output")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() && stdout.trim().is_empty() {
        let message = if stderr.is_empty() {
            format!("curl exited with status {}", output.status)
        } else {
            stderr
        };
        return Err(anyhow!(message));
    }

    let (status_code, body_text) = split_status_and_body(&stdout);
    let parsed_body = if body_text.trim().is_empty() {
        None
    } else if let Ok(value) = serde_json::from_str::<Value>(body_text.trim()) {
        Some(value)
    } else {
        Some(Value::String(body_text.trim().to_string()))
    };

    let success = status_code
        .map(|code| (200..=299).contains(&code))
        .unwrap_or(false);

    Ok((status_code, parsed_body, success))
}

pub(super) fn split_status_and_body(stdout: &str) -> (Option<u16>, String) {
    let marker = format!("{STATUS_MARKER}:");
    let Some(index) = stdout.rfind(&marker) else {
        return (None, stdout.trim().to_string());
    };

    let body = stdout[..index].trim().to_string();
    let status_text = stdout[index + marker.len()..].trim();
    let status = status_text.parse::<u16>().ok();
    (status, body)
}

pub(super) fn parse_curl_args(command: &str) -> Result<Vec<String>> {
    let tokens = tokenize(command);
    if tokens.is_empty() || tokens[0] != "curl" {
        return Err(anyhow!("command must start with curl"));
    }
    Ok(tokens.into_iter().skip(1).collect())
}

pub(super) fn tokenize(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut quote_char = '\0';

    for ch in command.chars() {
        if (ch == '"' || ch == '\'') && !in_quote {
            in_quote = true;
            quote_char = ch;
            continue;
        }

        if in_quote && ch == quote_char {
            in_quote = false;
            quote_char = '\0';
            continue;
        }

        if ch.is_whitespace() && !in_quote {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            continue;
        }

        current.push(ch);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

pub(super) fn mask_sensitive(command: &str) -> String {
    static BEARER_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"Bearer\s+[^\s"']+"#).expect("valid regex"));
    static API_KEY_HEADER_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"X-API-Key:\s*[^\s"']+"#).expect("valid regex"));
    static SK_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"sk_[A-Za-z0-9]+").expect("valid regex"));

    let masked = BEARER_RE.replace_all(command, "Bearer ***");
    let masked = API_KEY_HEADER_RE.replace_all(&masked, "X-API-Key: ***");
    SK_RE.replace_all(&masked, "sk_***").to_string()
}

#[derive(Debug, Serialize)]
pub(super) struct CurlResult {
    pub(super) success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<String>,
    pub(super) duration_ms: u128,
    pub(super) executed_command: String,
}

pub(super) async fn execute_curl(args: &CurlArgs, config: &AssistantConfig) -> Result<()> {
    let start = Instant::now();
    let timeout_secs = args
        .timeout
        .unwrap_or(config.default_timeout)
        .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);

    if let Err(reason) = sanitize_command(&args.command) {
        let result = CurlResult {
            success: false,
            status_code: None,
            body: None,
            error: Some(format!("Security violation: {reason}")),
            duration_ms: start.elapsed().as_millis(),
            executed_command: mask_sensitive(&args.command),
        };
        print_curl_result(&result, args.json)?;
        return Err(curl_failure_error(&result));
    }

    let Some(url) = extract_url(&args.command) else {
        let result = CurlResult {
            success: false,
            status_code: None,
            body: None,
            error: Some("Could not extract URL from curl command".to_string()),
            duration_ms: start.elapsed().as_millis(),
            executed_command: mask_sensitive(&args.command),
        };
        print_curl_result(&result, args.json)?;
        return Err(curl_failure_error(&result));
    };

    if let Err(reason) = validate_host(&url, &config.router_url) {
        let result = CurlResult {
            success: false,
            status_code: None,
            body: None,
            error: Some(reason),
            duration_ms: start.elapsed().as_millis(),
            executed_command: mask_sensitive(&args.command),
        };
        print_curl_result(&result, args.json)?;
        return Err(curl_failure_error(&result));
    }

    let command = if args.no_auto_auth {
        args.command.clone()
    } else {
        inject_auth_headers(&args.command, &url, config)
    };

    let exec_result = execute_curl_command(&command, timeout_secs).await;
    let duration_ms = start.elapsed().as_millis();

    let result = match exec_result {
        Ok((status_code, body, success)) => CurlResult {
            success,
            status_code,
            body,
            error: None,
            duration_ms,
            executed_command: mask_sensitive(&command),
        },
        Err(error) => CurlResult {
            success: false,
            status_code: None,
            body: None,
            error: Some(error.to_string()),
            duration_ms,
            executed_command: mask_sensitive(&command),
        },
    };

    print_curl_result(&result, args.json)?;
    if !result.success {
        return Err(curl_failure_error(&result));
    }

    Ok(())
}

pub(super) fn print_curl_result(result: &CurlResult, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(result)?);
        return Ok(());
    }

    if result.success {
        if let Some(status) = result.status_code {
            println!("Success (HTTP {status}, {} ms)", result.duration_ms);
        } else {
            println!("Success ({} ms)", result.duration_ms);
        }
    } else {
        let message = result.error.as_deref().unwrap_or("unknown error");
        println!("Error: {message}");
    }

    if let Some(body) = &result.body {
        match body {
            Value::String(text) => println!("{text}"),
            _ => println!("{}", serde_json::to_string_pretty(body)?),
        }
    }

    Ok(())
}

pub(super) fn curl_failure_error(result: &CurlResult) -> anyhow::Error {
    if let Some(message) = result.error.as_deref() {
        return anyhow!(message.to_string());
    }

    if let Some(status_code) = result.status_code {
        return anyhow!(format!(
            "HTTP request failed with status code {status_code}"
        ));
    }

    anyhow!("assistant curl command failed")
}
