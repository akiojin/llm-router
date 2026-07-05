use super::*;
use crate::audit::types::ActorType;
use crate::audit::writer::{AuditLogWriter, AuditLogWriterConfig};
use crate::balancer::LoadManager;
use crate::db::audit_log::AuditLogStorage;
use crate::AppState;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use chrono::Utc;
use std::sync::Arc;
use tower::ServiceExt;

async fn create_test_pool() -> sqlx::SqlitePool {
    crate::db::test_utils::test_db_pool().await
}

#[test]
fn clamp_archive_total_caps_at_fetch_limit() {
    // fetch 窓を超える深い総数は到達可能な最大件数(MAX_AUDIT_FETCH_LIMIT)に丸める
    assert_eq!(clamp_archive_total(8000, 5000), MAX_AUDIT_FETCH_LIMIT);
    assert_eq!(
        clamp_archive_total(MAX_AUDIT_FETCH_LIMIT, 1),
        MAX_AUDIT_FETCH_LIMIT
    );
}

#[test]
fn clamp_archive_total_preserves_small_totals() {
    // 窓に収まる総数はそのまま返し、ページング整合を壊さない
    assert_eq!(clamp_archive_total(100, 50), 150);
    assert_eq!(clamp_archive_total(0, 0), 0);
}

fn create_test_entry(
    path: &str,
    method: &str,
    actor: ActorType,
    username: Option<&str>,
) -> AuditLogEntry {
    AuditLogEntry {
        id: None,
        timestamp: Utc::now(),
        http_method: method.to_string(),
        request_path: path.to_string(),
        status_code: 200,
        actor_type: actor,
        actor_id: Some("user-1".to_string()),
        actor_username: username.map(|s| s.to_string()),
        api_key_owner_id: None,
        client_ip: Some("127.0.0.1".to_string()),
        duration_ms: Some(10),
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        model_name: None,
        endpoint_id: None,
        detail: None,
        batch_id: None,
        is_migrated: false,
    }
}

async fn create_test_state_with_archive(
    pool: sqlx::SqlitePool,
    archive_pool: Option<sqlx::SqlitePool>,
) -> AppState {
    let request_history = Arc::new(crate::db::request_history::RequestHistoryStorage::new(
        pool.clone(),
    ));
    let endpoint_registry = crate::registry::endpoints::EndpointRegistry::new(pool.clone())
        .await
        .expect("Failed to create endpoint registry");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let http_client = reqwest::Client::new();
    let inference_gate = crate::inference_gate::InferenceGate::default();
    let shutdown = crate::shutdown::ShutdownController::default();
    let update_manager = crate::update::UpdateManager::new(
        http_client.clone(),
        inference_gate.clone(),
        shutdown.clone(),
    )
    .expect("Failed to create update manager");
    let audit_log_storage = Arc::new(AuditLogStorage::new(pool.clone()));
    let audit_log_writer = AuditLogWriter::new(
        AuditLogStorage::new(pool.clone()),
        AuditLogWriterConfig {
            flush_interval_secs: 300,
            buffer_capacity: 100,
            batch_interval_secs: 300,
        },
    );

    AppState {
        load_manager,
        request_history,
        db_pool: pool,
        jwt_secret: "test-secret".to_string(),
        http_client,
        event_bus: crate::events::create_shared_event_bus(),
        endpoint_registry,
        inference_gate,
        shutdown,
        update_manager,
        audit_log_writer,
        audit_log_storage,
        audit_archive_pool: archive_pool,
    }
}

async fn create_test_state(pool: sqlx::SqlitePool) -> AppState {
    create_test_state_with_archive(pool, None).await
}

#[tokio::test]
async fn test_list_audit_logs_empty() {
    let pool = create_test_pool().await;
    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.items.len(), 0);
    assert_eq!(response.total, 0);
    assert_eq!(response.page, 1);
    assert_eq!(response.per_page, 50);
}

#[tokio::test]
async fn test_list_audit_logs_with_data() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let entries = vec![
        create_test_entry("/api/users", "GET", ActorType::User, Some("admin")),
        create_test_entry("/api/endpoints", "POST", ActorType::User, Some("admin")),
        create_test_entry("/v1/chat/completions", "POST", ActorType::ApiKey, None),
    ];
    storage.insert_batch(&entries).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.items.len(), 3);
    assert_eq!(response.total, 3);
}

#[tokio::test]
async fn test_list_audit_logs_with_actor_type_filter() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let entries = vec![
        create_test_entry("/api/users", "GET", ActorType::User, Some("admin")),
        create_test_entry("/v1/chat/completions", "POST", ActorType::ApiKey, None),
    ];
    storage.insert_batch(&entries).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?actor_type=user")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.total, 1);
}

#[tokio::test]
async fn test_list_audit_logs_pagination() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let mut entries = Vec::new();
    for i in 0..5 {
        entries.push(create_test_entry(
            &format!("/api/test/{}", i),
            "GET",
            ActorType::User,
            Some("admin"),
        ));
    }
    storage.insert_batch(&entries).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?page=1&per_page=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.items.len(), 2);
    assert_eq!(response.total, 5);
    assert_eq!(response.page, 1);
    assert_eq!(response.per_page, 2);
}

#[tokio::test]
async fn test_get_audit_log_stats() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let entries = vec![
        create_test_entry("/api/users", "GET", ActorType::User, Some("admin")),
        create_test_entry("/api/users", "POST", ActorType::User, Some("admin")),
        create_test_entry("/v1/chat/completions", "POST", ActorType::ApiKey, None),
    ];
    storage.insert_batch(&entries).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs/stats", get(get_audit_log_stats))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogStatsResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.total_entries, 3);
    assert_eq!(response.last_24h, 3);
    assert!(response.by_method.len() >= 2); // GET and POST
}

#[tokio::test]
async fn test_list_audit_logs_response_contract() {
    // レスポンスJSON構造のcontract test
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let entries = vec![create_test_entry(
        "/api/users",
        "GET",
        ActorType::User,
        Some("admin"),
    )];
    storage.insert_batch(&entries).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();

    // JSON構造の検証
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json.get("items").is_some(), "Response must have 'items'");
    assert!(json.get("total").is_some(), "Response must have 'total'");
    assert!(json.get("page").is_some(), "Response must have 'page'");
    assert!(
        json.get("per_page").is_some(),
        "Response must have 'per_page'"
    );

    // 各itemの必須フィールド検証
    let items = json["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert!(item.get("id").is_some(), "Item must have 'id'");
    assert!(
        item.get("timestamp").is_some(),
        "Item must have 'timestamp'"
    );
    assert!(
        item.get("http_method").is_some(),
        "Item must have 'http_method'"
    );
    assert!(
        item.get("request_path").is_some(),
        "Item must have 'request_path'"
    );
    assert!(
        item.get("status_code").is_some(),
        "Item must have 'status_code'"
    );
    assert!(
        item.get("actor_type").is_some(),
        "Item must have 'actor_type'"
    );
}

#[tokio::test]
async fn test_unsupported_format_returns_400() {
    let pool = create_test_pool().await;
    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?format=csv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_json_format_accepted() {
    let pool = create_test_pool().await;
    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?format=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_list_audit_logs_include_archive() {
    let pool = create_test_pool().await;
    let main_storage = AuditLogStorage::new(pool.clone());
    main_storage
        .insert_batch(&[create_test_entry(
            "/api/main-only",
            "GET",
            ActorType::User,
            Some("admin"),
        )])
        .await
        .unwrap();

    let archive_pool = crate::db::audit_log::create_archive_pool(":memory:")
        .await
        .unwrap();
    let archive_storage = AuditLogStorage::new(archive_pool.clone());
    archive_storage
        .insert_batch(&[create_test_entry(
            "/api/archive-only",
            "GET",
            ActorType::User,
            Some("admin"),
        )])
        .await
        .unwrap();

    let state = create_test_state_with_archive(pool, Some(archive_pool)).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?include_archive=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.total, 2);
    assert_eq!(response.items.len(), 2);
}

#[tokio::test]
async fn test_list_audit_logs_include_archive_with_search() {
    let pool = create_test_pool().await;
    let main_storage = AuditLogStorage::new(pool.clone());
    main_storage
        .insert_batch(&[create_test_entry(
            "/api/main-only",
            "GET",
            ActorType::User,
            Some("admin"),
        )])
        .await
        .unwrap();

    let archive_pool = crate::db::audit_log::create_archive_pool(":memory:")
        .await
        .unwrap();
    let archive_storage = AuditLogStorage::new(archive_pool.clone());
    archive_storage
        .insert_batch(&[create_test_entry(
            "/api/archive-search-target",
            "GET",
            ActorType::User,
            Some("admin"),
        )])
        .await
        .unwrap();

    let state = create_test_state_with_archive(pool, Some(archive_pool)).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?include_archive=true&search=archive-search-target")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.total, 1);
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].request_path, "/api/archive-search-target");
}

#[tokio::test]
async fn test_list_audit_logs_include_archive_deep_pagination() {
    let pool = create_test_pool().await;
    let archive_pool = crate::db::audit_log::create_archive_pool(":memory:")
        .await
        .unwrap();
    let archive_storage = AuditLogStorage::new(archive_pool.clone());

    let now = Utc::now();
    let mut archive_entries = Vec::new();
    for i in 0..1200 {
        let mut entry = create_test_entry(
            &format!("/api/archive-only-{}", i),
            "GET",
            ActorType::User,
            Some("admin"),
        );
        entry.timestamp = now - chrono::Duration::seconds(i);
        archive_entries.push(entry);
    }
    archive_storage
        .insert_batch(&archive_entries)
        .await
        .unwrap();

    let state = create_test_state_with_archive(pool, Some(archive_pool)).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?include_archive=true&page=23&per_page=50")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.total, 1200);
    assert_eq!(response.items.len(), 50);
}

// =========================================================================
// AuditLogQueryParams -> AuditLogFilter conversion tests
// =========================================================================

#[test]
fn test_query_params_to_filter_defaults() {
    let params = AuditLogQueryParams {
        actor_type: None,
        actor_id: None,
        http_method: None,
        request_path: None,
        status_code: None,
        time_from: None,
        time_to: None,
        ip: None,
        search: None,
        page: None,
        per_page: None,
        include_archive: None,
        format: None,
    };
    let filter: AuditLogFilter = params.into();
    assert!(filter.actor_type.is_none());
    assert!(filter.actor_id.is_none());
    assert!(filter.http_method.is_none());
    assert!(filter.request_path.is_none());
    assert!(filter.status_code.is_none());
    assert!(filter.time_from.is_none());
    assert!(filter.time_to.is_none());
    assert!(filter.client_ip.is_none());
    assert!(filter.search_text.is_none());
    assert!(filter.page.is_none());
    assert!(filter.per_page.is_none());
    assert!(filter.include_archive.is_none());
}

#[test]
fn test_query_params_to_filter_all_fields() {
    let now = Utc::now();
    let params = AuditLogQueryParams {
        actor_type: Some("user".to_string()),
        actor_id: Some("uid-1".to_string()),
        http_method: Some("POST".to_string()),
        request_path: Some("/api/test".to_string()),
        status_code: Some(200),
        time_from: Some(now),
        time_to: Some(now),
        ip: Some("192.168.1".to_string()),
        search: Some("query".to_string()),
        page: Some(3),
        per_page: Some(25),
        include_archive: Some(true),
        format: Some("json".to_string()),
    };
    let filter: AuditLogFilter = params.into();
    assert_eq!(filter.actor_type.as_deref(), Some("user"));
    assert_eq!(filter.actor_id.as_deref(), Some("uid-1"));
    assert_eq!(filter.http_method.as_deref(), Some("POST"));
    assert_eq!(filter.request_path.as_deref(), Some("/api/test"));
    assert_eq!(filter.status_code, Some(200));
    assert!(filter.time_from.is_some());
    assert!(filter.time_to.is_some());
    assert_eq!(filter.client_ip.as_deref(), Some("192.168.1"));
    assert_eq!(filter.search_text.as_deref(), Some("query"));
    assert_eq!(filter.page, Some(3));
    assert_eq!(filter.per_page, Some(25));
    assert_eq!(filter.include_archive, Some(true));
}

#[test]
fn test_query_params_search_maps_to_search_text() {
    let params = AuditLogQueryParams {
        actor_type: None,
        actor_id: None,
        http_method: None,
        request_path: None,
        status_code: None,
        time_from: None,
        time_to: None,
        ip: None,
        search: Some("findme".to_string()),
        page: None,
        per_page: None,
        include_archive: None,
        format: None,
    };
    let filter: AuditLogFilter = params.into();
    assert_eq!(filter.search_text, Some("findme".to_string()));
}

// =========================================================================
// AuditLogListResponse serialization tests
// =========================================================================

#[test]
fn test_audit_log_list_response_serialize_empty() {
    let response = AuditLogListResponse {
        items: vec![],
        total: 0,
        page: 1,
        per_page: 50,
    };
    let json = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["items"].as_array().unwrap().len(), 0);
    assert_eq!(parsed["total"], 0);
    assert_eq!(parsed["page"], 1);
    assert_eq!(parsed["per_page"], 50);
}

#[test]
fn test_audit_log_list_response_roundtrip() {
    let response = AuditLogListResponse {
        items: vec![],
        total: 42,
        page: 3,
        per_page: 10,
    };
    let json = serde_json::to_string(&response).unwrap();
    let back: AuditLogListResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total, 42);
    assert_eq!(back.page, 3);
    assert_eq!(back.per_page, 10);
}

// =========================================================================
// AuditLogStatsResponse serialization tests
// =========================================================================

#[test]
fn test_audit_log_stats_response_serialize() {
    let response = AuditLogStatsResponse {
        total_entries: 100,
        by_method: vec![
            MethodCount {
                method: "GET".to_string(),
                count: 60,
            },
            MethodCount {
                method: "POST".to_string(),
                count: 40,
            },
        ],
        by_actor_type: vec![ActorTypeCount {
            actor_type: "user".to_string(),
            count: 100,
        }],
        last_24h: 50,
    };
    let json = serde_json::to_string(&response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["total_entries"], 100);
    assert_eq!(parsed["last_24h"], 50);
    assert_eq!(parsed["by_method"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["by_actor_type"].as_array().unwrap().len(), 1);
}

#[test]
fn test_audit_log_stats_response_roundtrip() {
    let response = AuditLogStatsResponse {
        total_entries: 5,
        by_method: vec![],
        by_actor_type: vec![],
        last_24h: 2,
    };
    let json = serde_json::to_string(&response).unwrap();
    let back: AuditLogStatsResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(back.total_entries, 5);
    assert_eq!(back.last_24h, 2);
}

// =========================================================================
// MethodCount and ActorTypeCount serialization tests
// =========================================================================

#[test]
fn test_method_count_serialize() {
    let mc = MethodCount {
        method: "DELETE".to_string(),
        count: 7,
    };
    let json = serde_json::to_string(&mc).unwrap();
    assert!(json.contains("DELETE"));
    assert!(json.contains("7"));
}

#[test]
fn test_method_count_roundtrip() {
    let mc = MethodCount {
        method: "PATCH".to_string(),
        count: 3,
    };
    let json = serde_json::to_string(&mc).unwrap();
    let back: MethodCount = serde_json::from_str(&json).unwrap();
    assert_eq!(back.method, "PATCH");
    assert_eq!(back.count, 3);
}

#[test]
fn test_actor_type_count_serialize() {
    let atc = ActorTypeCount {
        actor_type: "api_key".to_string(),
        count: 42,
    };
    let json = serde_json::to_string(&atc).unwrap();
    assert!(json.contains("api_key"));
    assert!(json.contains("42"));
}

#[test]
fn test_actor_type_count_roundtrip() {
    let atc = ActorTypeCount {
        actor_type: "anonymous".to_string(),
        count: 10,
    };
    let json = serde_json::to_string(&atc).unwrap();
    let back: ActorTypeCount = serde_json::from_str(&json).unwrap();
    assert_eq!(back.actor_type, "anonymous");
    assert_eq!(back.count, 10);
}

// =========================================================================
// list_audit_logs with various filter combinations
// =========================================================================

#[tokio::test]
async fn test_list_audit_logs_filter_by_http_method() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let entries = vec![
        create_test_entry("/api/a", "GET", ActorType::User, Some("admin")),
        create_test_entry("/api/b", "POST", ActorType::User, Some("admin")),
        create_test_entry("/api/c", "DELETE", ActorType::User, Some("admin")),
    ];
    storage.insert_batch(&entries).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?http_method=POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.total, 1);
    assert_eq!(response.items[0].http_method, "POST");
}

#[tokio::test]
async fn test_list_audit_logs_filter_by_status_code() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let mut ok_entry = create_test_entry("/api/ok", "GET", ActorType::User, Some("admin"));
    ok_entry.status_code = 200;
    let mut err_entry = create_test_entry("/api/err", "GET", ActorType::User, Some("admin"));
    err_entry.status_code = 500;
    storage.insert_batch(&[ok_entry, err_entry]).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?status_code=500")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.total, 1);
    assert_eq!(response.items[0].status_code, 500);
}

#[tokio::test]
async fn test_list_audit_logs_page_2() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let now = Utc::now();
    let mut entries = Vec::new();
    for i in 0..5 {
        let mut entry = create_test_entry(
            &format!("/api/page2/{}", i),
            "GET",
            ActorType::User,
            Some("admin"),
        );
        entry.timestamp = now - chrono::Duration::seconds(i);
        entries.push(entry);
    }
    storage.insert_batch(&entries).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?page=2&per_page=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.page, 2);
    assert_eq!(response.per_page, 2);
    assert_eq!(response.items.len(), 2);
    assert_eq!(response.total, 5);
}

// =========================================================================
// list_audit_logs with search text (FTS)
// =========================================================================

#[tokio::test]
async fn test_list_audit_logs_with_search() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    let entries = vec![
        create_test_entry("/api/users", "GET", ActorType::User, Some("admin")),
        create_test_entry("/v1/chat/completions", "POST", ActorType::ApiKey, None),
    ];
    storage.insert_batch(&entries).await.unwrap();

    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?search=chat")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    // FTS should find the chat completions path
    assert!(response.total >= 1);
}

// =========================================================================
// verify_hash_chain endpoint test
// =========================================================================

#[tokio::test]
async fn test_verify_hash_chain_empty_returns_valid() {
    let pool = create_test_pool().await;
    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs/verify", axum::routing::post(verify_hash_chain))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/audit-logs/verify")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["valid"], true);
    assert_eq!(result["batches_checked"], 0);
}

// =========================================================================
// get_audit_log_stats with empty DB
// =========================================================================

#[tokio::test]
async fn test_get_audit_log_stats_empty() {
    let pool = create_test_pool().await;
    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs/stats", get(get_audit_log_stats))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogStatsResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(response.total_entries, 0);
    assert_eq!(response.last_24h, 0);
    assert!(response.by_method.is_empty());
    assert!(response.by_actor_type.is_empty());
}

// =========================================================================
// include_archive without archive pool (should fallback to main only)
// =========================================================================

#[tokio::test]
async fn test_list_audit_logs_include_archive_without_archive_pool() {
    let pool = create_test_pool().await;
    let storage = AuditLogStorage::new(pool.clone());

    storage
        .insert_batch(&[create_test_entry(
            "/api/main",
            "GET",
            ActorType::User,
            Some("admin"),
        )])
        .await
        .unwrap();

    // No archive pool
    let state = create_test_state(pool).await;
    let app = Router::new()
        .route("/audit-logs", get(list_audit_logs))
        .with_state(state);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/audit-logs?include_archive=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let body = axum::body::to_bytes(res.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let response: AuditLogListResponse = serde_json::from_slice(&body).unwrap();
    // Should still return main DB results when no archive pool
    assert_eq!(response.total, 1);
}

// =========================================================================
// create_test_entry helper verification
// =========================================================================

#[test]
fn test_create_test_entry_fields() {
    let entry = create_test_entry("/api/test", "POST", ActorType::ApiKey, Some("user1"));
    assert_eq!(entry.http_method, "POST");
    assert_eq!(entry.request_path, "/api/test");
    assert_eq!(entry.actor_type, ActorType::ApiKey);
    assert_eq!(entry.actor_username, Some("user1".to_string()));
    assert_eq!(entry.status_code, 200);
    assert_eq!(entry.client_ip, Some("127.0.0.1".to_string()));
    assert!(entry.id.is_none());
    assert!(!entry.is_migrated);
}

#[test]
fn test_create_test_entry_no_username() {
    let entry = create_test_entry("/api/test", "GET", ActorType::Anonymous, None);
    assert!(entry.actor_username.is_none());
    assert_eq!(entry.actor_type, ActorType::Anonymous);
}
