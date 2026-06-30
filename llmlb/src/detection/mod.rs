//! Endpoint Type Detection Module
//!
//! SPEC-e8e9326e: Automatic endpoint type detection
//!
//! Detection priority: xLLM > LM Studio > Ollama > vLLM > llama.cpp > OpenAI-compatible

mod llama_cpp;
mod lm_studio;
mod ollama;
mod vllm;
mod xllm;

use reqwest::Client;
use tracing::{debug, warn};

use crate::types::endpoint::EndpointType;

pub use llama_cpp::detect_llamacpp;
pub use lm_studio::detect_lm_studio;
pub use ollama::detect_ollama;
pub use vllm::detect_vllm;
pub use xllm::detect_xllm;

/// 検出エラー
#[derive(Debug, Clone)]
pub enum DetectionError {
    /// エンドポイントに接続できない（全プローブが接続エラー）
    Unreachable(String),
    /// 接続はできたが対応タイプに一致しない
    UnsupportedType(String),
}

impl std::fmt::Display for DetectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(msg) => write!(f, "endpoint unreachable: {}", msg),
            Self::UnsupportedType(msg) => write!(f, "unsupported endpoint type: {}", msg),
        }
    }
}

impl std::error::Error for DetectionError {}

/// 検出成功時の結果
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// 判定されたエンドポイントタイプ
    pub endpoint_type: EndpointType,
    /// 判定理由
    pub reason: String,
}

/// Detect endpoint type with a provided HTTP client
pub async fn detect_endpoint_type_with_client(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<DetectionResult, DetectionError> {
    let base_url = base_url.trim_end_matches('/');

    debug!(base_url = %base_url, "Starting endpoint type detection");

    // Track whether at least one HTTP response was received
    let mut got_any_response = false;

    // Priority 1: xLLM detection
    match detect_xllm(client, base_url, api_key).await {
        Some(reason) => {
            debug!(endpoint_type = "xllm", "Detected xLLM endpoint");
            return Ok(DetectionResult {
                endpoint_type: EndpointType::Xllm,
                reason,
            });
        }
        None => {
            // Check if xLLM probe got any HTTP response (not just connection error)
            // For now, we track this via the subsequent probes
        }
    }

    // Priority 2: LM Studio detection
    if let Some(reason) = detect_lm_studio(client, base_url, api_key).await {
        debug!(endpoint_type = "lm_studio", "Detected LM Studio endpoint");
        return Ok(DetectionResult {
            endpoint_type: EndpointType::LmStudio,
            reason,
        });
    }

    // Priority 3: Ollama detection
    if let Some(reason) = detect_ollama(client, base_url).await {
        debug!(endpoint_type = "ollama", "Detected Ollama endpoint");
        return Ok(DetectionResult {
            endpoint_type: EndpointType::Ollama,
            reason,
        });
    }

    // Priority 4: vLLM detection
    if let Some(reason) = detect_vllm(client, base_url, api_key).await {
        debug!(endpoint_type = "vllm", "Detected vLLM endpoint");
        return Ok(DetectionResult {
            endpoint_type: EndpointType::Vllm,
            reason,
        });
    }

    // Priority 5: llama.cpp detection
    if let Some(reason) = detect_llamacpp(client, base_url).await {
        debug!(endpoint_type = "llamacpp", "Detected llama.cpp endpoint");
        return Ok(DetectionResult {
            endpoint_type: EndpointType::Llamacpp,
            reason,
        });
    }

    // Priority 6: OpenAI-compatible detection (also serves as connectivity check)
    match detect_openai_compatible(client, base_url, api_key).await {
        OpenAiDetectResult::Detected(reason) => {
            debug!(
                endpoint_type = "openai_compatible",
                "Detected OpenAI-compatible endpoint"
            );
            return Ok(DetectionResult {
                endpoint_type: EndpointType::OpenaiCompatible,
                reason,
            });
        }
        OpenAiDetectResult::NotMatched => {
            got_any_response = true;
        }
        OpenAiDetectResult::ConnectionError => {
            // No HTTP response from this probe
        }
    }

    // If we got any HTTP response but no type matched, it's unsupported
    if got_any_response {
        warn!(base_url = %base_url, "Endpoint responded but type could not be determined");
        Err(DetectionError::UnsupportedType(format!(
            "endpoint at {} responded but does not match any supported type",
            base_url
        )))
    } else {
        // Try a simple connectivity check to distinguish unreachable from unsupported
        match client.get(format!("{}/v1/models", base_url)).send().await {
            Ok(_) => {
                // Got a response but nothing matched
                warn!(base_url = %base_url, "Endpoint responded but type could not be determined");
                Err(DetectionError::UnsupportedType(format!(
                    "endpoint at {} responded but does not match any supported type",
                    base_url
                )))
            }
            Err(e) => {
                warn!(base_url = %base_url, error = %e, "Endpoint is unreachable");
                Err(DetectionError::Unreachable(format!(
                    "could not connect to {}",
                    base_url
                )))
            }
        }
    }
}

/// Internal result for OpenAI-compatible detection
enum OpenAiDetectResult {
    /// Detected as OpenAI-compatible
    Detected(String),
    /// Got HTTP response but not OpenAI-compatible
    NotMatched,
    /// Connection error (no HTTP response)
    ConnectionError,
}

/// Detect OpenAI-compatible endpoint (GET /v1/models)
async fn detect_openai_compatible(
    client: &Client,
    base_url: &str,
    api_key: Option<&str>,
) -> OpenAiDetectResult {
    let url = format!("{}/v1/models", base_url);

    let mut request = client.get(&url);
    if let Some(key) = api_key {
        request = request.header("Authorization", format!("Bearer {}", key));
    }

    match request.send().await {
        Ok(response) if response.status().is_success() => {
            // Check if the response looks like OpenAI models response
            if let Ok(json) = response.json::<serde_json::Value>().await {
                if json.get("data").is_some() || json.get("object").is_some() {
                    return OpenAiDetectResult::Detected(
                        "OpenAI-compatible: /v1/models responded 200".to_string(),
                    );
                }
            }
            OpenAiDetectResult::NotMatched
        }
        Ok(_) => OpenAiDetectResult::NotMatched,
        Err(e) => {
            debug!(error = %e, "OpenAI-compatible detection failed");
            OpenAiDetectResult::ConnectionError
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn test_detection_error_display() {
        let unreachable = DetectionError::Unreachable("connection refused".to_string());
        assert!(unreachable.to_string().contains("unreachable"));

        let unsupported = DetectionError::UnsupportedType("no match".to_string());
        assert!(unsupported.to_string().contains("unsupported"));
    }

    #[tokio::test]
    async fn detect_endpoint_type_detects_openai_compatible() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/system"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "object": "list",
                "data": [
                    {"id": "gpt-test", "object": "model"}
                ]
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();

        let detected = detect_endpoint_type_with_client(&client, &server.uri(), None)
            .await
            .expect("should detect openai-compatible endpoint");
        assert_eq!(
            detected.endpoint_type,
            crate::types::endpoint::EndpointType::OpenaiCompatible
        );
        assert!(detected.reason.contains("/v1/models"));
    }

    #[tokio::test]
    async fn detect_endpoint_type_returns_unsupported_when_response_shape_is_unknown() {
        let server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/system"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/v1/models"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "hello": "world"
            })))
            .mount(&server)
            .await;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();

        let err = detect_endpoint_type_with_client(&client, &server.uri(), None)
            .await
            .expect_err("should fail for unsupported endpoint shape");
        match err {
            DetectionError::UnsupportedType(msg) => {
                assert!(msg.contains("does not match any supported type"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[tokio::test]
    async fn detect_endpoint_type_returns_unreachable_for_dead_host() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(100))
            .build()
            .unwrap();

        let err = detect_endpoint_type_with_client(&client, "http://127.0.0.1:9", None)
            .await
            .expect_err("connection should fail");
        match err {
            DetectionError::Unreachable(msg) => {
                assert!(msg.contains("could not connect"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
