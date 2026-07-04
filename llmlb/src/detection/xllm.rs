//! xLLM Endpoint Type Detection
//!
//! SPEC-e8e9326e: xLLM detection via GET /api/system
//!
//! xLLM endpoints expose a /api/system endpoint that returns
//! system information including `xllm_version` field.

use crate::common::http::RequestBuilderBearerExt;
use reqwest::Client;
use serde::Deserialize;
use tracing::debug;

/// xLLM system info response
#[derive(Debug, Deserialize)]
struct XllmSystemInfo {
    /// xLLM version string (e.g., "0.1.0")
    xllm_version: Option<String>,
    /// Optional server name
    #[serde(default)]
    #[allow(dead_code)]
    server_name: Option<String>,
}

/// Detect xLLM endpoint by querying GET /api/system
///
/// Returns a reason string if the endpoint responds with
/// a JSON object containing `xllm_version` field.
pub async fn detect_xllm(client: &Client, base_url: &str, api_key: Option<&str>) -> Option<String> {
    let url = format!("{}/api/system", base_url);

    let mut request = client.get(&url);
    request = request.bearer_opt(api_key);

    match request.send().await {
        Ok(response) if response.status().is_success() => {
            match response.json::<XllmSystemInfo>().await {
                Ok(info) => {
                    if let Some(version) = info.xllm_version {
                        debug!(
                            version = ?version,
                            "Detected xLLM endpoint"
                        );
                        return Some(format!("xLLM: /api/system xllm_version={}", version));
                    }
                    None
                }
                Err(e) => {
                    debug!(error = %e, "Failed to parse xLLM system info");
                    None
                }
            }
        }
        Ok(response) => {
            debug!(
                status = %response.status(),
                "xLLM detection: non-success status"
            );
            None
        }
        Err(e) => {
            debug!(error = %e, "xLLM detection request failed");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn test_xllm_system_info_deserialize() {
        let json = r#"{"xllm_version": "0.1.0", "server_name": "test"}"#;
        let info: XllmSystemInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.xllm_version, Some("0.1.0".to_string()));
    }

    #[test]
    fn test_xllm_system_info_deserialize_minimal() {
        let json = r#"{"xllm_version": "1.0.0"}"#;
        let info: XllmSystemInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.xllm_version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_xllm_system_info_deserialize_no_version() {
        let json = r#"{"server_name": "other"}"#;
        let info: XllmSystemInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.xllm_version, None);
    }

    #[tokio::test]
    async fn detect_xllm_returns_reason_on_success() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/system"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "xllm_version": "1.2.3",
                "server_name": "test-xllm"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let detected = detect_xllm(&client, &server.uri(), Some("sk-test")).await;
        assert_eq!(
            detected,
            Some("xLLM: /api/system xllm_version=1.2.3".to_string())
        );
    }

    #[tokio::test]
    async fn detect_xllm_returns_none_on_non_success_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/system"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        assert_eq!(detect_xllm(&client, &server.uri(), None).await, None);
    }

    #[tokio::test]
    async fn detect_xllm_returns_none_on_invalid_json() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/system"))
            .respond_with(ResponseTemplate::new(200).set_body_string("{"))
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        assert_eq!(detect_xllm(&client, &server.uri(), None).await, None);
    }
}
