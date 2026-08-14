//! SPEC #585 FR-027 / Issue #696: 公開 API ルートの契約テスト。
//!
//! 既存の unit / integration / E2E テストと重複しないよう、本番 Router への接続、
//! 認証境界、HTTP status、主要な response shape に限定して検証する。

use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    response::Response,
    Router,
};
use llmlb::common::auth::UserRole;
use reqwest::Client;
use serde_json::{json, Value};
use serial_test::serial;
use std::{ffi::OsString, net::SocketAddr, time::Duration};
use tower::ServiceExt;
use uuid::Uuid;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct ScopedEnvVar {
    key: &'static str,
    previous: Option<OsString>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, previous }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        if let Some(value) = self.previous.as_ref() {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

async fn build_app_with_admin_jwt() -> (Router, String) {
    let (app, db_pool) = crate::support::lb::create_test_lb_default_auth().await;
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let admin = llmlb::db::users::create(
        &db_pool,
        "route-contract-admin",
        &password_hash,
        UserRole::Admin,
        false,
    )
    .await
    .expect("create route contract admin");
    let jwt = llmlb::auth::jwt::create_jwt(
        &admin.id.to_string(),
        UserRole::Admin,
        &crate::support::lb::test_jwt_secret(),
        false,
        0,
    )
    .expect("create route contract admin jwt");

    (app, jwt)
}

async fn send_request(
    app: &Router,
    method: Method,
    uri: &str,
    jwt: Option<&str>,
    json_body: Option<Value>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(jwt) = jwt {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {jwt}"));
    }
    let body = if let Some(value) = json_body {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
        Body::from(serde_json::to_vec(&value).expect("serialize request body"))
    } else {
        Body::empty()
    };

    app.clone()
        .oneshot(builder.body(body).expect("build request"))
        .await
        .expect("route response")
}

async fn response_json(response: Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read response body");
    serde_json::from_slice(&body).expect("response must be JSON")
}

fn websocket_request(client: &Client, addr: SocketAddr, path: &str) -> reqwest::RequestBuilder {
    client
        .get(format!("http://{addr}{path}"))
        .header(header::CONNECTION, "Upgrade")
        .header(header::UPGRADE, "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
}

#[tokio::test]
#[serial]
async fn protected_route_methods_reject_missing_authentication() {
    let (app, _db_pool) = crate::support::lb::create_test_lb_default_auth().await;
    let missing_run_id = Uuid::new_v4();
    let benchmark_run_uri = format!("/api/benchmarks/tps/{missing_run_id}");
    let cases = [
        (Method::GET, "/api/dashboard/audit-logs", None),
        (Method::GET, "/api/dashboard/audit-logs/stats", None),
        (Method::POST, "/api/dashboard/audit-logs/verify", None),
        (
            Method::POST,
            "/api/benchmarks/tps",
            Some(json!({ "model": "contract-model" })),
        ),
        (Method::GET, benchmark_run_uri.as_str(), None),
        (Method::GET, "/api/catalog/search?q=contract", None),
        (Method::POST, "/api/system/update/check", None),
        (Method::POST, "/api/system/update/apply", None),
        (Method::POST, "/api/system/update/apply/force", None),
        (
            Method::POST,
            "/api/system/update/schedule",
            Some(json!({ "mode": "idle" })),
        ),
        (Method::GET, "/api/system/update/schedule", None),
        (Method::DELETE, "/api/system/update/schedule", None),
        (Method::POST, "/api/system/update/rollback", None),
        (Method::GET, "/api/metrics/cloud", None),
        (Method::GET, "/api/models/hub", None),
    ];

    for (method, uri, body) in cases {
        let response = send_request(&app, method.clone(), uri, None, body).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {uri} must reject an unauthenticated request"
        );
    }
}

#[tokio::test]
#[serial]
async fn system_update_routes_expose_only_documented_methods() {
    let (app, jwt) = build_app_with_admin_jwt().await;
    let cases: [(&str, &[&str]); 5] = [
        ("/api/system/update/check", &["POST"]),
        ("/api/system/update/apply", &["POST"]),
        ("/api/system/update/apply/force", &["POST"]),
        (
            "/api/system/update/schedule",
            &["GET", "HEAD", "POST", "DELETE"],
        ),
        ("/api/system/update/rollback", &["POST"]),
    ];

    for (uri, expected_methods) in cases {
        let response = send_request(&app, Method::OPTIONS, uri, Some(&jwt), None).await;
        assert_eq!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "OPTIONS {uri} must expose the registered method contract"
        );
        let allow = response
            .headers()
            .get(header::ALLOW)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_else(|| panic!("OPTIONS {uri} must include an Allow header"));
        let allowed_methods: Vec<_> = allow.split(',').map(str::trim).collect();
        assert_eq!(
            allowed_methods.len(),
            expected_methods.len(),
            "OPTIONS {uri} returned an unexpected Allow set: {allow}"
        );
        for method in expected_methods {
            assert!(
                allowed_methods.contains(method),
                "OPTIONS {uri} must allow {method}; got {allow}"
            );
        }
    }
}

#[tokio::test]
#[serial]
async fn audit_log_routes_return_documented_response_shapes() {
    let (app, jwt) = build_app_with_admin_jwt().await;

    let response = send_request(
        &app,
        Method::GET,
        "/api/dashboard/audit-logs",
        Some(&jwt),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["items"].is_array());
    assert!(body["total"].is_i64());
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 50);

    let response = send_request(
        &app,
        Method::GET,
        "/api/dashboard/audit-logs/stats",
        Some(&jwt),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["total_entries"].is_i64());
    assert!(body["by_method"].is_array());
    assert!(body["by_actor_type"].is_array());
    assert!(body["last_24h"].is_i64());

    let response = send_request(
        &app,
        Method::POST,
        "/api/dashboard/audit-logs/verify",
        Some(&jwt),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["valid"].is_boolean());
    assert!(body["batches_checked"].is_i64());
    assert!(body.get("tampered_batch").is_some());
    assert!(body.get("message").is_some());
}

#[tokio::test]
#[serial]
async fn benchmark_routes_return_acceptance_and_lookup_contracts() {
    let (app, jwt) = build_app_with_admin_jwt().await;
    let response = send_request(
        &app,
        Method::POST,
        "/api/benchmarks/tps",
        Some(&jwt),
        Some(json!({
            "model": "contract-model",
            "total_requests": 1,
            "concurrency": 1,
            "max_tokens": 1
        })),
    )
    .await;
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let accepted = response_json(response).await;
    assert_eq!(accepted["status"], "running");
    let run_id = accepted["run_id"]
        .as_str()
        .expect("accepted response run_id");

    let response = send_request(
        &app,
        Method::GET,
        &format!("/api/benchmarks/tps/{run_id}"),
        Some(&jwt),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let run = response_json(response).await;
    assert_eq!(run["run_id"], run_id);
    assert_eq!(run["request"]["model"], "contract-model");
    assert!(matches!(
        run["status"].as_str(),
        Some("running" | "failed" | "completed")
    ));
}

#[tokio::test]
#[serial]
async fn catalog_search_route_returns_models_contract() {
    let mock = MockServer::start().await;
    let query = format!("issue-696-{}", Uuid::new_v4());
    Mock::given(method("GET"))
        .and(path("/api/models"))
        .and(query_param("search", query.as_str()))
        .and(query_param("limit", "1"))
        .and(query_param("filter", "gguf"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": "owner/contract-model-GGUF",
            "tags": ["gguf"],
            "downloads": 7,
            "siblings": [{ "rfilename": "model.Q4_K_M.gguf" }]
        }])))
        .mount(&mock)
        .await;
    let hf_base_url = ScopedEnvVar::set("HF_BASE_URL", mock.uri());

    let (app, jwt) = build_app_with_admin_jwt().await;
    let response = send_request(
        &app,
        Method::GET,
        &format!("/api/catalog/search?q={query}&limit=1"),
        Some(&jwt),
        None,
    )
    .await;
    drop(hf_base_url);

    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body["models"].is_array());
    assert_eq!(body["models"][0]["repo_id"], "owner/contract-model-GGUF");
    assert_eq!(body["models"][0]["downloads"], 7);
}

#[tokio::test]
#[serial]
async fn safe_read_routes_return_documented_contracts() {
    let (app, jwt) = build_app_with_admin_jwt().await;

    let response = send_request(
        &app,
        Method::GET,
        "/api/system/update/schedule",
        Some(&jwt),
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body.get("schedule").is_some());
    assert!(body["schedule"].is_null());

    // Prometheus は未観測の metric family を gather 結果へ含めないため、
    // export 契約を検証する前に1サンプル記録する。
    llmlb::cloud_metrics::record("contract", 200, 1);
    let response = send_request(&app, Method::GET, "/api/metrics/cloud", Some(&jwt), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/plain")));
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read metrics response");
    let body = String::from_utf8(body.to_vec()).expect("metrics body must be UTF-8");
    assert!(body.contains("cloud_requests_total"));

    let response = send_request(&app, Method::GET, "/api/models/hub", Some(&jwt), None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert!(body.is_array());
    assert!(body.as_array().is_some_and(Vec::is_empty));
}

#[tokio::test]
#[serial]
async fn websocket_contract_uses_dashboard_route_and_keeps_chat_route_removed() {
    // WebSocketUpgrade extractor は実サーバーが付与する OnUpgrade extension を必要とする。
    let server = crate::support::lb::spawn_test_lb().await;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build WebSocket contract client");

    let response = websocket_request(&client, server.addr(), "/ws/dashboard")
        .send()
        .await
        .expect("dashboard WebSocket response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = websocket_request(&client, server.addr(), "/ws/chat")
        .send()
        .await
        .expect("removed chat WebSocket response");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
