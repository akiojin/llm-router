//! Clients Heatmap API の Contract Tests
//!
//! T035: ヒートマップAPI統合テスト
//! SPEC-62ac4b68: 時間帯×曜日ヒートマップ

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

/// T035: GET /api/dashboard/clients/heatmap - データなしで空配列
#[tokio::test]
#[serial]
async fn test_clients_heatmap_api_empty() {
    let (app, _db_pool, jwt) = build_app().await;

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/heatmap")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    assert!(body.is_array(), "heatmap should be an array");
    let cells = body.as_array().unwrap();

    // 全セルがcount=0
    for cell in cells {
        assert!(cell["day_of_week"].is_number());
        assert!(cell["hour"].is_number());
        assert_eq!(cell["count"], 0);
    }
}

/// T035: GET /api/dashboard/clients/heatmap - データありのセル
#[tokio::test]
#[serial]
async fn test_clients_heatmap_api_with_data() {
    let (app, db_pool, jwt) = build_app().await;
    let now = Utc::now();
    let endpoint_id = Uuid::new_v4();

    // 直近のリクエストを3件追加
    for i in 0..3 {
        let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        let record =
            create_test_record("model-a", endpoint_id, now - Duration::minutes(i), Some(ip));
        insert_record(&db_pool, &record).await;
    }

    let response = app
        .oneshot(
            admin_request(&jwt)
                .method("GET")
                .uri("/api/dashboard/clients/heatmap")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();

    let cells = body.as_array().unwrap();

    // 各セルの構造検証
    for cell in cells {
        let dow = cell["day_of_week"].as_i64().unwrap();
        let hour = cell["hour"].as_i64().unwrap();
        assert!((0..=6).contains(&dow), "day_of_week should be 0-6");
        assert!((0..=23).contains(&hour), "hour should be 0-23");
        assert!(cell["count"].is_number());
    }

    // 少なくとも1つのセルがcount > 0
    let has_data = cells.iter().any(|c| c["count"].as_i64().unwrap_or(0) > 0);
    assert!(has_data, "should have at least one cell with count > 0");
}

/// T035: 認証なしでは401
#[tokio::test]
#[serial]
async fn test_clients_heatmap_requires_auth() {
    let (app, _db_pool, _jwt) = build_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/dashboard/clients/heatmap")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
