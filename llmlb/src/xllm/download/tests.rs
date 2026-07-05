use super::*;
use wiremock::{
    matchers::{header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

#[test]
fn test_download_request_serialization() {
    let request = DownloadRequest {
        repo: "bartowski/Llama-3.2-1B-Instruct-GGUF".to_string(),
        filename: Some("Llama-3.2-1B-Instruct-Q4_K_M.gguf".to_string()),
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("bartowski/Llama-3.2-1B-Instruct-GGUF"));
    assert!(json.contains("Llama-3.2-1B-Instruct-Q4_K_M.gguf"));
}

#[test]
fn test_download_request_without_filename() {
    let request = DownloadRequest {
        repo: "bartowski/Llama-3.2-1B-Instruct-GGUF".to_string(),
        filename: None,
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("bartowski/Llama-3.2-1B-Instruct-GGUF"));
    assert!(!json.contains("filename"));
}

#[test]
fn test_download_progress_deserialization() {
    let json = r#"{
            "task_id": "task-123",
            "model": "llama-3.2-1b",
            "status": "downloading",
            "progress": 45.5,
            "speed_mbps": 12.3,
            "eta_seconds": 120
        }"#;

    let progress: DownloadProgressResponse = serde_json::from_str(json).unwrap();
    assert_eq!(progress.task_id, "task-123");
    assert_eq!(progress.status, "downloading");
    assert_eq!(progress.progress, 45.5);
    assert_eq!(progress.speed_mbps, Some(12.3));
    assert_eq!(progress.eta_seconds, Some(120));
}

#[test]
fn test_download_progress_completed() {
    let json = r#"{
            "task_id": "task-123",
            "model": "llama-3.2-1b",
            "status": "completed",
            "progress": 100.0,
            "filename": "model.gguf"
        }"#;

    let progress: DownloadProgressResponse = serde_json::from_str(json).unwrap();
    assert_eq!(progress.status, "completed");
    assert_eq!(progress.progress, 100.0);
    assert_eq!(progress.filename, Some("model.gguf".to_string()));
}

#[test]
fn test_download_progress_failed() {
    let json = r#"{
            "task_id": "task-123",
            "model": "llama-3.2-1b",
            "status": "failed",
            "progress": 25.0,
            "error": "Connection timeout"
        }"#;

    let progress: DownloadProgressResponse = serde_json::from_str(json).unwrap();
    assert_eq!(progress.status, "failed");
    assert_eq!(progress.error, Some("Connection timeout".to_string()));
}

#[test]
fn test_download_error_display() {
    let err = DownloadError::XllmError {
        status: 404,
        message: "Model not found".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "xLLM returned error: 404 - Model not found"
    );
}

// --- DownloadError additional tests ---

#[test]
fn test_download_error_invalid_response_display() {
    let err = DownloadError::InvalidResponse("bad json".to_string());
    assert_eq!(err.to_string(), "Invalid response format: bad json");
}

#[test]
fn test_download_error_xllm_error_500() {
    let err = DownloadError::XllmError {
        status: 500,
        message: "Internal server error".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "xLLM returned error: 500 - Internal server error"
    );
}

#[test]
fn test_download_error_xllm_error_429() {
    let err = DownloadError::XllmError {
        status: 429,
        message: "Rate limited".to_string(),
    };
    assert!(err.to_string().contains("429"));
    assert!(err.to_string().contains("Rate limited"));
}

#[test]
fn test_download_error_xllm_error_empty_message() {
    let err = DownloadError::XllmError {
        status: 503,
        message: "".to_string(),
    };
    assert_eq!(err.to_string(), "xLLM returned error: 503 - ");
}

#[test]
fn test_download_error_debug_format() {
    let err = DownloadError::InvalidResponse("test".to_string());
    let debug = format!("{:?}", err);
    assert!(debug.contains("InvalidResponse"));
}

// --- DownloadRequest additional tests ---

#[test]
fn test_download_request_serialization_with_both_fields() {
    let request = DownloadRequest {
        repo: "test/repo".to_string(),
        filename: Some("model.gguf".to_string()),
    };
    let json: serde_json::Value = serde_json::to_value(&request).unwrap();
    assert_eq!(json["repo"], "test/repo");
    assert_eq!(json["filename"], "model.gguf");
}

#[test]
fn test_download_request_skip_serializing_none_filename() {
    let request = DownloadRequest {
        repo: "test/repo".to_string(),
        filename: None,
    };
    let json: serde_json::Value = serde_json::to_value(&request).unwrap();
    assert_eq!(json["repo"], "test/repo");
    assert!(json.get("filename").is_none());
}

#[test]
fn test_download_request_clone() {
    let request = DownloadRequest {
        repo: "bartowski/Llama-3.2-1B-Instruct-GGUF".to_string(),
        filename: Some("model.gguf".to_string()),
    };
    let cloned = request.clone();
    assert_eq!(cloned.repo, request.repo);
    assert_eq!(cloned.filename, request.filename);
}

#[test]
fn test_download_request_debug() {
    let request = DownloadRequest {
        repo: "test/repo".to_string(),
        filename: None,
    };
    let debug = format!("{:?}", request);
    assert!(debug.contains("test/repo"));
}

#[test]
fn test_download_request_empty_repo() {
    let request = DownloadRequest {
        repo: "".to_string(),
        filename: None,
    };
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("\"repo\":\"\""));
}

// --- DownloadProgressResponse additional tests ---

#[test]
fn test_download_progress_minimal_fields() {
    let json = r#"{
            "task_id": "t1",
            "model": "m1",
            "status": "pending",
            "progress": 0.0
        }"#;
    let progress: DownloadProgressResponse = serde_json::from_str(json).unwrap();
    assert_eq!(progress.task_id, "t1");
    assert_eq!(progress.model, "m1");
    assert_eq!(progress.status, "pending");
    assert_eq!(progress.progress, 0.0);
    assert!(progress.speed_mbps.is_none());
    assert!(progress.eta_seconds.is_none());
    assert!(progress.error.is_none());
    assert!(progress.filename.is_none());
}

#[test]
fn test_download_progress_all_fields() {
    let json = r#"{
            "task_id": "task-all",
            "model": "model-all",
            "status": "downloading",
            "progress": 55.5,
            "speed_mbps": 25.3,
            "eta_seconds": 60,
            "error": null,
            "filename": null
        }"#;
    let progress: DownloadProgressResponse = serde_json::from_str(json).unwrap();
    assert_eq!(progress.speed_mbps, Some(25.3));
    assert_eq!(progress.eta_seconds, Some(60));
    assert!(progress.error.is_none());
    assert!(progress.filename.is_none());
}

#[test]
fn test_download_progress_serde_roundtrip() {
    let original = DownloadProgressResponse {
        task_id: "rt-task".to_string(),
        model: "rt-model".to_string(),
        status: "downloading".to_string(),
        progress: 42.0,
        speed_mbps: Some(10.5),
        eta_seconds: Some(30),
        error: None,
        filename: None,
    };
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: DownloadProgressResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.task_id, "rt-task");
    assert_eq!(deserialized.progress, 42.0);
    assert_eq!(deserialized.speed_mbps, Some(10.5));
}

#[test]
fn test_download_progress_skip_serializing_none_fields() {
    let progress = DownloadProgressResponse {
        task_id: "t1".to_string(),
        model: "m1".to_string(),
        status: "pending".to_string(),
        progress: 0.0,
        speed_mbps: None,
        eta_seconds: None,
        error: None,
        filename: None,
    };
    let json: serde_json::Value = serde_json::to_value(&progress).unwrap();
    assert!(json.get("speed_mbps").is_none());
    assert!(json.get("eta_seconds").is_none());
    assert!(json.get("error").is_none());
    assert!(json.get("filename").is_none());
}

#[test]
fn test_download_progress_clone() {
    let progress = DownloadProgressResponse {
        task_id: "t1".to_string(),
        model: "m1".to_string(),
        status: "completed".to_string(),
        progress: 100.0,
        speed_mbps: Some(50.0),
        eta_seconds: None,
        error: None,
        filename: Some("model.gguf".to_string()),
    };
    let cloned = progress.clone();
    assert_eq!(cloned.task_id, progress.task_id);
    assert_eq!(cloned.progress, progress.progress);
    assert_eq!(cloned.filename, progress.filename);
}

#[test]
fn test_download_progress_100_percent() {
    let progress = DownloadProgressResponse {
        task_id: "t".to_string(),
        model: "m".to_string(),
        status: "completed".to_string(),
        progress: 100.0,
        speed_mbps: None,
        eta_seconds: None,
        error: None,
        filename: Some("output.gguf".to_string()),
    };
    assert_eq!(progress.progress, 100.0);
    assert_eq!(progress.status, "completed");
}

#[test]
fn test_download_progress_zero_speed() {
    let json = r#"{
            "task_id": "t1",
            "model": "m1",
            "status": "downloading",
            "progress": 10.0,
            "speed_mbps": 0.0
        }"#;
    let progress: DownloadProgressResponse = serde_json::from_str(json).unwrap();
    assert_eq!(progress.speed_mbps, Some(0.0));
}

#[test]
fn test_download_progress_cancelled_status() {
    let json = r#"{
            "task_id": "t-cancel",
            "model": "m1",
            "status": "cancelled",
            "progress": 50.0
        }"#;
    let progress: DownloadProgressResponse = serde_json::from_str(json).unwrap();
    assert_eq!(progress.status, "cancelled");
    assert_eq!(progress.progress, 50.0);
}

#[tokio::test]
async fn download_model_success_returns_task_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/models/download"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "task_id": "task-xyz",
            "model": "llama-3.2-1b",
            "status": "pending"
        })))
        .mount(&server)
        .await;

    let client = Client::new();
    let req = DownloadRequest {
        repo: "bartowski/Llama-3.2-1B-Instruct-GGUF".to_string(),
        filename: None,
    };

    let task_id = download_model(
        &client,
        &format!("{}/", server.uri()),
        Some("sk-test"),
        &req,
    )
    .await
    .expect("download_model should succeed");
    assert_eq!(task_id, "task-xyz");
}

#[tokio::test]
async fn download_model_maps_non_success_to_xllm_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/models/download"))
        .respond_with(ResponseTemplate::new(503).set_body_string("service unavailable"))
        .mount(&server)
        .await;

    let client = Client::new();
    let req = DownloadRequest {
        repo: "repo/model".to_string(),
        filename: Some("model.gguf".to_string()),
    };

    let err = download_model(&client, &server.uri(), None, &req)
        .await
        .expect_err("download_model should fail");
    match err {
        DownloadError::XllmError { status, message } => {
            assert_eq!(status, 503);
            assert!(message.contains("service unavailable"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn download_model_maps_invalid_json_to_invalid_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/models/download"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"status":"ok"}"#))
        .mount(&server)
        .await;

    let client = Client::new();
    let req = DownloadRequest {
        repo: "repo/model".to_string(),
        filename: None,
    };

    let err = download_model(&client, &server.uri(), None, &req)
        .await
        .expect_err("download_model should fail on invalid schema");
    match err {
        DownloadError::InvalidResponse(msg) => {
            assert!(msg.contains("Failed to parse download response"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn get_download_progress_success_returns_payload() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/download/progress"))
        .and(query_param("task_id", "task-123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "task_id": "task-123",
            "model": "llama-3.2-1b",
            "status": "downloading",
            "progress": 12.5,
            "speed_mbps": 8.2,
            "eta_seconds": 90
        })))
        .mount(&server)
        .await;

    let client = Client::new();
    let progress = get_download_progress(&client, &server.uri(), None, "task-123")
        .await
        .expect("get_download_progress should succeed");

    assert_eq!(progress.task_id, "task-123");
    assert_eq!(progress.status, "downloading");
    assert_eq!(progress.progress, 12.5);
    assert_eq!(progress.speed_mbps, Some(8.2));
    assert_eq!(progress.eta_seconds, Some(90));
}

#[tokio::test]
async fn get_download_progress_maps_non_success_to_xllm_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/download/progress"))
        .and(query_param("task_id", "task-404"))
        .respond_with(ResponseTemplate::new(404).set_body_string("task not found"))
        .mount(&server)
        .await;

    let client = Client::new();
    let err = get_download_progress(&client, &server.uri(), None, "task-404")
        .await
        .expect_err("get_download_progress should fail");
    match err {
        DownloadError::XllmError { status, message } => {
            assert_eq!(status, 404);
            assert!(message.contains("task not found"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[tokio::test]
async fn get_download_progress_maps_invalid_json_to_invalid_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/download/progress"))
        .and(query_param("task_id", "task-bad"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"task_id":"x"}"#))
        .mount(&server)
        .await;

    let client = Client::new();
    let err = get_download_progress(&client, &server.uri(), None, "task-bad")
        .await
        .expect_err("get_download_progress should fail on invalid schema");
    match err {
        DownloadError::InvalidResponse(msg) => {
            assert!(msg.contains("Failed to parse progress response"));
        }
        other => panic!("unexpected error: {other:?}"),
    }
}
