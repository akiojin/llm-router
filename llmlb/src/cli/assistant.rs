//! assistant subcommand
//!
//! Provides helper functionality previously available in the legacy MCP server as a CLI.

use anyhow::{Context, Result};
use reqwest::Url;
use std::path::PathBuf;

mod args;
mod curl;
mod guide;
mod openapi;
pub use args::{AssistantArgs, AssistantCommand, CurlArgs, GuideArgs, GuideCategory, OpenApiArgs};
use curl::*;
use guide::{
    dashboard_guide, endpoint_management_guide, model_management_guide, openai_guide,
    overview_guide,
};
use openapi::load_openapi_value;
#[cfg(test)]
use openapi::{default_openapi_spec, find_openapi_in_ancestors};

const DEFAULT_ROUTER_URL: &str = "http://localhost:32768";
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 300;
const MIN_TIMEOUT_SECS: u64 = 1;

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

/// Execute an assistant command
pub async fn execute(command: &AssistantCommand) -> Result<()> {
    let config = AssistantConfig::from_env()?;
    match command {
        AssistantCommand::Curl(args) => execute_curl(args, &config).await,
        AssistantCommand::Openapi(args) => execute_openapi(args, &config),
        AssistantCommand::Guide(args) => execute_guide(args, &config),
    }
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
