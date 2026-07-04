//! Clients API Keys API の Contract Tests
//!
//! T047: APIキー別集計テスト
//! SPEC-62ac4b68: APIキーとのクロス分析

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

fn create_test_record_with_key(
    model: &str,
    endpoint_id: Uuid,
    timestamp: chrono::DateTime<Utc>,
    client_ip: Option<std::net::IpAddr>,
    api_key_id: Option<Uuid>,
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
        api_key_id,
    }
}

/// T047: GET /api/dashboard/clients/{ip}/api-keys - APIキー別集計
#[tokio::test]
#[serial]
async fn test_clients_apikeys_api() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();
    let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();

    let key_a = Uuid::new_v4();
    let key_b = Uuid::new_v4();

    // key_a: 3リクエスト
    for i in 0..3 {
        let record = create_test_record_with_key(
            "model-a",
            endpoint_id,
            now - Duration::minutes(i),
            Some(ip),
            Some(key_a),
        );
        insert_record(&db_pool, &record).await;
    }

    // key_b: 2リクエスト
    for i in 0..2 {
        let record = create_test_record_with_key(
            "model-b",
            endpoint_id,
            now - Duration::minutes(10 + i),
            Some(ip),
            Some(key_b),
        );
        insert_record(&db_pool, &record).await;
    }

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/10.0.0.1/api-keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body.is_array());
    let keys = body.as_array().unwrap();
    assert_eq!(keys.len(), 2);

    // リクエスト数降順
    assert_eq!(keys[0]["request_count"], 3);
    assert!(keys[0]["api_key_id"].is_string());

    assert_eq!(keys[1]["request_count"], 2);
}

/// T047: GET /api/dashboard/clients/{ip}/api-keys - IPv6 /64指定
#[tokio::test]
#[serial]
async fn test_clients_apikeys_api_ipv6_prefix64() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    let key_a = Uuid::new_v4();
    let key_b = Uuid::new_v4();

    // 同一/64: key_a 2件, key_b 1件
    for ip_str in ["2001:db8:1:0:abcd::1", "2001:db8:1:0:beef::2"] {
        let ip: std::net::IpAddr = ip_str.parse().unwrap();
        let record = create_test_record_with_key(
            "model-a",
            endpoint_id,
            now - Duration::minutes(1),
            Some(ip),
            Some(key_a),
        );
        insert_record(&db_pool, &record).await;
    }
    let ip_same_prefix: std::net::IpAddr = "2001:db8:1:0:cafe::3".parse().unwrap();
    let record = create_test_record_with_key(
        "model-a",
        endpoint_id,
        now - Duration::minutes(2),
        Some(ip_same_prefix),
        Some(key_b),
    );
    insert_record(&db_pool, &record).await;

    // 別/64: key_a 1件（集計に含まれない）
    let ip_other_prefix: std::net::IpAddr = "2001:db8:1:1::1".parse().unwrap();
    let record = create_test_record_with_key(
        "model-a",
        endpoint_id,
        now - Duration::minutes(3),
        Some(ip_other_prefix),
        Some(key_a),
    );
    insert_record(&db_pool, &record).await;

    let encoded_prefix = "2001:db8:1::/64".replace("/", "%2F");
    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri(format!("/api/dashboard/clients/{encoded_prefix}/api-keys"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let keys = body.as_array().unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0]["request_count"], 2);
    assert_eq!(keys[1]["request_count"], 1);
}

/// T047: GET /api/dashboard/clients/{ip}/api-keys - データなしで空配列
#[tokio::test]
#[serial]
async fn test_clients_apikeys_api_empty() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/192.168.99.99/api-keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    let keys = body.as_array().unwrap();
    assert_eq!(keys.len(), 0);
}

/// T047: 認証なしでは401
#[tokio::test]
#[serial]
async fn test_clients_apikeys_requires_auth() {
    let (app, _db_pool, _jwt) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/clients/10.0.0.1/api-keys")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
