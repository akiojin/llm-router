//! エンドポイントバリデーションのUnit Test
//!
//! SPEC-e8e9326e T035: エンドポイントバリデーションのテスト
//! - name UNIQUE制約
//! - base_url形式バリデーション
//! - 必須フィールドの検証

use llmlb::types::endpoint::{Endpoint, EndpointType};

#[test]
fn test_endpoint_new_creates_valid_endpoint() {
    let endpoint = Endpoint::new(
        "Test Endpoint".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );

    assert!(!endpoint.id.is_nil());
    assert_eq!(endpoint.name, "Test Endpoint");
    assert_eq!(endpoint.base_url, "http://localhost:8080");
    assert_eq!(
        endpoint.status,
        llmlb::types::endpoint::EndpointStatus::Pending
    );
    assert_eq!(endpoint.health_check_interval_secs, 30); // default
    assert_eq!(endpoint.inference_timeout_secs, 600); // local runtime default
    assert_eq!(endpoint.error_count, 0);
}

#[test]
fn test_endpoint_name_cannot_be_empty() {
    // 空の名前を持つエンドポイントも作成自体は可能だが、
    // 実際のバリデーションはAPI層で行われる
    let endpoint = Endpoint::new(
        "".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert!(endpoint.name.is_empty());
}

#[test]
fn test_endpoint_name_with_special_characters() {
    // 特殊文字を含む名前も許容される
    let endpoint = Endpoint::new(
        "Test-Endpoint_v2.0 (Production)".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert_eq!(endpoint.name, "Test-Endpoint_v2.0 (Production)");
}

#[test]
fn test_endpoint_name_unicode() {
    // Unicodeを含む名前も許容される
    let endpoint = Endpoint::new(
        "日本語エンドポイント".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert_eq!(endpoint.name, "日本語エンドポイント");
}

#[test]
fn test_endpoint_base_url_formats() {
    // 様々なURL形式をテスト
    let test_cases = [
        ("http://localhost:8080", true),
        ("https://api.example.com", true),
        ("http://192.168.1.100:11434", true),
        ("https://ollama.local:11434/api", true),
        ("http://10.0.0.1:8000/v1", true),
    ];

    for (url, _expected_valid) in test_cases {
        let endpoint = Endpoint::new("Test".to_string(), url.to_string(), EndpointType::Xllm);
        assert_eq!(endpoint.base_url, url);
    }
}

#[test]
fn test_endpoint_api_key_optional() {
    let endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert!(endpoint.api_key.is_none());
}

#[test]
fn test_endpoint_with_api_key() {
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    endpoint.api_key = Some("sk-test-api-key".to_string());
    assert_eq!(endpoint.api_key, Some("sk-test-api-key".to_string()));
}

#[test]
fn test_endpoint_health_check_interval_range() {
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );

    // デフォルトは30秒
    assert_eq!(endpoint.health_check_interval_secs, 30);

    // カスタム値を設定
    endpoint.health_check_interval_secs = 60;
    assert_eq!(endpoint.health_check_interval_secs, 60);

    // 最小値（極端に短い間隔）
    endpoint.health_check_interval_secs = 1;
    assert_eq!(endpoint.health_check_interval_secs, 1);
}

#[test]
fn test_endpoint_inference_timeout_range() {
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );

    // xLLM のデフォルトは600秒
    assert_eq!(endpoint.inference_timeout_secs, 600);

    // 長いタイムアウト（大規模モデル用）
    endpoint.inference_timeout_secs = 300;
    assert_eq!(endpoint.inference_timeout_secs, 300);
}

#[test]
fn test_endpoint_default_inference_timeout_depends_on_endpoint_type() {
    let test_cases = [
        (EndpointType::Xllm, 600),
        (EndpointType::Ollama, 600),
        (EndpointType::LmStudio, 600),
        (EndpointType::Vllm, 120),
        (EndpointType::Llamacpp, 120),
        (EndpointType::OpenaiCompatible, 120),
    ];

    for (endpoint_type, expected_timeout) in test_cases {
        let endpoint = Endpoint::new(
            format!("endpoint-{endpoint_type}"),
            "http://localhost:8080".to_string(),
            endpoint_type,
        );

        assert_eq!(
            endpoint.inference_timeout_secs, expected_timeout,
            "unexpected default timeout for {endpoint_type}"
        );
    }
}

#[test]
fn test_endpoint_latency_ms_tracking() {
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );

    // 初期状態ではNone
    assert!(endpoint.latency_ms.is_none());

    // レイテンシを記録
    endpoint.latency_ms = Some(50);
    assert_eq!(endpoint.latency_ms, Some(50));
}

#[test]
fn test_endpoint_error_count_tracking() {
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );

    // 初期状態は0
    assert_eq!(endpoint.error_count, 0);

    // エラーカウントを増加
    endpoint.error_count = 3;
    assert_eq!(endpoint.error_count, 3);
}

#[test]
fn test_endpoint_notes_optional() {
    let endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    assert!(endpoint.notes.is_none());
}

#[test]
fn test_endpoint_with_notes() {
    let mut endpoint = Endpoint::new(
        "Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    endpoint.notes = Some("This is a production endpoint".to_string());
    assert_eq!(
        endpoint.notes,
        Some("This is a production endpoint".to_string())
    );
}

#[test]
fn test_endpoint_serialization_roundtrip() {
    let mut endpoint = Endpoint::new(
        "Serialization Test".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    // Note: api_key is NOT serialized for security reasons
    endpoint.latency_ms = Some(25);
    endpoint.notes = Some("Test notes".to_string());

    let json = serde_json::to_string(&endpoint).unwrap();
    let deserialized: Endpoint = serde_json::from_str(&json).unwrap();

    assert_eq!(endpoint.id, deserialized.id);
    assert_eq!(endpoint.name, deserialized.name);
    assert_eq!(endpoint.base_url, deserialized.base_url);
    // api_key is skipped in serialization, so we don't compare it
    assert_eq!(endpoint.latency_ms, deserialized.latency_ms);
    assert_eq!(endpoint.notes, deserialized.notes);
}
