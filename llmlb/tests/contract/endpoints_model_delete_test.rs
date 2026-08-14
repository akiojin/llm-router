//! Contract tests for POST /api/endpoints/:id/models/delete.
//!
//! SPEC #575 US-024 / AS-024-1..4: deleting an Ollama model must immediately
//! reconcile the endpoint's persisted models and registry mappings, while
//! unsupported and failed requests must preserve the seeded metadata.

use axum::{
    body::{to_bytes, Body},
    http::{Request, Response, StatusCode},
    Router,
};
use chrono::{DateTime, Utc};
use llmlb::common::auth::{ApiKeyPermission, UserRole};
use llmlb::types::endpoint::{Endpoint, EndpointModel, EndpointStatus, EndpointType, SupportedAPI};
use llmlb::{api, balancer::LoadManager, registry::endpoints::EndpointRegistry, AppState};
use serde_json::{json, Value};
use serial_test::serial;
use sqlx::SqlitePool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const MODEL_ID: &str = "delete-me";
const CANONICAL_MODEL_ID: &str = "example/delete-me-canonical";

struct TestApp {
    app: Router,
    admin_key: String,
    db_pool: SqlitePool,
    endpoint_registry: EndpointRegistry,
}

fn seeded_endpoint(
    name: &str,
    base_url: String,
    endpoint_type: EndpointType,
) -> (Endpoint, EndpointModel) {
    let mut endpoint = Endpoint::new(name.to_string(), base_url, endpoint_type);
    endpoint.status = EndpointStatus::Online;
    endpoint.latency_ms = Some(17);
    endpoint.last_seen = Some(fixed_time());
    endpoint.notes = Some(format!("seeded {name}"));

    let model = EndpointModel {
        endpoint_id: endpoint.id,
        model_id: MODEL_ID.to_string(),
        capabilities: Some(vec!["chat".to_string(), "embeddings".to_string()]),
        max_tokens: Some(32_768),
        last_checked: Some(fixed_time()),
        supported_apis: vec![
            SupportedAPI::ChatCompletions,
            SupportedAPI::Responses,
            SupportedAPI::Embeddings,
        ],
        canonical_name: Some(CANONICAL_MODEL_ID.to_string()),
    };

    (endpoint, model)
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-15T01:02:03Z")
        .expect("fixed timestamp must parse")
        .with_timezone(&Utc)
}

async fn build_app(seeds: Vec<(Endpoint, EndpointModel)>) -> TestApp {
    let db_pool = crate::support::lb::create_test_db_pool().await;
    for (endpoint, model) in &seeds {
        llmlb::db::endpoints::create_endpoint(&db_pool, endpoint)
            .await
            .expect("seed endpoint");
        llmlb::db::endpoints::add_endpoint_model(&db_pool, model)
            .await
            .expect("seed endpoint model");
    }

    // Construct the real registry only after seeding so both caches start from SQLite.
    let endpoint_registry = EndpointRegistry::new(db_pool.clone())
        .await
        .expect("create endpoint registry from seeded database");
    let load_manager = LoadManager::new(Arc::new(endpoint_registry.clone()));
    let request_history = Arc::new(llmlb::db::request_history::RequestHistoryStorage::new(
        db_pool.clone(),
    ));
    let http_client = reqwest::Client::new();
    let inference_gate = llmlb::inference_gate::InferenceGate::default();
    let shutdown = llmlb::shutdown::ShutdownController::default();
    let update_manager = llmlb::update::UpdateManager::new(
        http_client.clone(),
        inference_gate.clone(),
        shutdown.clone(),
    )
    .expect("create update manager");
    let state = AppState {
        load_manager,
        request_history,
        db_pool: db_pool.clone(),
        jwt_secret: "test-secret".to_string(),
        http_client,
        event_bus: llmlb::events::create_shared_event_bus(),
        endpoint_registry: endpoint_registry.clone(),
        inference_gate,
        shutdown,
        update_manager,
        audit_log_writer: llmlb::audit::writer::AuditLogWriter::new(
            llmlb::db::audit_log::AuditLogStorage::new(db_pool.clone()),
            llmlb::audit::writer::AuditLogWriterConfig::default(),
        ),
        audit_log_storage: Arc::new(llmlb::db::audit_log::AuditLogStorage::new(db_pool.clone())),
        audit_archive_pool: None,
    };

    let password_hash =
        llmlb::auth::password::hash_password("password123").expect("hash admin password");
    let admin_user =
        llmlb::db::users::create(&db_pool, "admin", &password_hash, UserRole::Admin, false)
            .await
            .expect("create admin user");
    let admin_key = llmlb::db::api_keys::create(
        &db_pool,
        "admin-key",
        admin_user.id,
        None,
        ApiKeyPermission::all(),
    )
    .await
    .expect("create admin API key")
    .key;

    TestApp {
        app: api::create_app(state),
        admin_key,
        db_pool,
        endpoint_registry,
    }
}

fn admin_request(admin_key: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {admin_key}"))
}

async fn request_delete(app: &TestApp, endpoint_id: Uuid) -> Response<Body> {
    app.app
        .clone()
        .oneshot(
            admin_request(&app.admin_key)
                .method("POST")
                .uri(format!("/api/endpoints/{endpoint_id}/models/delete"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({ "model": MODEL_ID }))
                        .expect("serialize delete request"),
                ))
                .expect("build delete request"),
        )
        .await
        .expect("delete request must complete")
}

async fn response_json(response: Response<Body>) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&body).expect("response body must be JSON")
}

async fn list_models_over_http(app: &TestApp, endpoint_id: Uuid) -> Vec<Value> {
    let response = app
        .app
        .clone()
        .oneshot(
            admin_request(&app.admin_key)
                .method("GET")
                .uri(format!("/api/endpoints/{endpoint_id}/models"))
                .body(Body::empty())
                .expect("build model list request"),
        )
        .await
        .expect("model list request must complete");
    assert_eq!(response.status(), StatusCode::OK);

    let body = response_json(response).await;
    body["models"]
        .as_array()
        .expect("models must be an array")
        .clone()
}

async fn load_seeded_model(pool: &SqlitePool, endpoint_id: Uuid) -> EndpointModel {
    let models = llmlb::db::endpoints::list_endpoint_models(pool, endpoint_id)
        .await
        .expect("load endpoint models from SQLite");
    assert_eq!(models.len(), 1, "seeded endpoint must retain one model");
    models.into_iter().next().expect("one model must exist")
}

fn assert_full_metadata_eq(expected: &EndpointModel, actual: &EndpointModel) {
    assert_eq!(actual.endpoint_id, expected.endpoint_id);
    assert_eq!(actual.model_id, expected.model_id);
    assert_eq!(actual.capabilities, expected.capabilities);
    assert_eq!(actual.max_tokens, expected.max_tokens);
    assert_eq!(actual.last_checked, expected.last_checked);
    assert_eq!(actual.supported_apis, expected.supported_apis);
    assert_eq!(actual.canonical_name, expected.canonical_name);
}

async fn mount_successful_ollama(server: &MockServer) {
    Mock::given(method("DELETE"))
        .and(path("/api/delete"))
        .and(body_json(json!({ "name": MODEL_ID })))
        .respond_with(ResponseTemplate::new(204))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": []
        })))
        .mount(server)
        .await;
}

#[tokio::test]
#[serial]
async fn as_024_1_ollama_delete_returns_204_and_resyncs_models() {
    let upstream = MockServer::start().await;
    mount_successful_ollama(&upstream).await;
    let (endpoint, model) =
        seeded_endpoint("successful Ollama", upstream.uri(), EndpointType::Ollama);
    let endpoint_id = endpoint.id;
    let app = build_app(vec![(endpoint, model)]).await;

    assert_eq!(
        app.endpoint_registry.find_by_model(MODEL_ID).await.len(),
        1,
        "precondition: registry mapping must be seeded"
    );

    let response = request_delete(&app, endpoint_id).await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(
        app.endpoint_registry
            .find_by_model(CANONICAL_MODEL_ID)
            .await
            .is_empty(),
        "successful delete must immediately remove the stale canonical registry mapping"
    );
    assert!(
        list_models_over_http(&app, endpoint_id).await.is_empty(),
        "successful delete must immediately remove the stale SQLite model row"
    );
    assert!(
        app.endpoint_registry
            .find_by_model(MODEL_ID)
            .await
            .is_empty(),
        "successful delete must immediately remove the stale registry mapping"
    );

    let requests = upstream
        .received_requests()
        .await
        .expect("upstream request recording must be enabled");
    assert_eq!(
        requests.len(),
        2,
        "delete must be followed by one model resync"
    );
    assert_eq!(requests[0].method.as_str(), "DELETE");
    assert_eq!(requests[0].url.path(), "/api/delete");
    assert_eq!(requests[1].method.as_str(), "GET");
    assert_eq!(requests[1].url.path(), "/v1/models");
}

#[tokio::test]
#[serial]
async fn as_024_2_unsupported_endpoint_types_preserve_metadata() {
    let upstream = MockServer::start().await;
    let cases = [
        EndpointType::Xllm,
        EndpointType::LmStudio,
        EndpointType::Llamacpp,
    ];
    let seeds: Vec<_> = cases
        .iter()
        .map(|endpoint_type| {
            seeded_endpoint(
                endpoint_type.as_str(),
                format!("{}/{}", upstream.uri(), endpoint_type.as_str()),
                *endpoint_type,
            )
        })
        .collect();
    let endpoint_ids: Vec<_> = seeds.iter().map(|(endpoint, _)| endpoint.id).collect();
    let app = build_app(seeds).await;

    for (endpoint_type, endpoint_id) in cases.into_iter().zip(endpoint_ids) {
        let before = load_seeded_model(&app.db_pool, endpoint_id).await;
        let response = request_delete(&app, endpoint_id).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(response).await,
            json!({
                "error": format!(
                    "Model delete is not supported for {} endpoints",
                    endpoint_type.as_str()
                )
            })
        );
        let after = load_seeded_model(&app.db_pool, endpoint_id).await;
        assert_full_metadata_eq(&before, &after);
    }

    assert!(
        upstream
            .received_requests()
            .await
            .expect("upstream request recording must be enabled")
            .is_empty(),
        "unsupported endpoint types must not call an upstream"
    );
}

#[tokio::test]
#[serial]
async fn as_024_3_scope_excluded_endpoint_types_are_rejected() {
    let upstream = MockServer::start().await;
    let cases = [EndpointType::Vllm, EndpointType::OpenaiCompatible];
    let seeds: Vec<_> = cases
        .iter()
        .map(|endpoint_type| {
            seeded_endpoint(
                endpoint_type.as_str(),
                format!("{}/{}", upstream.uri(), endpoint_type.as_str()),
                *endpoint_type,
            )
        })
        .collect();
    let endpoint_ids: Vec<_> = seeds.iter().map(|(endpoint, _)| endpoint.id).collect();
    let app = build_app(seeds).await;

    for endpoint_id in endpoint_ids {
        let before = load_seeded_model(&app.db_pool, endpoint_id).await;
        let response = request_delete(&app, endpoint_id).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let after = load_seeded_model(&app.db_pool, endpoint_id).await;
        assert_full_metadata_eq(&before, &after);
    }

    assert!(
        upstream
            .received_requests()
            .await
            .expect("upstream request recording must be enabled")
            .is_empty(),
        "scope-excluded endpoint types must not call an upstream"
    );
}

#[tokio::test]
#[serial]
async fn as_024_4_delete_requests_are_isolated_per_endpoint() {
    let successful_upstream = MockServer::start().await;
    mount_successful_ollama(&successful_upstream).await;
    let failed_upstream = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/delete"))
        .and(body_json(json!({ "name": MODEL_ID })))
        .respond_with(ResponseTemplate::new(500).set_body_string("upstream failure"))
        .mount(&failed_upstream)
        .await;

    let (successful_endpoint, successful_model) = seeded_endpoint(
        "successful Ollama",
        successful_upstream.uri(),
        EndpointType::Ollama,
    );
    let successful_endpoint_id = successful_endpoint.id;
    let (failed_endpoint, failed_model) =
        seeded_endpoint("failed Ollama", failed_upstream.uri(), EndpointType::Ollama);
    let failed_endpoint_id = failed_endpoint.id;
    let app = build_app(vec![
        (successful_endpoint, successful_model),
        (failed_endpoint, failed_model),
    ])
    .await;
    let failed_metadata_before = load_seeded_model(&app.db_pool, failed_endpoint_id).await;

    let successful_response = request_delete(&app, successful_endpoint_id).await;
    assert_eq!(successful_response.status(), StatusCode::NO_CONTENT);
    let failed_response = request_delete(&app, failed_endpoint_id).await;
    assert_eq!(failed_response.status(), StatusCode::BAD_REQUEST);

    assert!(
        list_models_over_http(&app, successful_endpoint_id)
            .await
            .is_empty(),
        "a later failure must not roll back the successful endpoint's SQLite reconciliation"
    );
    let failed_models = list_models_over_http(&app, failed_endpoint_id).await;
    assert_eq!(failed_models.len(), 1);
    assert_eq!(failed_models[0]["model_id"], MODEL_ID);
    let failed_metadata_after = load_seeded_model(&app.db_pool, failed_endpoint_id).await;
    assert_full_metadata_eq(&failed_metadata_before, &failed_metadata_after);

    let resolved = app.endpoint_registry.find_by_model(MODEL_ID).await;
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].id, failed_endpoint_id);
    let canonical_resolved = app
        .endpoint_registry
        .find_by_model(CANONICAL_MODEL_ID)
        .await;
    assert_eq!(canonical_resolved.len(), 1);
    assert_eq!(canonical_resolved[0].id, failed_endpoint_id);

    let successful_requests = successful_upstream
        .received_requests()
        .await
        .expect("successful upstream request recording must be enabled");
    assert_eq!(successful_requests.len(), 2);
    assert_eq!(successful_requests[0].url.path(), "/api/delete");
    assert_eq!(successful_requests[1].url.path(), "/v1/models");
    let failed_requests = failed_upstream
        .received_requests()
        .await
        .expect("failed upstream request recording must be enabled");
    assert_eq!(failed_requests.len(), 1);
    assert_eq!(failed_requests[0].url.path(), "/api/delete");
}
