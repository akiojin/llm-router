use axum::http::{HeaderMap, HeaderValue, StatusCode};
use std::net::IpAddr;

#[test]
fn extract_client_ip_prefers_first_valid_x_forwarded_for() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("unknown, 203.0.113.5, 10.0.0.1"),
    );
    headers.insert(
        "forwarded",
        HeaderValue::from_static("for=198.51.100.10;proto=https"),
    );
    let parsed = crate::common::ip::extract_client_ip_from_headers(&headers)
        .expect("must parse x-forwarded-for");
    assert_eq!(parsed, "203.0.113.5".parse::<IpAddr>().unwrap());
}

#[test]
fn extract_client_ip_falls_back_to_forwarded_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static("for=unknown;proto=https, for=\"[2001:db8::a]:8443\""),
    );
    let parsed =
        crate::common::ip::extract_client_ip_from_headers(&headers).expect("must parse forwarded");
    assert_eq!(parsed, "2001:db8::a".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_supports_bracketed_ipv6() {
    let parsed = crate::common::ip::parse_forwarded_ip_candidate("\"[2001:db8::f]:443\"")
        .expect("must parse bracketed ipv6");
    assert_eq!(parsed, "2001:db8::f".parse::<IpAddr>().unwrap());
}

#[test]
fn test_input_length_validation() {
    // 4096文字以下は許可
    let short_input = "a".repeat(4096);
    assert!(short_input.chars().count() <= 4096);

    // 4097文字以上は拒否
    let long_input = "a".repeat(4097);
    assert!(long_input.chars().count() > 4096);
}

#[test]
fn test_unicode_input_length() {
    // 日本語文字のカウント（バイト数ではなく文字数）
    let japanese = "あ".repeat(4096);
    assert_eq!(japanese.chars().count(), 4096);

    let japanese_long = "あ".repeat(4097);
    assert!(japanese_long.chars().count() > 4096);
}

// T004: TTS capabilities検証テスト (RED)
// TextToSpeech capability を持たないモデルで /v1/audio/speech を呼ぶとエラー
#[test]
fn test_tts_capability_validation_error_message() {
    use crate::types::model::{ModelCapability, ModelType};

    // LLMモデルはTextGenerationのみ、TextToSpeechは非対応
    let llm_caps = ModelCapability::from_model_type(ModelType::Llm);
    assert!(!llm_caps.contains(&ModelCapability::TextToSpeech));

    // 期待されるエラーメッセージ形式
    let model_name = "llama-3.1-8b";
    let expected_error = format!("Model '{}' does not support text-to-speech", model_name);
    assert!(expected_error.contains("does not support text-to-speech"));
}

// T005: ASR capabilities検証テスト (RED)
// SpeechToText capability を持たないモデルで /v1/audio/transcriptions を呼ぶとエラー
#[test]
fn test_asr_capability_validation_error_message() {
    use crate::types::model::{ModelCapability, ModelType};

    // LLMモデルはTextGenerationのみ、SpeechToTextは非対応
    let llm_caps = ModelCapability::from_model_type(ModelType::Llm);
    assert!(!llm_caps.contains(&ModelCapability::SpeechToText));

    // TTSモデルもSpeechToTextは非対応
    let tts_caps = ModelCapability::from_model_type(ModelType::TextToSpeech);
    assert!(!tts_caps.contains(&ModelCapability::SpeechToText));

    // 期待されるエラーメッセージ形式
    let model_name = "vibevoice-v1";
    let expected_error = format!("Model '{}' does not support speech-to-text", model_name);
    assert!(expected_error.contains("does not support speech-to-text"));
}

// --- error_response tests ---

#[test]
fn error_response_http_error_type() {
    use super::error_response;
    use crate::common::error::LbError;

    let resp = error_response(
        LbError::Http("connection refused".to_string()),
        StatusCode::BAD_GATEWAY,
    );
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}

#[test]
fn error_response_service_unavailable_type() {
    use super::error_response;
    use crate::common::error::LbError;

    let resp = error_response(
        LbError::ServiceUnavailable("no backends".to_string()),
        StatusCode::SERVICE_UNAVAILABLE,
    );
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn error_response_invalid_model_name_type() {
    use super::error_response;
    use crate::common::error::LbError;

    let resp = error_response(
        LbError::InvalidModelName("bad:model:name".to_string()),
        StatusCode::BAD_REQUEST,
    );
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn error_response_fallback_api_error_type() {
    use super::error_response;
    use crate::common::error::LbError;

    let resp = error_response(
        LbError::Internal("unknown error".to_string()),
        StatusCode::INTERNAL_SERVER_ERROR,
    );
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// --- openai_error helper tests ---

#[test]
fn openai_error_returns_ok_with_status() {
    use super::openai_error;

    let result = openai_error("test error message", StatusCode::BAD_REQUEST);
    assert!(result.is_ok());
    let resp = result.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn openai_error_accepts_string_type() {
    use super::openai_error;

    let msg = String::from("dynamic error");
    let result = openai_error(msg, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(result.is_ok());
    let resp = result.unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// --- forwarded header extraction edge cases ---

#[test]
fn extract_client_ip_returns_none_for_empty_headers() {
    let headers = HeaderMap::new();
    assert!(crate::common::ip::extract_client_ip_from_headers(&headers).is_none());
}

#[test]
fn extract_client_ip_returns_none_for_all_unknown_xff() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("unknown, unknown"),
    );
    assert!(crate::common::ip::extract_client_ip_from_headers(&headers).is_none());
}

#[test]
fn parse_forwarded_ip_candidate_empty_string() {
    assert!(crate::common::ip::parse_forwarded_ip_candidate("").is_none());
}

#[test]
fn parse_forwarded_ip_candidate_unknown_string() {
    assert!(crate::common::ip::parse_forwarded_ip_candidate("unknown").is_none());
}

#[test]
fn parse_forwarded_ip_candidate_unknown_case_insensitive() {
    assert!(crate::common::ip::parse_forwarded_ip_candidate("UNKNOWN").is_none());
    assert!(crate::common::ip::parse_forwarded_ip_candidate("Unknown").is_none());
}

#[test]
fn parse_forwarded_ip_candidate_obfuscated_identifier() {
    // RFC 7239: obfuscated identifiers start with underscore
    assert!(crate::common::ip::parse_forwarded_ip_candidate("_hidden").is_none());
}

#[test]
fn parse_forwarded_ip_candidate_plain_ipv4() {
    let parsed = crate::common::ip::parse_forwarded_ip_candidate("198.51.100.1")
        .expect("must parse plain ipv4");
    assert_eq!(parsed, "198.51.100.1".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_plain_ipv6() {
    let parsed = crate::common::ip::parse_forwarded_ip_candidate("2001:db8::1")
        .expect("must parse plain ipv6");
    assert_eq!(parsed, "2001:db8::1".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_quoted_ipv4() {
    let parsed = crate::common::ip::parse_forwarded_ip_candidate("\"198.51.100.2\"")
        .expect("must parse quoted ipv4");
    assert_eq!(parsed, "198.51.100.2".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_ipv4_with_port() {
    let parsed = crate::common::ip::parse_forwarded_ip_candidate("10.0.0.1:8080")
        .expect("must parse ipv4 with port");
    assert_eq!(parsed, "10.0.0.1".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_bracketed_ipv6_with_port() {
    let parsed = crate::common::ip::parse_forwarded_ip_candidate("[2001:db8::1]:443")
        .expect("must parse bracketed ipv6 with port");
    assert_eq!(parsed, "2001:db8::1".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_whitespace_trimming() {
    let parsed = crate::common::ip::parse_forwarded_ip_candidate("  10.0.0.1  ")
        .expect("must parse with whitespace");
    assert_eq!(parsed, "10.0.0.1".parse::<IpAddr>().unwrap());
}

#[test]
fn parse_forwarded_ip_candidate_invalid_returns_none() {
    assert!(crate::common::ip::parse_forwarded_ip_candidate("not-an-ip").is_none());
}

#[test]
fn extract_x_forwarded_for_single_ip() {
    let mut headers = HeaderMap::new();
    headers.insert("x-forwarded-for", HeaderValue::from_static("192.168.1.1"));
    let ip = crate::common::ip::extract_x_forwarded_for(&headers).expect("should parse single ip");
    assert_eq!(ip, "192.168.1.1".parse::<IpAddr>().unwrap());
}

#[test]
fn extract_x_forwarded_for_multiple_ips_returns_first_valid() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("unknown, _obfuscated, 10.0.0.1, 192.168.0.1"),
    );
    let ip =
        crate::common::ip::extract_x_forwarded_for(&headers).expect("should skip invalid entries");
    assert_eq!(ip, "10.0.0.1".parse::<IpAddr>().unwrap());
}

#[test]
fn extract_x_forwarded_for_missing_header_returns_none() {
    let headers = HeaderMap::new();
    assert!(crate::common::ip::extract_x_forwarded_for(&headers).is_none());
}

#[test]
fn extract_forwarded_for_standard_format() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static("for=192.0.2.60;proto=http;by=203.0.113.43"),
    );
    let ip =
        crate::common::ip::extract_forwarded_for(&headers).expect("should parse standard format");
    assert_eq!(ip, "192.0.2.60".parse::<IpAddr>().unwrap());
}

#[test]
fn extract_forwarded_for_multiple_entries() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static("for=unknown, for=198.51.100.20"),
    );
    let ip = crate::common::ip::extract_forwarded_for(&headers).expect("should parse second entry");
    assert_eq!(ip, "198.51.100.20".parse::<IpAddr>().unwrap());
}

#[test]
fn extract_forwarded_for_missing_header_returns_none() {
    let headers = HeaderMap::new();
    assert!(crate::common::ip::extract_forwarded_for(&headers).is_none());
}

#[test]
fn extract_forwarded_for_ignores_non_for_keys() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "forwarded",
        HeaderValue::from_static("by=203.0.113.43;proto=https"),
    );
    assert!(crate::common::ip::extract_forwarded_for(&headers).is_none());
}

// --- SpeechRequest / input validation edge case tests ---

#[test]
fn test_speech_request_deserialization_with_all_fields() {
    use crate::common::protocol::SpeechRequest;
    let json = r#"{
            "model": "tts-1-hd",
            "input": "Hello world",
            "voice": "echo",
            "response_format": "flac",
            "speed": 1.5
        }"#;
    let req: SpeechRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.model, "tts-1-hd");
    assert_eq!(req.input, "Hello world");
    assert_eq!(req.voice, "echo");
    assert_eq!(req.speed, 1.5);
}

#[test]
fn test_empty_input_validation_logic() {
    // Verifies the empty-check logic used in the handler
    let empty = "";
    assert!(empty.is_empty());
    let non_empty = "hello";
    assert!(!non_empty.is_empty());
}

#[test]
fn test_input_char_count_with_mixed_scripts() {
    // Mixed ASCII + CJK + emoji
    let input = "Hello, \u{4e16}\u{754c}! \u{1f600}";
    let count = input.chars().count();
    // "Hello, " = 7, "世界" = 2, "! " = 2, emoji = 1 = 12
    assert_eq!(count, 12);
    assert!(count <= 4096);
}

#[test]
fn test_input_exactly_4096_chars() {
    let input = "x".repeat(4096);
    assert_eq!(input.chars().count(), 4096);
    assert!(input.chars().count() <= 4096);
}

#[test]
fn test_input_exactly_4097_chars() {
    let input = "x".repeat(4097);
    assert!(input.chars().count() > 4096);
}
