//! Clients Drilldown API の Contract Tests
//!
//! T041: IPドリルダウンAPI統合テスト
//! SPEC-62ac4b68: IPドリルダウン詳細ビュー

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use chrono::{Duration, Utc};
use llmlb::common::auth::UserRole;
use llmlb::common::protocol::{RecordStatus, RequestResponseRecord, RequestType};
use llmlb::db::request_history::RequestHistoryStorage;
use serde_json::{json, Value};
use serial_test::serial;
use sqlx::SqlitePool;
use tower::ServiceExt;
use uuid::Uuid;

async fn build_app() -> (Router, SqlitePool, String) {
    let (app, db_pool) = crate::support::lb::create_test_lb().await;

    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let admin_user =
        llmlb::db::users::create(&db_pool, "admin", &password_hash, UserRole::Admin, false)
            .await
            .expect("create admin user");
    let jwt = llmlb::auth::jwt::create_jwt(
        &admin_user.id.to_string(),
        UserRole::Admin,
        &crate::support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create admin jwt");

    (app, db_pool, jwt)
}

fn admin_request(jwt: &str) -> axum::http::request::Builder {
    Request::builder().header("authorization", format!("Bearer {}", jwt))
}

async fn insert_record(db_pool: &SqlitePool, record: &RequestResponseRecord) {
    let storage = RequestHistoryStorage::new(db_pool.clone());
    storage.save_record(record).await.unwrap();
}

fn create_test_record(
    model: &str,
    endpoint_id: Uuid,
    timestamp: chrono::DateTime<Utc>,
    client_ip: Option<std::net::IpAddr>,
) -> RequestResponseRecord {
    RequestResponseRecord {
        id: Uuid::new_v4(),
        timestamp,
        request_type: RequestType::Chat,
        model: model.to_string(),
        endpoint_id,
        endpoint_name: "test-node".to_string(),
        endpoint_ip: "127.0.0.1".parse().unwrap(),
        client_ip,
        request_body: json!({"model": model, "messages": [{"role": "user", "content": "hello"}]}),
        response_body: Some(
            json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}),
        ),
        duration_ms: 42,
        status: RecordStatus::Success,
        completed_at: timestamp + Duration::seconds(1),
        input_tokens: None,
        output_tokens: None,
        total_tokens: None,
        api_key_id: None,
    }
}

/// T041: GET /api/dashboard/clients/{ip}/detail - 基本
#[tokio::test]
#[serial]
async fn test_clients_drilldown_api() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
    // model-a: 3回, model-b: 2回
    for i in 0..3 {
        let record =
            create_test_record("model-a", endpoint_id, now - Duration::minutes(i), Some(ip));
        insert_record(&db_pool, &record).await;
    }
    for i in 0..2 {
        let record = create_test_record(
            "model-b",
            endpoint_id,
            now - Duration::minutes(10 + i),
            Some(ip),
        );
        insert_record(&db_pool, &record).await;
    }

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/10.0.0.1/detail")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    // 合計リクエスト数
    assert_eq!(body["total_requests"], 5);

    // 直近リクエスト
    assert!(body["recent_requests"].is_array());
    let recent = body["recent_requests"].as_array().unwrap();
    assert!(recent.len() <= 20);
    assert!(!recent.is_empty());

    // モデル分布
    assert!(body["model_distribution"].is_array());
    let models = body["model_distribution"].as_array().unwrap();
    assert_eq!(models.len(), 2);

    // 時間帯パターン
    assert!(body["hourly_pattern"].is_array());
}

/// T041: GET /api/dashboard/clients/{ip}/detail - IPv6 /64指定
#[tokio::test]
#[serial]
async fn test_clients_drilldown_api_ipv6_prefix64() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    let same_prefix_ips = ["2001:db8:1:0:abcd::1", "2001:db8:1:0:beef::2"];
    for ip_str in same_prefix_ips {
        let ip: std::net::IpAddr = ip_str.parse().unwrap();
        let record =
            create_test_record("model-a", endpoint_id, now - Duration::minutes(1), Some(ip));
        insert_record(&db_pool, &record).await;
    }

    let other_prefix_ip: std::net::IpAddr = "2001:db8:1:1::1".parse().unwrap();
    let record = create_test_record(
        "model-b",
        endpoint_id,
        now - Duration::minutes(2),
        Some(other_prefix_ip),
    );
    insert_record(&db_pool, &record).await;

    let encoded_prefix = "2001:db8:1::/64".replace("/", "%2F");
    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri(format!("/api/dashboard/clients/{encoded_prefix}/detail"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["total_requests"], 2);
}

/// T041: GET /api/dashboard/clients/{ip}/detail - 存在しないIP
#[tokio::test]
#[serial]
async fn test_clients_drilldown_api_not_found() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/192.168.99.99/detail")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(body["total_requests"], 0);
    assert_eq!(body["recent_requests"].as_array().unwrap().len(), 0);
}

/// T041: 認証なしでは401
#[tokio::test]
#[serial]
async fn test_clients_drilldown_requires_auth() {
    let (app, _db_pool, _jwt) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/clients/10.0.0.1/detail")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
