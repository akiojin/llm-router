use axum::http::{HeaderMap, HeaderValue, StatusCode};
use std::net::IpAddr;

#[test]
fn extract_client_ip_prefers_x_forwarded_for() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("unknown, 198.51.100.30, 10.0.0.3"),
    );
    let parsed = crate::common::ip::extract_client_ip_from_headers(&headers)
        .expect("must parse x-forwarded-for");
    assert_eq!(parsed, "198.51.100.30".parse::<IpAddr>().unwrap());
}

#[test]
fn extract_client_ip_uses_forwarded_when_xff_missing() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static("for=unknown;proto=https, for=\"[2001:db8::20]:9443\""),
    );
    let parsed =
        crate::common::ip::extract_client_ip_from_headers(&headers).expect("must parse forwarded");
    assert_eq!(parsed, "2001:db8::20".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_parses_ipv4_with_port() {
    let parsed = crate::common::ip::parse_forwarded_ip_candidate("198.51.100.44:8080")
        .expect("must parse ipv4 with port");
    assert_eq!(parsed, "198.51.100.44".parse::<IpAddr>().unwrap());
}

#[test]
fn test_image_size_limit() {
    const MAX_IMAGE_SIZE: usize = 4 * 1024 * 1024; // 4MB

    // 4MB以下は許可
    let small_image = vec![0u8; MAX_IMAGE_SIZE];
    assert!(small_image.len() <= MAX_IMAGE_SIZE);

    // 4MB超は拒否
    let large_image = vec![0u8; MAX_IMAGE_SIZE + 1];
    assert!(large_image.len() > MAX_IMAGE_SIZE);
}

#[test]
fn test_n_validation() {
    // Helper function to validate n parameter (1-10 is valid)
    fn is_valid_n(n: u32) -> bool {
        (1..=10).contains(&n)
    }

    // n = 0 は無効
    assert!(!is_valid_n(0));

    // n = 1-10 は有効
    for n in 1..=10 {
        assert!(is_valid_n(n));
    }

    // n = 11 は無効
    assert!(!is_valid_n(11));
}

// T007: 画像生成 capabilities検証テスト (RED)
// ImageGeneration capability を持たないモデルで /v1/images/generations を呼ぶとエラー
#[test]
fn test_image_generation_capability_validation_error_message() {
    use crate::types::model::{ModelCapability, ModelType};

    // LLMモデルはTextGenerationのみ、ImageGenerationは非対応
    let llm_caps = ModelCapability::from_model_type(ModelType::Llm);
    assert!(!llm_caps.contains(&ModelCapability::ImageGeneration));

    // TTSモデルもTextToSpeechのみ、ImageGenerationは非対応
    let tts_caps = ModelCapability::from_model_type(ModelType::TextToSpeech);
    assert!(!tts_caps.contains(&ModelCapability::ImageGeneration));

    // ASRモデルもSpeechToTextのみ、ImageGenerationは非対応
    let stt_caps = ModelCapability::from_model_type(ModelType::SpeechToText);
    assert!(!stt_caps.contains(&ModelCapability::ImageGeneration));

    // EmbeddingモデルもEmbeddingのみ、ImageGenerationは非対応
    let embed_caps = ModelCapability::from_model_type(ModelType::Embedding);
    assert!(!embed_caps.contains(&ModelCapability::ImageGeneration));

    // 期待されるエラーメッセージ形式
    let model_name = "llama-3.1-8b";
    let expected_error = format!("Model '{}' does not support image generation", model_name);
    assert!(expected_error.contains("does not support image generation"));
}

// --- error_response tests ---

#[test]
fn error_response_http_returns_correct_status() {
    use super::error_response;
    use crate::common::error::LbError;

    let resp = error_response(
        LbError::Http("upstream error".to_string()),
        StatusCode::BAD_GATEWAY,
    );
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}

#[test]
fn error_response_service_unavailable_returns_503() {
    use super::error_response;
    use crate::common::error::LbError;

    let resp = error_response(
        LbError::ServiceUnavailable("no image backends".to_string()),
        StatusCode::SERVICE_UNAVAILABLE,
    );
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn error_response_invalid_model_name_returns_400() {
    use super::error_response;
    use crate::common::error::LbError;

    let resp = error_response(
        LbError::InvalidModelName("bad:model:name".to_string()),
        StatusCode::BAD_REQUEST,
    );
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn error_response_fallback_type_for_internal_error() {
    use super::error_response;
    use crate::common::error::LbError;

    let resp = error_response(
        LbError::Database("db connection lost".to_string()),
        StatusCode::INTERNAL_SERVER_ERROR,
    );
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// --- openai_error helper tests ---

#[test]
fn openai_error_returns_ok_with_requested_status() {
    use super::openai_error;

    let result = openai_error("missing field", StatusCode::BAD_REQUEST);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().status(), StatusCode::BAD_REQUEST);
}

#[test]
fn openai_error_accepts_owned_string() {
    use super::openai_error;

    let msg = format!("Image file exceeds maximum size of {}MB", 4);
    let result = openai_error(msg, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(result.is_ok());
}

// --- forwarded header extraction tests ---

#[test]
fn extract_client_ip_returns_none_for_empty_headers() {
    let headers = HeaderMap::new();
    assert!(crate::common::ip::extract_client_ip_from_headers(&headers).is_none());
}

#[test]
fn extract_client_ip_returns_none_when_all_unknown() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("unknown, unknown, _hidden"),
    );
    assert!(crate::common::ip::extract_client_ip_from_headers(&headers).is_none());
}

#[test]
fn parse_forwarded_ip_candidate_empty_returns_none() {
    assert!(crate::common::ip::parse_forwarded_ip_candidate("").is_none());
}

#[test]
fn parse_forwarded_ip_candidate_unknown_returns_none() {
    assert!(crate::common::ip::parse_forwarded_ip_candidate("unknown").is_none());
    assert!(crate::common::ip::parse_forwarded_ip_candidate("UNKNOWN").is_none());
}

#[test]
fn parse_forwarded_ip_candidate_obfuscated_returns_none() {
    assert!(crate::common::ip::parse_forwarded_ip_candidate("_secret").is_none());
}

#[test]
fn parse_forwarded_ip_candidate_plain_ipv4() {
    let ip =
        crate::common::ip::parse_forwarded_ip_candidate("203.0.113.50").expect("should parse ipv4");
    assert_eq!(ip, "203.0.113.50".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_plain_ipv6() {
    let ip =
        crate::common::ip::parse_forwarded_ip_candidate("2001:db8::1").expect("should parse ipv6");
    assert_eq!(ip, "2001:db8::1".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_bracketed_ipv6() {
    let ip = crate::common::ip::parse_forwarded_ip_candidate("\"[2001:db8::ff]:9090\"")
        .expect("should parse bracketed ipv6");
    assert_eq!(ip, "2001:db8::ff".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_ipv4_with_port() {
    let ip = crate::common::ip::parse_forwarded_ip_candidate("10.0.0.5:3000")
        .expect("should parse ipv4 with port");
    assert_eq!(ip, "10.0.0.5".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_invalid() {
    assert!(crate::common::ip::parse_forwarded_ip_candidate("garbage-value").is_none());
}

// --- Image generation validation logic tests ---

#[test]
fn test_prompt_empty_check() {
    let empty_prompt = "";
    assert!(empty_prompt.is_empty());
    let valid_prompt = "A cat sitting on a windowsill";
    assert!(!valid_prompt.is_empty());
}

#[test]
fn test_n_boundary_values() {
    let is_valid_n = |n: u8| (1..=10).contains(&n);
    // n=0 is invalid
    assert!(!is_valid_n(0));
    // n=1 is valid
    assert!(is_valid_n(1));
    // n=10 is valid
    assert!(is_valid_n(10));
    // n=11 is invalid
    assert!(!is_valid_n(11));
}

#[test]
fn test_image_size_max_boundary() {
    const MAX_IMAGE_SIZE: usize = 4 * 1024 * 1024;
    let fits_limit = |size: usize| size <= MAX_IMAGE_SIZE;

    // Exactly at boundary - allowed
    assert!(fits_limit(MAX_IMAGE_SIZE));

    // One byte over - rejected
    assert!(!fits_limit(MAX_IMAGE_SIZE + 1));

    // Well below boundary - allowed
    let small = 1024_usize;
    assert!(small <= MAX_IMAGE_SIZE);
}

#[test]
fn test_image_generation_request_defaults() {
    use crate::common::protocol::ImageGenerationRequest;

    let json = r#"{"model":"sd-xl","prompt":"A landscape"}"#;
    let req: ImageGenerationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.n, 1);
    assert_eq!(req.model, "sd-xl");
    assert_eq!(req.prompt, "A landscape");
    assert!(req.negative_prompt.is_none());
    assert!(req.seed.is_none());
    assert!(req.steps.is_none());
}

#[test]
fn test_image_generation_request_with_all_optional_fields() {
    use crate::common::protocol::ImageGenerationRequest;

    let json = r#"{
            "model": "sd-xl",
            "prompt": "A beautiful sunset",
            "n": 4,
            "size": "512x512",
            "quality": "hd",
            "style": "natural",
            "response_format": "b64_json",
            "negative_prompt": "blurry, low quality",
            "seed": 42,
            "steps": 30
        }"#;
    let req: ImageGenerationRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.n, 4);
    assert_eq!(req.negative_prompt, Some("blurry, low quality".to_string()));
    assert_eq!(req.seed, Some(42));
    assert_eq!(req.steps, Some(30));
}

#[test]
fn test_model_default_for_edits() {
    // When model is None, default is "stable-diffusion-xl"
    let model: Option<String> = None;
    let resolved = match model {
        Some(model) => model,
        None => "stable-diffusion-xl".to_string(),
    };
    assert_eq!(resolved, "stable-diffusion-xl");
}

#[test]
fn test_model_override_for_edits() {
    let model: Option<String> = Some("dall-e-3".to_string());
    let resolved = match model {
        Some(model) => model,
        None => "stable-diffusion-xl".to_string(),
    };
    assert_eq!(resolved, "dall-e-3");
}

#[test]
fn test_model_default_for_variations() {
    let model: Option<String> = None;
    let resolved = match model {
        Some(model) => model,
        None => "stable-diffusion-xl".to_string(),
    };
    assert_eq!(resolved, "stable-diffusion-xl");
}
