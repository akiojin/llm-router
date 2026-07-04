//! assistant subcommand
//!
//! Provides helper functionality previously available in the legacy MCP server as a CLI.

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Url;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;

const STATUS_MARKER: &str = "__STATUS_CODE__";
const DEFAULT_ROUTER_URL: &str = "http://localhost:32768";
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 300;
const MIN_TIMEOUT_SECS: u64 = 1;

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

/// Arguments for the assistant subcommand
#[derive(Args, Debug, Clone)]
pub struct AssistantArgs {
    /// Assistant helper subcommand
    #[command(subcommand)]
    pub command: AssistantCommand,
}

/// Assistant subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum AssistantCommand {
    /// Execute curl command with safety checks and optional auth injection
    Curl(CurlArgs),
    /// Print OpenAPI spec (JSON)
    Openapi(OpenApiArgs),
    /// Print API guide text
    Guide(GuideArgs),
}

/// Arguments for `assistant curl`
#[derive(Args, Debug, Clone)]
pub struct CurlArgs {
    /// curl command to execute
    #[arg(long)]
    pub command: String,

    /// Disable automatic auth header injection
    #[arg(long, default_value_t = false)]
    pub no_auto_auth: bool,

    /// Request timeout in seconds (1-300)
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Output as JSON (compatible with automation)
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// Arguments for `assistant openapi`
#[derive(Args, Debug, Clone)]
pub struct OpenApiArgs {
    /// Path to OpenAPI file (YAML/JSON)
    #[arg(long)]
    pub path: Option<PathBuf>,
}

/// Arguments for `assistant guide`
#[derive(Args, Debug, Clone)]
pub struct GuideArgs {
    /// Guide category
    #[arg(long, value_enum)]
    pub category: GuideCategory,
}

/// Guide categories
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum GuideCategory {
    /// API overview and auth notes
    Overview,
    /// OpenAI-compatible /v1/* APIs
    #[value(name = "openai-compatible")]
    OpenAiCompatible,
    /// /api/endpoints APIs
    #[value(name = "endpoint-management")]
    EndpointManagement,
    /// /api/models/* APIs
    #[value(name = "model-management")]
    ModelManagement,
    /// /api/dashboard/* APIs
    Dashboard,
}

#[derive(Debug, Clone)]
struct AssistantConfig {
    router_url: Url,
    api_key: Option<String>,
    admin_api_key: Option<String>,
    jwt_token: Option<String>,
    openapi_path: Option<PathBuf>,
    default_timeout: u64,
}

impl AssistantConfig {
    fn from_env() -> Result<Self> {
        let router_url_raw =
            std::env::var("LLMLB_URL").unwrap_or_else(|_| DEFAULT_ROUTER_URL.to_string());
        let router_url = Url::parse(&router_url_raw)
            .with_context(|| format!("invalid LLMLB_URL: {router_url_raw}"))?;

        let default_timeout = std::env::var("LLMLB_TIMEOUT")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS);

        Ok(Self {
            router_url,
            api_key: std::env::var("LLMLB_API_KEY")
                .ok()
                .filter(|v| !v.is_empty()),
            admin_api_key: std::env::var("LLMLB_ADMIN_API_KEY")
                .ok()
                .filter(|v| !v.is_empty()),
            jwt_token: std::env::var("LLMLB_JWT_TOKEN")
                .ok()
                .filter(|v| !v.is_empty()),
            openapi_path: std::env::var("LLMLB_OPENAPI_PATH").ok().map(PathBuf::from),
            default_timeout,
        })
    }
}

#[derive(Debug, Serialize)]
struct CurlResult {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_code: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    duration_ms: u128,
    executed_command: String,
}

/// Execute an assistant command
pub async fn execute(command: &AssistantCommand) -> Result<()> {
    let config = AssistantConfig::from_env()?;
    match command {
        AssistantCommand::Curl(args) => execute_curl(args, &config).await,
        AssistantCommand::Openapi(args) => execute_openapi(args, &config),
        AssistantCommand::Guide(args) => execute_guide(args, &config),
    }
}

async fn execute_curl(args: &CurlArgs, config: &AssistantConfig) -> Result<()> {
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

fn execute_openapi(args: &OpenApiArgs, config: &AssistantConfig) -> Result<()> {
    let json_value = load_openapi_value(args.path.as_ref(), config.openapi_path.as_ref());
    let text = serde_json::to_string_pretty(&json_value)?;
    println!("{text}");
    Ok(())
}

fn execute_guide(args: &GuideArgs, config: &AssistantConfig) -> Result<()> {
    let text = match args.category {
        GuideCategory::Overview => overview_guide(config.router_url.as_str()),
        GuideCategory::OpenAiCompatible => openai_guide(config.router_url.as_str()),
        GuideCategory::EndpointManagement => endpoint_management_guide(config.router_url.as_str()),
        GuideCategory::ModelManagement => model_management_guide(config.router_url.as_str()),
        GuideCategory::Dashboard => dashboard_guide(config.router_url.as_str()),
    };
    println!("{text}");
    Ok(())
}

fn print_curl_result(result: &CurlResult, as_json: bool) -> Result<()> {
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

fn curl_failure_error(result: &CurlResult) -> anyhow::Error {
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

fn sanitize_command(command: &str) -> std::result::Result<(), String> {
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

fn extract_url(command: &str) -> Option<String> {
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

fn validate_host(target_url: &str, router_url: &Url) -> std::result::Result<(), String> {
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

fn host_with_optional_port(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let value = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    Some(value)
}

fn safe_pathname(url: &str) -> String {
    Url::parse(url)
        .map(|parsed| parsed.path().to_string())
        .unwrap_or_else(|_| url.to_string())
}

fn inject_auth_headers(command: &str, url: &str, config: &AssistantConfig) -> String {
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

async fn execute_curl_command(
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

fn split_status_and_body(stdout: &str) -> (Option<u16>, String) {
    let marker = format!("{STATUS_MARKER}:");
    let Some(index) = stdout.rfind(&marker) else {
        return (None, stdout.trim().to_string());
    };

    let body = stdout[..index].trim().to_string();
    let status_text = stdout[index + marker.len()..].trim();
    let status = status_text.parse::<u16>().ok();
    (status, body)
}

fn parse_curl_args(command: &str) -> Result<Vec<String>> {
    let tokens = tokenize(command);
    if tokens.is_empty() || tokens[0] != "curl" {
        return Err(anyhow!("command must start with curl"));
    }
    Ok(tokens.into_iter().skip(1).collect())
}

fn tokenize(command: &str) -> Vec<String> {
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

fn mask_sensitive(command: &str) -> String {
    static BEARER_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"Bearer\s+[^\s"']+"#).expect("valid regex"));
    static API_KEY_HEADER_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r#"X-API-Key:\s*[^\s"']+"#).expect("valid regex"));
    static SK_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"sk_[A-Za-z0-9]+").expect("valid regex"));

    let masked = BEARER_RE.replace_all(command, "Bearer ***");
    let masked = API_KEY_HEADER_RE.replace_all(&masked, "X-API-Key: ***");
    SK_RE.replace_all(&masked, "sk_***").to_string()
}

fn load_openapi_value(path: Option<&PathBuf>, env_path: Option<&PathBuf>) -> Value {
    let mut candidates = Vec::new();

    if let Some(path) = path {
        candidates.push(path.clone());
    }

    if let Some(path) = env_path {
        candidates.push(path.clone());
    }

    // Backward-compatible default: search docs/openapi.yaml from cwd to ancestors.
    if candidates.is_empty() {
        if let Some(path) = find_openapi_in_ancestors(
            std::env::current_dir()
                .ok()
                .as_deref()
                .unwrap_or(Path::new(".")),
        ) {
            candidates.push(path);
        }
    }

    for candidate in candidates {
        if candidate.exists() {
            if let Ok(content) = std::fs::read_to_string(&candidate) {
                if let Ok(json_value) = serde_json::from_str::<Value>(&content) {
                    return json_value;
                }

                if let Ok(yaml_value) = serde_yaml::from_str::<serde_yaml::Value>(&content) {
                    if let Ok(json_value) = serde_json::to_value(yaml_value) {
                        return json_value;
                    }
                }
            }
        }
    }

    default_openapi_spec()
}

fn find_openapi_in_ancestors(start: &Path) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let candidate = dir.join("docs").join("openapi.yaml");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn default_openapi_spec() -> Value {
    json!({
      "openapi": "3.1.0",
      "info": {
        "title": "llmlb API",
        "version": "0.1.0",
        "description": "OpenAI-compatible endpoints with optional cloud routing by model prefix (openai:/google:/anthropic:)."
      },
      "servers": [{ "url": "http://localhost:32768" }],
      "paths": {
        "/v1/chat/completions": {
          "post": {
            "summary": "Chat completion (local or cloud depending on model prefix)",
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/ChatRequest" }
                }
              }
            },
            "responses": {
              "200": {
                "description": "Chat completion response",
                "content": {
                  "application/json": {
                    "schema": { "$ref": "#/components/schemas/ChatResponse" }
                  }
                }
              }
            }
          }
        },
        "/v1/models": {
          "get": {
            "summary": "List available models",
            "responses": {
              "200": { "description": "List of models" }
            }
          }
        },
        "/v1/embeddings": {
          "post": {
            "summary": "Generate embeddings",
            "requestBody": {
              "required": true,
              "content": {
                "application/json": {
                  "schema": { "$ref": "#/components/schemas/EmbeddingRequest" }
                }
              }
            }
          }
        },
        "/api/auth/login": {
          "post": { "summary": "Login (sets HttpOnly cookie for the dashboard)" }
        },
        "/api/auth/me": {
          "get": { "summary": "Get current user session" }
        },
        "/api/endpoints": {
          "get": { "summary": "List endpoints" },
          "post": { "summary": "Create endpoint" }
        },
        "/api/endpoints/{id}": {
          "get": { "summary": "Get endpoint detail" },
          "put": { "summary": "Update endpoint" },
          "delete": { "summary": "Delete endpoint" }
        },
        "/api/dashboard/overview": {
          "get": { "summary": "Get dashboard overview" }
        },
        "/api/dashboard/stats": {
          "get": { "summary": "Get dashboard statistics" }
        },
        "/api/models/register": {
          "post": { "summary": "Register a model (admin only)" }
        }
      },
      "components": {
        "schemas": {
          "ChatRequest": {
            "type": "object",
            "properties": {
              "model": { "type": "string", "example": "openai:gpt-4o" },
              "messages": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/ChatMessage" }
              },
              "stream": { "type": "boolean" }
            },
            "required": ["model", "messages"]
          },
          "ChatMessage": {
            "type": "object",
            "properties": {
              "role": { "type": "string", "enum": ["system", "user", "assistant"] },
              "content": { "type": "string" }
            },
            "required": ["role", "content"]
          },
          "ChatResponse": {
            "type": "object",
            "properties": {
              "id": { "type": "string" },
              "model": { "type": "string" },
              "choices": {
                "type": "array",
                "items": { "$ref": "#/components/schemas/ChatChoice" }
              }
            }
          },
          "ChatChoice": {
            "type": "object",
            "properties": {
              "index": { "type": "integer" },
              "message": { "$ref": "#/components/schemas/ChatMessage" },
              "finish_reason": { "type": "string" }
            }
          },
          "EmbeddingRequest": {
            "type": "object",
            "properties": {
              "model": { "type": "string" },
              "input": { "oneOf": [{ "type": "string" }, { "type": "array" }] }
            },
            "required": ["model", "input"]
          }
        }
      }
    })
}

fn overview_guide(router_url: &str) -> String {
    format!(
        "# llmlb API Overview\n\n## Base URL\n\n```\n{router_url}\n```\n\n## API Categories\n\n| Category | Base Path | Notes |\n|----------|-----------|-------|\n| OpenAI-Compatible | /v1/* | Inference APIs. Requires an API key with `api` scope. |\n| Management | /api/* | Endpoint/model/dashboard/admin APIs. Prefer an API key with `admin` scope. |\n| Dashboard UI | /dashboard | Browser UI. Uses HttpOnly cookies after login. |\n\n## Authentication\n\n### API Key Authentication (recommended for programmatic access)\n\n**Header**: `X-API-Key: sk_xxx` (or `Authorization: Bearer sk_xxx`)\n\nScopes (examples):\n- `api`: /v1/* inference endpoints\n- `admin`: /api/* management endpoints\n\nThis CLI can auto-inject:\n- `LLMLB_API_KEY` for /v1/*\n- `LLMLB_ADMIN_API_KEY` for /api/* (preferred)\n\n### Dashboard Session (browser UI)\n\nThe dashboard uses **HttpOnly cookies** for JWT sessions. This CLI does not manage browser cookies.\nUse scoped API keys for automation."
    )
}

fn openai_guide(router_url: &str) -> String {
    format!(
        "# OpenAI-Compatible API (/v1/*)\n\n## Chat Completions\n\n**Endpoint**: POST {router_url}/v1/chat/completions\n\n```bash\ncurl -X POST {router_url}/v1/chat/completions \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: YOUR_API_KEY\" \\\n  -d '{{\n    \"model\": \"llama3.2:3b\",\n    \"messages\": [\n      {{\"role\": \"system\", \"content\": \"You are a helpful assistant.\"}},\n      {{\"role\": \"user\", \"content\": \"Hello!\"}}\n    ],\n    \"stream\": false\n  }}'\n```\n\n## Cloud Routing (model prefix)\n\n```bash\ncurl -X POST {router_url}/v1/chat/completions \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: YOUR_API_KEY\" \\\n  -d '{{\"model\": \"openai:gpt-4o\", \"messages\": [{{\"role\":\"user\",\"content\":\"Hello\"}}]}}'\n```\n\nSupported prefixes:\n- `openai:`\n- `google:`\n- `anthropic:`\n\n## List Models\n\n**Endpoint**: GET {router_url}/v1/models\n\n```bash\ncurl {router_url}/v1/models \\\n  -H \"X-API-Key: YOUR_API_KEY\"\n```\n\n## Embeddings\n\n**Endpoint**: POST {router_url}/v1/embeddings\n\n```bash\ncurl -X POST {router_url}/v1/embeddings \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: YOUR_API_KEY\" \\\n  -d '{{\"model\": \"nomic-embed-text-v1.5\", \"input\": \"Hello world\"}}'\n```"
    )
}

fn endpoint_management_guide(router_url: &str) -> String {
    format!(
        "# Endpoint Management API (/api/endpoints)\n\n## List Endpoints\n\n**Endpoint**: GET {router_url}/api/endpoints\n\n```bash\ncurl {router_url}/api/endpoints \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Create Endpoint\n\n**Endpoint**: POST {router_url}/api/endpoints\n\n```bash\ncurl -X POST {router_url}/api/endpoints \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: ADMIN_API_KEY\" \\\n  -d '{{\n    \"name\": \"xllm-local\",\n    \"base_url\": \"http://127.0.0.1:8080\",\n    \"api_key\": null\n  }}'\n```\n\nNotes:\n- `endpoint_type` can be provided to override auto-detection (xllm/ollama/vllm/openai_compatible).\n- If omitted, llmlb will auto-detect the endpoint type (when reachable)."
    )
}

fn model_management_guide(router_url: &str) -> String {
    format!(
        "# Model Management API (/api/models/*)\n\n## List Models (management view)\n\n**Endpoint**: GET {router_url}/api/models/hub\n\n```bash\ncurl {router_url}/api/models/hub \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Register Model (admin only)\n\n**Endpoint**: POST {router_url}/api/models/register\n\n```bash\ncurl -X POST {router_url}/api/models/register \\\n  -H \"Content-Type: application/json\" \\\n  -H \"X-API-Key: ADMIN_API_KEY\" \\\n  -d '{{\n    \"repo\": \"TheBloke/Llama-2-7B-GGUF\",\n    \"filename\": \"llama-2-7b.Q4_K_M.gguf\"\n  }}'\n```\n\n## Delete Model (admin only)\n\n**Endpoint**: DELETE {router_url}/api/models/:model_name\n\n```bash\ncurl -X DELETE {router_url}/api/models/gpt-oss-20b \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\nNote:\n- llmlb does not push binaries to runtimes. Runtimes fetch manifests and artifacts as needed."
    )
}

fn dashboard_guide(router_url: &str) -> String {
    format!(
        "# Dashboard API (/api/dashboard/*)\n\n## Overview\n\n**Endpoint**: GET {router_url}/api/dashboard/overview\n\n```bash\ncurl {router_url}/api/dashboard/overview \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Stats\n\n**Endpoint**: GET {router_url}/api/dashboard/stats\n\n```bash\ncurl {router_url}/api/dashboard/stats \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Request/Response History (API)\n\n**Endpoint**: GET {router_url}/api/dashboard/request-responses\n\n```bash\ncurl {router_url}/api/dashboard/request-responses \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```\n\n## Router Logs\n\n**Endpoint**: GET {router_url}/api/dashboard/logs/lb\n\n```bash\ncurl {router_url}/api/dashboard/logs/lb \\\n  -H \"X-API-Key: ADMIN_API_KEY\"\n```"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(overrides: impl FnOnce(&mut AssistantConfig)) -> AssistantConfig {
        let mut config = AssistantConfig {
            router_url: Url::parse(DEFAULT_ROUTER_URL).expect("valid url"),
            api_key: None,
            admin_api_key: None,
            jwt_token: None,
            openapi_path: None,
            default_timeout: DEFAULT_TIMEOUT_SECS,
        };
        overrides(&mut config);
        config
    }

    #[test]
    fn sanitize_accepts_valid_command() {
        assert!(sanitize_command("curl http://localhost:32768/v1/models").is_ok());
    }

    #[test]
    fn sanitize_rejects_non_curl() {
        let error = sanitize_command("wget http://localhost").expect_err("must reject");
        assert!(error.contains("must start"));
    }

    #[test]
    fn sanitize_rejects_forbidden_option() {
        let error = sanitize_command("curl --output=/tmp/file http://localhost:32768")
            .expect_err("must reject");
        assert!(error.contains("Forbidden option"));
    }

    #[test]
    fn sanitize_rejects_shell_injection() {
        let error =
            sanitize_command("curl http://localhost:32768; rm -rf /").expect_err("must reject");
        assert!(error.contains("shell injection"));
    }

    #[test]
    fn extract_url_reads_url_from_command() {
        let url = extract_url("curl -X POST http://localhost:32768/api/endpoints");
        assert_eq!(url.as_deref(), Some("http://localhost:32768/api/endpoints"));
    }

    #[test]
    fn validate_host_accepts_localhost_any_port() {
        let router = Url::parse("http://localhost:32768").expect("valid");
        assert!(validate_host("http://localhost:3000/api", &router).is_ok());
        assert!(validate_host("http://127.0.0.1:9999/api", &router).is_ok());
    }

    #[test]
    fn validate_host_rejects_external() {
        let router = Url::parse("http://localhost:32768").expect("valid");
        let error = validate_host("http://example.com/api", &router).expect_err("must reject");
        assert!(error.contains("Host not allowed"));
    }

    #[test]
    fn validate_host_requires_exact_external_host_port() {
        let router = Url::parse("https://api.example.com:9000").expect("valid");
        assert!(validate_host("https://api.example.com:9000/v1/models", &router).is_ok());
        let error = validate_host("https://api.example.com:443/v1/models", &router)
            .expect_err("must reject");
        assert!(error.contains("Host not allowed"));
    }

    #[test]
    fn inject_auth_for_v1_uses_api_key() {
        let cfg = test_config(|c| c.api_key = Some("sk_api".to_string()));
        let out = inject_auth_headers(
            "curl http://localhost:32768/v1/models",
            "http://localhost:32768/v1/models",
            &cfg,
        );
        assert_eq!(
            out,
            "curl -H \"X-API-Key: sk_api\" http://localhost:32768/v1/models"
        );
    }

    #[test]
    fn inject_auth_for_v1_falls_back_to_admin_key() {
        let cfg = test_config(|c| c.admin_api_key = Some("sk_admin".to_string()));
        let out = inject_auth_headers(
            "curl http://localhost:32768/v1/models",
            "http://localhost:32768/v1/models",
            &cfg,
        );
        assert_eq!(
            out,
            "curl -H \"X-API-Key: sk_admin\" http://localhost:32768/v1/models"
        );
    }

    #[test]
    fn inject_auth_for_api_uses_admin_key() {
        let cfg = test_config(|c| c.admin_api_key = Some("sk_admin".to_string()));
        let out = inject_auth_headers(
            "curl http://localhost:32768/api/dashboard/overview",
            "http://localhost:32768/api/dashboard/overview",
            &cfg,
        );
        assert_eq!(
            out,
            "curl -H \"X-API-Key: sk_admin\" http://localhost:32768/api/dashboard/overview"
        );
    }

    #[test]
    fn inject_auth_for_api_falls_back_to_jwt() {
        let cfg = test_config(|c| c.jwt_token = Some("jwt_legacy".to_string()));
        let out = inject_auth_headers(
            "curl http://localhost:32768/api/dashboard/overview",
            "http://localhost:32768/api/dashboard/overview",
            &cfg,
        );
        assert_eq!(
            out,
            "curl -H \"Authorization: Bearer jwt_legacy\" http://localhost:32768/api/dashboard/overview"
        );
    }

    #[test]
    fn inject_auth_for_api_auth_prefers_jwt() {
        let cfg = test_config(|c| {
            c.admin_api_key = Some("sk_admin".to_string());
            c.jwt_token = Some("jwt_legacy".to_string());
        });
        let out = inject_auth_headers(
            "curl http://localhost:32768/api/auth/me",
            "http://localhost:32768/api/auth/me",
            &cfg,
        );
        assert_eq!(
            out,
            "curl -H \"Authorization: Bearer jwt_legacy\" http://localhost:32768/api/auth/me"
        );
    }

    #[test]
    fn inject_auth_skips_if_header_exists() {
        let cfg = test_config(|c| c.api_key = Some("sk_api".to_string()));
        let cmd = "curl -H \"X-API-Key: already\" http://localhost:32768/v1/models";
        let out = inject_auth_headers(cmd, "http://localhost:32768/v1/models", &cfg);
        assert_eq!(out, cmd);
    }

    #[test]
    fn mask_sensitive_replaces_tokens() {
        let cmd =
            "curl -H \"Authorization: Bearer abc\" -H \"X-API-Key: sk_123\" http://localhost:32768";
        let masked = mask_sensitive(cmd);
        assert!(masked.contains("Bearer ***"));
        assert!(masked.contains("X-API-Key: ***"));
        assert!(!masked.contains("sk_123"));
    }

    #[test]
    fn split_status_and_body_parses_marker() {
        let (status, body) = split_status_and_body("{\"ok\":true}\n__STATUS_CODE__:200");
        assert_eq!(status, Some(200));
        assert_eq!(body, "{\"ok\":true}");
    }

    #[test]
    fn load_openapi_falls_back_to_default() {
        let value = load_openapi_value(Some(&PathBuf::from("/tmp/not-found-openapi.yaml")), None);
        assert_eq!(value["openapi"], "3.1.0");
        assert!(value["paths"].is_object());
    }

    #[test]
    fn find_openapi_in_ancestors_discovers_docs_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let nested = tmp.path().join("nested").join("deeper");
        std::fs::create_dir_all(&nested).expect("create nested");

        let docs_dir = tmp.path().join("docs");
        std::fs::create_dir_all(&docs_dir).expect("create docs");
        let openapi_path = docs_dir.join("openapi.yaml");
        std::fs::write(
            &openapi_path,
            r#"
openapi: 3.1.0
info:
  title: Test OpenAPI
  version: 1.0.0
paths: {}
"#,
        )
        .expect("write openapi");

        let found = find_openapi_in_ancestors(&nested).expect("must find docs/openapi.yaml");
        assert_eq!(found, openapi_path);
    }

    #[tokio::test]
    async fn execute_curl_returns_err_for_invalid_command() {
        let cfg = test_config(|_| {});
        let args = CurlArgs {
            command: "wget http://localhost:32768/v1/models".to_string(),
            no_auto_auth: false,
            timeout: None,
            json: true,
        };

        let result = execute_curl(&args, &cfg).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn execute_curl_returns_err_for_disallowed_host() {
        let cfg = test_config(|_| {});
        let args = CurlArgs {
            command: "curl http://example.com/v1/models".to_string(),
            no_auto_auth: false,
            timeout: None,
            json: true,
        };

        let result = execute_curl(&args, &cfg).await;
        assert!(result.is_err());
    }

    #[test]
    fn guide_contains_base_url() {
        let text = overview_guide("http://localhost:32768");
        assert!(text.contains("http://localhost:32768"));
        assert!(text.contains("API Categories"));
    }

    // --- additional coverage tests ---

    #[test]
    fn tokenize_simple() {
        let tokens = tokenize("curl http://localhost:32768/v1/models");
        assert_eq!(tokens, vec!["curl", "http://localhost:32768/v1/models"]);
    }

    #[test]
    fn tokenize_with_single_quotes() {
        let tokens = tokenize("curl -d '{\"key\":\"value\"}' http://localhost:32768");
        assert_eq!(
            tokens,
            vec![
                "curl",
                "-d",
                "{\"key\":\"value\"}",
                "http://localhost:32768"
            ]
        );
    }

    #[test]
    fn tokenize_with_double_quotes() {
        let tokens = tokenize("curl -H \"Content-Type: application/json\" http://localhost:32768");
        assert_eq!(
            tokens,
            vec![
                "curl",
                "-H",
                "Content-Type: application/json",
                "http://localhost:32768"
            ]
        );
    }

    #[test]
    fn tokenize_empty_string() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn tokenize_multiple_spaces() {
        let tokens = tokenize("curl    -s    http://localhost:32768");
        assert_eq!(tokens, vec!["curl", "-s", "http://localhost:32768"]);
    }

    #[test]
    fn sanitize_rejects_pipe_pattern() {
        let err =
            sanitize_command("curl http://localhost:32768 | grep ok").expect_err("must reject");
        assert!(err.contains("shell injection"));
    }

    #[test]
    fn sanitize_rejects_backtick() {
        let err = sanitize_command("curl http://localhost:32768`id`").expect_err("must reject");
        assert!(err.contains("shell injection"));
    }

    #[test]
    fn sanitize_rejects_dollar_paren() {
        let err =
            sanitize_command("curl http://localhost:32768$(whoami)").expect_err("must reject");
        assert!(err.contains("shell injection"));
    }

    #[test]
    fn sanitize_rejects_output_option_short() {
        let err =
            sanitize_command("curl -o /tmp/file http://localhost:32768").expect_err("must reject");
        assert!(err.contains("Forbidden option"));
    }

    #[test]
    fn sanitize_rejects_remote_name() {
        let err = sanitize_command("curl -O http://localhost:32768/file").expect_err("must reject");
        assert!(err.contains("Forbidden option"));
    }

    #[test]
    fn sanitize_rejects_config_option() {
        let err = sanitize_command("curl -K /etc/curlrc http://localhost:32768")
            .expect_err("must reject");
        assert!(err.contains("Forbidden option"));
    }

    #[test]
    fn sanitize_rejects_redirect_pattern() {
        let err =
            sanitize_command("curl http://localhost:32768 >> /tmp/log").expect_err("must reject");
        assert!(err.contains("shell injection"));
    }

    #[test]
    fn sanitize_accepts_curl_alone() {
        // "curl" alone (no URL) should still pass sanitization
        assert!(sanitize_command("curl").is_ok());
    }

    #[test]
    fn extract_url_returns_none_for_no_url() {
        let url = extract_url("curl -X POST -d '{}'");
        assert!(url.is_none());
    }

    #[test]
    fn extract_url_skips_flag_arguments() {
        let url = extract_url(
            "curl -X POST -H \"Content-Type: application/json\" http://localhost:32768/api/test",
        );
        assert_eq!(url.as_deref(), Some("http://localhost:32768/api/test"));
    }

    #[test]
    fn extract_url_handles_https() {
        let url = extract_url("curl https://api.example.com/v1/models");
        assert_eq!(url.as_deref(), Some("https://api.example.com/v1/models"));
    }

    #[test]
    fn extract_url_ignores_non_http_tokens() {
        let url = extract_url("curl ftp://example.com/file");
        assert!(url.is_none());
    }

    #[test]
    fn validate_host_accepts_ipv6_localhost() {
        // IPv6 bracket notation matches via host_with_optional_port fallback
        let router = Url::parse("http://[::1]:32768").expect("valid");
        assert!(validate_host("http://[::1]:32768/api", &router).is_ok());
    }

    #[test]
    fn validate_host_rejects_invalid_url() {
        let router = Url::parse("http://localhost:32768").expect("valid");
        let err = validate_host("not-a-url", &router).expect_err("must reject");
        assert!(err.contains("Invalid URL"));
    }

    #[test]
    fn validate_host_rejects_invalid_scheme() {
        let router = Url::parse("http://localhost:32768").expect("valid");
        let err = validate_host("ftp://localhost:32768/api", &router).expect_err("must reject");
        assert!(err.contains("Invalid protocol"));
    }

    #[test]
    fn host_with_optional_port_no_port() {
        let url = Url::parse("http://example.com").expect("valid");
        // http has default port 80 which is implicit
        let result = host_with_optional_port(&url);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "example.com");
    }

    #[test]
    fn host_with_optional_port_explicit_port() {
        let url = Url::parse("http://example.com:9090").expect("valid");
        let result = host_with_optional_port(&url);
        assert_eq!(result.unwrap(), "example.com:9090");
    }

    #[test]
    fn safe_pathname_valid_url() {
        let path = safe_pathname("http://localhost:32768/v1/chat/completions");
        assert_eq!(path, "/v1/chat/completions");
    }

    #[test]
    fn safe_pathname_invalid_url_returns_input() {
        let path = safe_pathname("not-a-url");
        assert_eq!(path, "not-a-url");
    }

    #[test]
    fn inject_auth_no_keys_configured() {
        let cfg = test_config(|_| {});
        let out = inject_auth_headers(
            "curl http://localhost:32768/v1/models",
            "http://localhost:32768/v1/models",
            &cfg,
        );
        // No keys configured, command unchanged
        assert_eq!(out, "curl http://localhost:32768/v1/models");
    }

    #[test]
    fn inject_auth_skips_for_non_api_non_v1_path() {
        let cfg = test_config(|c| {
            c.api_key = Some("sk_api".to_string());
            c.admin_api_key = Some("sk_admin".to_string());
        });
        let out = inject_auth_headers(
            "curl http://localhost:32768/health",
            "http://localhost:32768/health",
            &cfg,
        );
        // Path is neither /api/ nor /v1/, no auth injection
        assert_eq!(out, "curl http://localhost:32768/health");
    }

    #[test]
    fn inject_auth_for_api_falls_back_to_api_key() {
        let cfg = test_config(|c| c.api_key = Some("sk_api".to_string()));
        let out = inject_auth_headers(
            "curl http://localhost:32768/api/endpoints",
            "http://localhost:32768/api/endpoints",
            &cfg,
        );
        // No admin key or jwt, falls back to api_key for /api/ path
        assert_eq!(
            out,
            "curl -H \"X-API-Key: sk_api\" http://localhost:32768/api/endpoints"
        );
    }

    #[test]
    fn inject_auth_skips_if_authorization_header_present() {
        let cfg = test_config(|c| c.api_key = Some("sk_api".to_string()));
        let cmd = "curl -H \"Authorization: Bearer my_token\" http://localhost:32768/v1/models";
        let out = inject_auth_headers(cmd, "http://localhost:32768/v1/models", &cfg);
        assert_eq!(out, cmd);
    }

    #[test]
    fn mask_sensitive_no_sensitive_data() {
        let cmd = "curl http://localhost:32768/v1/models";
        let masked = mask_sensitive(cmd);
        assert_eq!(masked, cmd);
    }

    #[test]
    fn split_status_and_body_no_marker() {
        let (status, body) = split_status_and_body("just some text");
        assert!(status.is_none());
        assert_eq!(body, "just some text");
    }

    #[test]
    fn split_status_and_body_invalid_status() {
        let (status, body) = split_status_and_body("body\n__STATUS_CODE__:abc");
        assert!(status.is_none()); // "abc" can't be parsed as u16
        assert_eq!(body, "body");
    }

    #[test]
    fn split_status_and_body_empty_body() {
        let (status, body) = split_status_and_body("__STATUS_CODE__:404");
        assert_eq!(status, Some(404));
        assert_eq!(body, "");
    }

    #[test]
    fn parse_curl_args_valid() {
        let args = parse_curl_args("curl -X POST http://localhost:32768").unwrap();
        assert_eq!(args, vec!["-X", "POST", "http://localhost:32768"]);
    }

    #[test]
    fn parse_curl_args_not_curl() {
        let result = parse_curl_args("wget http://localhost");
        assert!(result.is_err());
    }

    #[test]
    fn parse_curl_args_empty() {
        let result = parse_curl_args("");
        assert!(result.is_err());
    }

    #[test]
    fn default_openapi_spec_structure() {
        let spec = default_openapi_spec();
        assert_eq!(spec["openapi"], "3.1.0");
        assert!(spec["info"]["title"].as_str().is_some());
        assert!(spec["paths"].is_object());
        assert!(spec["components"]["schemas"].is_object());
    }

    #[test]
    fn curl_failure_error_with_error_message() {
        let result = CurlResult {
            success: false,
            status_code: Some(500),
            body: None,
            error: Some("server error".to_string()),
            duration_ms: 100,
            executed_command: "curl ...".to_string(),
        };
        let err = curl_failure_error(&result);
        assert!(err.to_string().contains("server error"));
    }

    #[test]
    fn curl_failure_error_with_status_code() {
        let result = CurlResult {
            success: false,
            status_code: Some(404),
            body: None,
            error: None,
            duration_ms: 50,
            executed_command: "curl ...".to_string(),
        };
        let err = curl_failure_error(&result);
        assert!(err.to_string().contains("404"));
    }

    #[test]
    fn curl_failure_error_generic() {
        let result = CurlResult {
            success: false,
            status_code: None,
            body: None,
            error: None,
            duration_ms: 10,
            executed_command: "curl ...".to_string(),
        };
        let err = curl_failure_error(&result);
        assert!(err.to_string().contains("failed"));
    }

    #[test]
    fn openai_guide_contains_endpoints() {
        let text = openai_guide("http://localhost:32768");
        assert!(text.contains("/v1/chat/completions"));
        assert!(text.contains("/v1/models"));
        assert!(text.contains("/v1/embeddings"));
    }

    #[test]
    fn endpoint_management_guide_contains_crud() {
        let text = endpoint_management_guide("http://localhost:32768");
        assert!(text.contains("/api/endpoints"));
        assert!(text.contains("POST"));
        assert!(text.contains("GET"));
    }

    #[test]
    fn model_management_guide_contains_register() {
        let text = model_management_guide("http://localhost:32768");
        assert!(text.contains("/api/models/register"));
        assert!(text.contains("DELETE"));
    }

    #[test]
    fn dashboard_guide_contains_overview() {
        let text = dashboard_guide("http://localhost:32768");
        assert!(text.contains("/api/dashboard/overview"));
        assert!(text.contains("/api/dashboard/stats"));
    }

    #[test]
    fn sanitize_rejects_dollar_brace() {
        let err = sanitize_command("curl http://localhost:32768${HOME}").expect_err("must reject");
        assert!(err.contains("shell injection"));
    }

    #[test]
    fn sanitize_rejects_forbidden_option_with_equals() {
        let err = sanitize_command("curl --output=/tmp/file http://localhost:32768")
            .expect_err("must reject");
        assert!(err.contains("Forbidden option"));
    }

    #[test]
    fn sanitize_rejects_user_option() {
        let err = sanitize_command("curl --user admin:pass http://localhost:32768")
            .expect_err("must reject");
        assert!(err.contains("Forbidden option"));
    }

    #[test]
    fn extract_url_skips_compressed_flag() {
        let url = extract_url("curl --compressed http://localhost:32768/v1/models");
        assert_eq!(url.as_deref(), Some("http://localhost:32768/v1/models"));
    }
}
