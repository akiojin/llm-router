use super::*;
use crate::common::auth::{Claims, UserRole};
use crate::db::test_utils::{TestAppStateBuilder, TEST_LOCK};
use crate::types::endpoint::{
    DeviceInfo, DeviceType, DownloadStatus, Endpoint, EndpointModel, EndpointStatus, EndpointType,
    GpuDevice, ModelDownloadTask,
};
use axum::{
    body::{to_bytes, Bytes},
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_default_health_check_interval() {
    assert_eq!(default_health_check_interval(), 30);
}

#[test]
fn test_create_endpoint_request_minimal() {
    let json = json!({
        "name": "test-ep",
        "base_url": "http://localhost:8080"
    });
    let req: CreateEndpointRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.name, "test-ep");
    assert_eq!(req.base_url, "http://localhost:8080");
    assert!(req.api_key.is_none());
    assert_eq!(req.health_check_interval_secs, 30);
    assert!(req.inference_timeout_secs.is_none());
    assert!(req.notes.is_none());
    assert!(req.capabilities.is_empty());
}

#[test]
fn test_create_endpoint_request_full() {
    let json = json!({
        "name": "my-endpoint",
        "base_url": "http://gpu-server:11434",
        "api_key": "sk-test-key",
        "health_check_interval_secs": 60,
        "inference_timeout_secs": 300,
        "notes": "Production GPU node",
        "capabilities": ["image_generation"]
    });
    let req: CreateEndpointRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.name, "my-endpoint");
    assert_eq!(req.api_key, Some("sk-test-key".to_string()));
    assert_eq!(req.health_check_interval_secs, 60);
    assert_eq!(req.inference_timeout_secs, Some(300));
    assert_eq!(req.notes, Some("Production GPU node".to_string()));
    assert_eq!(req.capabilities.len(), 1);
}

#[test]
fn test_create_endpoint_request_missing_name_fails() {
    let json = json!({ "base_url": "http://localhost:8080" });
    assert!(serde_json::from_value::<CreateEndpointRequest>(json).is_err());
}

#[test]
fn test_create_endpoint_request_missing_base_url_fails() {
    let json = json!({ "name": "test" });
    assert!(serde_json::from_value::<CreateEndpointRequest>(json).is_err());
}

#[test]
fn test_update_endpoint_request_empty() {
    let json = json!({});
    let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
    assert!(req.name.is_none());
    assert!(req.base_url.is_none());
    assert!(req.api_key.is_none());
    assert!(req.health_check_interval_secs.is_none());
    assert!(req.inference_timeout_secs.is_none());
    assert!(req.notes.is_none());
}

#[test]
fn test_update_endpoint_request_partial() {
    let json = json!({ "name": "new-name", "health_check_interval_secs": 45 });
    let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.name, Some("new-name".to_string()));
    assert_eq!(req.health_check_interval_secs, Some(45));
    assert!(req.base_url.is_none());
}

#[test]
fn test_update_endpoint_request_notes_null_clears() {
    let json = json!({ "notes": null });
    let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.notes, Some(None));
}

#[test]
fn test_update_endpoint_request_notes_with_value() {
    let json = json!({ "notes": "updated note" });
    let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.notes, Some(Some("updated note".to_string())));
}

#[tokio::test]
async fn update_endpoint_syncs_registry_cache() {
    let _guard = TEST_LOCK.lock().await;
    let state = TestAppStateBuilder::new().await.build().await;

    let endpoint = Endpoint::new(
        "sync-cache".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::OpenaiCompatible,
    );
    let endpoint_id = endpoint.id;
    state
        .endpoint_registry
        .add(endpoint)
        .await
        .expect("add endpoint");

    let claims = Claims {
        sub: "admin-user".to_string(),
        role: UserRole::Admin,
        exp: 0,
        must_change_password: false,
        password_changed_at: 0,
        username: None,
    };

    let response = update_endpoint(
        Extension(claims),
        State(state.clone()),
        Path(endpoint_id),
        Json(UpdateEndpointRequest {
            name: None,
            base_url: None,
            api_key: None,
            health_check_interval_secs: None,
            inference_timeout_secs: Some(1),
            notes: None,
        }),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let updated = state
        .endpoint_registry
        .get(endpoint_id)
        .await
        .expect("endpoint remains in registry");
    assert_eq!(updated.inference_timeout_secs, 1);
}

#[tokio::test]
async fn proxy_chat_completions_keeps_endpoint_online_on_client_error() {
    let _guard = TEST_LOCK.lock().await;
    let state = TestAppStateBuilder::new().await.build().await;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "message": "invalid model",
                "type": "invalid_request_error",
                "code": 400
            }
        })))
        .mount(&server)
        .await;

    let mut endpoint = Endpoint::new(
        "proxy-client-error".to_string(),
        server.uri(),
        EndpointType::OpenaiCompatible,
    );
    endpoint.status = EndpointStatus::Online;
    let endpoint_id = endpoint.id;
    state
        .endpoint_registry
        .add(endpoint)
        .await
        .expect("add endpoint to registry");

    let response = proxy_chat_completions(
        State(state.clone()),
        Path(endpoint_id),
        Bytes::from(
            serde_json::to_vec(&json!({
                "model": "bad-model",
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .expect("serialize request"),
        ),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), 1_000_000)
        .await
        .expect("proxy response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("proxy response json");
    assert_eq!(json["error"]["message"], "invalid model");

    let updated = state
        .endpoint_registry
        .get(endpoint_id)
        .await
        .expect("endpoint remains in registry");
    assert_eq!(updated.status, EndpointStatus::Online);
    assert_eq!(updated.last_error, None);
}

#[tokio::test]
async fn proxy_chat_completions_does_not_probe_ollama_load_state_on_success() {
    let _guard = TEST_LOCK.lock().await;
    let state = TestAppStateBuilder::new().await.build().await;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;

    let mut endpoint = Endpoint::new(
        "proxy-ollama-success".to_string(),
        server.uri(),
        EndpointType::Ollama,
    );
    endpoint.status = EndpointStatus::Online;
    let endpoint_id = endpoint.id;
    state
        .endpoint_registry
        .add(endpoint)
        .await
        .expect("add endpoint to registry");

    let response = proxy_chat_completions(
        State(state.clone()),
        Path(endpoint_id),
        Bytes::from(
            serde_json::to_vec(&json!({
                "model": "qwen3:30b",
                "messages": [{"role": "user", "content": "hello"}]
            }))
            .expect("serialize request"),
        ),
    )
    .await
    .into_response();

    assert_eq!(response.status(), StatusCode::OK);

    let requests = server.received_requests().await.expect("recorded requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.path(), "/v1/chat/completions");
}

fn sample_endpoint() -> Endpoint {
    let mut ep = Endpoint::new(
        "test-ep".to_string(),
        "http://localhost:8080".to_string(),
        EndpointType::Xllm,
    );
    ep.status = EndpointStatus::Online;
    ep.latency_ms = Some(42);
    ep.error_count = 0;
    ep.notes = Some("note".to_string());
    ep
}

#[test]
fn test_endpoint_response_from_endpoint() {
    let ep = sample_endpoint();
    let id = ep.id;
    let resp = EndpointResponse::from(ep);
    assert_eq!(resp.id, id);
    assert_eq!(resp.name, "test-ep");
    assert_eq!(resp.base_url, "http://localhost:8080");
    assert_eq!(resp.status, "online");
    assert_eq!(resp.endpoint_type, "xllm");
    assert_eq!(resp.latency_ms, Some(42));
    assert_eq!(resp.error_count, 0);
    assert_eq!(resp.notes, Some("note".to_string()));
    assert!(resp.model_count.is_none());
    assert!(resp.models.is_none());
}

#[test]
fn test_endpoint_response_serialization() {
    let ep = sample_endpoint();
    let resp = EndpointResponse::from(ep);
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["name"], "test-ep");
    assert_eq!(json["status"], "online");
    assert_eq!(json["endpoint_type"], "xllm");
    assert_eq!(json["latency_ms"], 42);
}

#[test]
fn test_endpoint_response_with_device_info() {
    let mut ep = sample_endpoint();
    ep.device_info = Some(DeviceInfo {
        device_type: DeviceType::Gpu,
        gpu_devices: vec![GpuDevice {
            name: "RTX 4090".to_string(),
            total_memory_bytes: 24_000_000_000,
            used_memory_bytes: 8_000_000_000,
        }],
    });
    let resp = EndpointResponse::from(ep);
    assert!(resp.device_info.is_some());
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json["device_info"].is_object());
}

#[test]
fn test_endpoint_response_offline_status() {
    let mut ep = sample_endpoint();
    ep.status = EndpointStatus::Offline;
    ep.latency_ms = None;
    ep.last_error = Some("Connection refused".to_string());
    ep.error_count = 5;
    let resp = EndpointResponse::from(ep);
    assert_eq!(resp.status, "offline");
    assert!(resp.latency_ms.is_none());
    assert_eq!(resp.last_error, Some("Connection refused".to_string()));
    assert_eq!(resp.error_count, 5);
}

#[test]
fn test_endpoint_response_pending_status() {
    let mut ep = sample_endpoint();
    ep.status = EndpointStatus::Pending;
    let resp = EndpointResponse::from(ep);
    assert_eq!(resp.status, "pending");
}

#[test]
fn test_endpoint_response_error_status() {
    let mut ep = sample_endpoint();
    ep.status = EndpointStatus::Error;
    let resp = EndpointResponse::from(ep);
    assert_eq!(resp.status, "error");
}

#[test]
fn test_list_endpoints_response_empty() {
    let resp = ListEndpointsResponse {
        endpoints: vec![],
        total: 0,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["total"], 0);
    assert!(json["endpoints"].as_array().unwrap().is_empty());
}

#[test]
fn test_list_endpoints_query_empty() {
    let json = json!({});
    let q: ListEndpointsQuery = serde_json::from_value(json).unwrap();
    assert!(q.status.is_none());
    assert!(q.endpoint_type.is_none());
}

#[test]
fn test_list_endpoints_query_with_filters() {
    let json = json!({ "status": "online", "type": "xllm" });
    let q: ListEndpointsQuery = serde_json::from_value(json).unwrap();
    assert_eq!(q.status, Some("online".to_string()));
    assert_eq!(q.endpoint_type, Some("xllm".to_string()));
}

#[test]
fn test_endpoint_model_response_from() {
    let model = EndpointModel {
        endpoint_id: Uuid::new_v4(),
        model_id: "llama3".to_string(),
        capabilities: Some(vec!["chat".to_string()]),
        max_tokens: Some(8192),
        last_checked: Some(Utc::now()),
        supported_apis: vec![],
        canonical_name: None,
    };
    let resp = EndpointModelResponse::from(model);
    assert_eq!(resp.model_id, "llama3");
    assert_eq!(resp.capabilities, Some(vec!["chat".to_string()]));
    assert_eq!(resp.max_tokens, Some(8192));
    assert!(resp.last_checked.is_some());
    assert_eq!(resp.canonical_name, None);
}

#[test]
fn test_endpoint_model_response_from_with_canonical_name() {
    let model = EndpointModel {
        endpoint_id: Uuid::new_v4(),
        model_id: "gpt-oss:20b".to_string(),
        capabilities: Some(vec!["chat".to_string()]),
        max_tokens: Some(8192),
        last_checked: None,
        supported_apis: vec![],
        canonical_name: Some("openai/gpt-oss-20b".to_string()),
    };
    let resp = EndpointModelResponse::from(model);
    assert_eq!(resp.model_id, "gpt-oss:20b");
    assert_eq!(resp.canonical_name, Some("openai/gpt-oss-20b".to_string()));

    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["canonical_name"], "openai/gpt-oss-20b");
}

#[test]
fn test_endpoint_model_response_from_minimal() {
    let model = EndpointModel {
        endpoint_id: Uuid::new_v4(),
        model_id: "embed-v1".to_string(),
        capabilities: None,
        max_tokens: None,
        last_checked: None,
        supported_apis: vec![],
        canonical_name: None,
    };
    let resp = EndpointModelResponse::from(model);
    assert_eq!(resp.model_id, "embed-v1");
    assert!(resp.capabilities.is_none());
    assert!(resp.max_tokens.is_none());
    assert!(resp.last_checked.is_none());
}

#[test]
fn test_endpoint_models_response_serialization() {
    let id = Uuid::new_v4();
    let resp = EndpointModelsResponse {
        endpoint_id: id,
        models: vec![EndpointModelResponse {
            model_id: "gpt-4".to_string(),
            capabilities: Some(vec!["chat".to_string(), "embeddings".to_string()]),
            max_tokens: Some(128000),
            last_checked: None,
            canonical_name: None,
        }],
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["endpoint_id"], id.to_string());
    assert_eq!(json["models"].as_array().unwrap().len(), 1);
}

#[test]
fn test_sync_models_response_serialization() {
    let resp = SyncModelsResponse {
        synced_models: vec![],
        added: 3,
        removed: 1,
        updated: 2,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["added"], 3);
    assert_eq!(json["removed"], 1);
    assert_eq!(json["updated"], 2);
}

#[test]
fn test_connection_response_success() {
    let resp = TestConnectionResponse {
        success: true,
        latency_ms: Some(15),
        error: None,
        models_found: Some(vec!["llama3".to_string()]),
        endpoint_info: Some(EndpointTestInfo { model_count: 1 }),
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["success"], true);
    assert_eq!(json["latency_ms"], 15);
}

#[test]
fn test_connection_response_failure() {
    let resp = TestConnectionResponse {
        success: false,
        latency_ms: None,
        error: Some("Connection refused".to_string()),
        models_found: None,
        endpoint_info: None,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["success"], false);
    assert_eq!(json["error"], "Connection refused");
}

#[test]
fn test_download_model_request() {
    let json = json!({ "model": "llama3:8b" });
    let req: DownloadModelRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.model, "llama3:8b");
}

#[test]
fn test_download_model_request_missing_model_fails() {
    let json = json!({});
    assert!(serde_json::from_value::<DownloadModelRequest>(json).is_err());
}

#[test]
fn test_download_task_response_from() {
    let task = ModelDownloadTask {
        id: "task-1".to_string(),
        endpoint_id: Uuid::new_v4(),
        model: "llama3:8b".to_string(),
        status: DownloadStatus::Downloading,
        progress: 45.5,
        speed_mbps: Some(120.0),
        eta_seconds: Some(60),
        error_message: None,
        started_at: Utc::now(),
        completed_at: None,
        filename: None,
    };
    let resp = DownloadTaskResponse::from(task);
    assert_eq!(resp.task_id, "task-1");
    assert_eq!(resp.model, "llama3:8b");
    assert_eq!(resp.status, "downloading");
    assert!((resp.progress - 45.5).abs() < f64::EPSILON);
    assert_eq!(resp.speed_mbps, Some(120.0));
    assert_eq!(resp.eta_seconds, Some(60));
    assert!(resp.error_message.is_none());
}

#[test]
fn test_download_task_response_completed() {
    let task = ModelDownloadTask {
        id: "task-2".to_string(),
        endpoint_id: Uuid::new_v4(),
        model: "mistral:7b".to_string(),
        status: DownloadStatus::Completed,
        progress: 100.0,
        speed_mbps: None,
        eta_seconds: None,
        error_message: None,
        started_at: Utc::now(),
        completed_at: None,
        filename: Some("model.gguf".to_string()),
    };
    let resp = DownloadTaskResponse::from(task);
    assert_eq!(resp.status, "completed");
    assert!((resp.progress - 100.0).abs() < f64::EPSILON);
}

#[test]
fn test_download_task_response_failed() {
    let task = ModelDownloadTask {
        id: "task-3".to_string(),
        endpoint_id: Uuid::new_v4(),
        model: "big-model".to_string(),
        status: DownloadStatus::Failed,
        progress: 30.0,
        speed_mbps: None,
        eta_seconds: None,
        error_message: Some("Disk full".to_string()),
        started_at: Utc::now(),
        completed_at: None,
        filename: None,
    };
    let resp = DownloadTaskResponse::from(task);
    assert_eq!(resp.status, "failed");
    assert_eq!(resp.error_message, Some("Disk full".to_string()));
}

#[test]
fn test_download_task_response_serialization_skips_none() {
    let task = ModelDownloadTask {
        id: "task-4".to_string(),
        endpoint_id: Uuid::new_v4(),
        model: "test".to_string(),
        status: DownloadStatus::Pending,
        progress: 0.0,
        speed_mbps: None,
        eta_seconds: None,
        error_message: None,
        started_at: Utc::now(),
        completed_at: None,
        filename: None,
    };
    let resp = DownloadTaskResponse::from(task);
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json.get("speed_mbps").is_none());
    assert!(json.get("eta_seconds").is_none());
    assert!(json.get("error_message").is_none());
}

#[test]
fn test_download_progress_response_empty() {
    let id = Uuid::new_v4();
    let resp = DownloadProgressResponse {
        endpoint_id: id,
        tasks: vec![],
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["endpoint_id"], id.to_string());
    assert!(json["tasks"].as_array().unwrap().is_empty());
}

#[test]
fn test_model_info_response_serialization() {
    let id = Uuid::new_v4();
    let resp = ModelInfoResponse {
        model_id: "llama3".to_string(),
        endpoint_id: id,
        max_tokens: Some(8192),
        last_checked: Some("2024-01-01T00:00:00Z".to_string()),
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["model_id"], "llama3");
    assert_eq!(json["endpoint_id"], id.to_string());
    assert_eq!(json["max_tokens"], 8192);
}

#[test]
fn test_model_info_response_skips_none() {
    let resp = ModelInfoResponse {
        model_id: "test".to_string(),
        endpoint_id: Uuid::new_v4(),
        max_tokens: None,
        last_checked: None,
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert!(json.get("max_tokens").is_none());
    assert!(json.get("last_checked").is_none());
}

#[test]
fn test_model_info_path_deserialization() {
    let id = Uuid::new_v4();
    let json = json!({ "id": id.to_string(), "model": "llama3:8b" });
    let path: ModelInfoPath = serde_json::from_value(json).unwrap();
    assert_eq!(path.id, id);
    assert_eq!(path.model, "llama3:8b");
}

#[test]
fn test_error_response_serialization() {
    let resp = ErrorResponse {
        error: "Something went wrong".to_string(),
        code: "INTERNAL_ERROR".to_string(),
    };
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["error"], "Something went wrong");
    assert_eq!(json["code"], "INTERNAL_ERROR");
}

#[test]
fn test_ensure_admin_with_admin_role() {
    let claims = Claims {
        sub: "admin-user".to_string(),
        role: UserRole::Admin,
        exp: 0,
        must_change_password: false,
        password_changed_at: 0,
        username: None,
    };
    assert!(ensure_admin(&claims).is_ok());
}

#[test]
fn test_ensure_admin_with_viewer_role() {
    let claims = Claims {
        sub: "viewer-user".to_string(),
        role: UserRole::Viewer,
        exp: 0,
        must_change_password: false,
        password_changed_at: 0,
        username: None,
    };
    assert!(ensure_admin(&claims).is_err());
}

#[test]
fn test_endpoint_test_info_serialization() {
    let info = EndpointTestInfo { model_count: 5 };
    let json = serde_json::to_value(&info).unwrap();
    assert_eq!(json["model_count"], 5);
}

#[test]
fn test_optional_field_absent() {
    let json = json!({});
    let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
    assert!(req.notes.is_none());
}

#[test]
fn test_optional_field_null() {
    let json = json!({ "notes": null });
    let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.notes, Some(None));
}

#[test]
fn test_optional_field_present() {
    let json = json!({ "notes": "hello" });
    let req: UpdateEndpointRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.notes, Some(Some("hello".to_string())));
}

#[test]
fn test_endpoint_response_all_types() {
    for (ep_type, expected) in [
        (EndpointType::Xllm, "xllm"),
        (EndpointType::Ollama, "ollama"),
        (EndpointType::Vllm, "vllm"),
        (EndpointType::OpenaiCompatible, "openai_compatible"),
        (EndpointType::LmStudio, "lm_studio"),
    ] {
        let ep = Endpoint::new("ep".to_string(), "http://localhost".to_string(), ep_type);
        let resp = EndpointResponse::from(ep);
        assert_eq!(resp.endpoint_type, expected);
    }
}

#[test]
fn test_create_endpoint_request_multiple_capabilities() {
    let json = json!({
        "name": "multi-cap",
        "base_url": "http://localhost:8080",
        "capabilities": ["image_generation", "audio_transcription", "audio_speech"]
    });
    let req: CreateEndpointRequest = serde_json::from_value(json).unwrap();
    assert_eq!(req.capabilities.len(), 3);
}

#[test]
fn test_endpoint_response_registered_at_is_rfc3339() {
    let ep = sample_endpoint();
    let resp = EndpointResponse::from(ep);
    assert!(chrono::DateTime::parse_from_rfc3339(&resp.registered_at).is_ok());
}
