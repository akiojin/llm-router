//! Clients Timeline & Model Distribution API の Contract Tests
//!
//! T028: 時系列分析・モデル分布API統合テスト
//! SPEC-62ac4b68: 使用パターンの時系列分析

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

/// T028: GET /api/dashboard/clients/timeline - 1時間刻みのユニークIP数
#[tokio::test]
#[serial]
async fn test_clients_timeline_api() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    // 1時間前: IP-A, IP-B (2ユニーク)
    for ip_str in ["10.0.0.1", "10.0.0.2"] {
        let ip: std::net::IpAddr = ip_str.parse().unwrap();
        let record = create_test_record("model-a", endpoint_id, now - Duration::hours(1), Some(ip));
        insert_record(&db_pool, &record).await;
    }

    // 2時間前: IP-A, IP-A, IP-C (2ユニーク)
    for ip_str in ["10.0.0.1", "10.0.0.1", "10.0.0.3"] {
        let ip: std::net::IpAddr = ip_str.parse().unwrap();
        let record = create_test_record("model-a", endpoint_id, now - Duration::hours(2), Some(ip));
        insert_record(&db_pool, &record).await;
    }

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/timeline")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    // 配列でタイムライン返却
    assert!(body.is_array(), "timeline should be an array");
    let timeline = body.as_array().unwrap();

    // 24時間分のデータポイント
    assert_eq!(timeline.len(), 24, "should have 24 hourly data points");

    // 各ポイントにhourとunique_ipsフィールドがある
    assert!(timeline[0]["hour"].is_string());
    assert!(timeline[0]["unique_ips"].is_number());
}

/// T028: GET /api/dashboard/clients/timeline - データなしで24ポイント（全てゼロ）
#[tokio::test]
#[serial]
async fn test_clients_timeline_api_empty() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/timeline")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    let timeline = body.as_array().unwrap();
    assert_eq!(timeline.len(), 24);

    // 全てゼロ
    for point in timeline {
        assert_eq!(point["unique_ips"], 0);
    }
}

/// T028: GET /api/dashboard/clients/models - モデル分布
#[tokio::test]
#[serial]
async fn test_clients_models_api() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    // model-a: 3リクエスト
    for i in 0..3 {
        let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        let record =
            create_test_record("model-a", endpoint_id, now - Duration::minutes(i), Some(ip));
        insert_record(&db_pool, &record).await;
    }

    // model-b: 2リクエスト
    for i in 0..2 {
        let ip: std::net::IpAddr = "10.0.0.2".parse().unwrap();
        let record =
            create_test_record("model-b", endpoint_id, now - Duration::minutes(i), Some(ip));
        insert_record(&db_pool, &record).await;
    }

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body.is_array(), "models should be an array");
    let models = body.as_array().unwrap();
    assert_eq!(models.len(), 2);

    // リクエスト数降順
    assert_eq!(models[0]["model"], "model-a");
    assert_eq!(models[0]["request_count"], 3);
    assert!(models[0]["percentage"].is_number());

    assert_eq!(models[1]["model"], "model-b");
    assert_eq!(models[1]["request_count"], 2);
}

/// T028: GET /api/dashboard/clients/models - データなしで空配列
#[tokio::test]
#[serial]
async fn test_clients_models_api_empty() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    let models = body.as_array().unwrap();
    assert_eq!(models.len(), 0);
}

/// T028: GET /api/dashboard/clients/timeline - IPv6は/64でユニーク集計
#[tokio::test]
#[serial]
async fn test_clients_timeline_api_ipv6_prefix64_grouping() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();
    let target_hour = (now - Duration::hours(1))
        .format("%Y-%m-%dT%H:00:00")
        .to_string();

    // 同一/64
    for ip_str in ["2001:db8:1:0:abcd::1", "2001:db8:1:0:beef::2"] {
        let ip: std::net::IpAddr = ip_str.parse().unwrap();
        let record = create_test_record("model-a", endpoint_id, now - Duration::hours(1), Some(ip));
        insert_record(&db_pool, &record).await;
    }
    // 別/64
    let ip_other: std::net::IpAddr = "2001:db8:1:1::1".parse().unwrap();
    let record = create_test_record(
        "model-a",
        endpoint_id,
        now - Duration::hours(1),
        Some(ip_other),
    );
    insert_record(&db_pool, &record).await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/timeline")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    let timeline = body.as_array().unwrap();
    let point = timeline
        .iter()
        .find(|p| p["hour"].as_str() == Some(target_hour.as_str()))
        .expect("target hour should exist");

    // 同一/64の2件は1クライアントとして扱われるため、ユニーク数は2
    assert_eq!(point["unique_ips"], 2);
}

/// T028: 認証なしでは401
#[tokio::test]
#[serial]
async fn test_clients_timeline_requires_auth() {
    let (app, _db_pool, _jwt) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/clients/timeline")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
