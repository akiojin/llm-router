//! Integration Test: Dashboard endpoints response includes endpoint type
//!
//! Dashboard UI expects `endpoint_type` from
//! `GET /api/dashboard/endpoints` (Type column + detail modal).

use llmlb::common::auth::UserRole;
use reqwest::Client;
use serde_json::{json, Value};
use sqlx::SqlitePool;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

use crate::support::lb::spawn_test_lb_with_db;

async fn create_admin_jwt(db_pool: &SqlitePool) -> String {
    let password_hash = llmlb::auth::password::hash_password("password123").unwrap();
    let created =
        llmlb::db::users::create(db_pool, "admin", &password_hash, UserRole::Admin, false).await;
    let admin_id = match created {
        Ok(user) => user.id,
        Err(_) => {
            llmlb::db::users::find_by_username(db_pool, "admin")
                .await
                .unwrap()
                .unwrap()
                .id
        }
    };

    llmlb::auth::jwt::create_jwt(
        &admin_id.to_string(),
        UserRole::Admin,
        &crate::support::lb::test_jwt_secret(),
        false,
        0,
        None,
    )
    .expect("create admin jwt")
}

#[tokio::test]
async fn test_dashboard_endpoints_includes_endpoint_type() {
    let (server, db_pool) = spawn_test_lb_with_db().await;
    let client = Client::new();
    let jwt = create_admin_jwt(&db_pool).await;

    // Register endpoint with mock server for auto-detection
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "xllm_version": "0.1.0"
        })))
        .mount(&mock)
        .await;

    let create_resp = client
        .post(format!("http://{}/api/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", jwt))
        .json(&json!({
            "name": "Type for Dashboard",
            "base_url": mock.uri()
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(create_resp.status().as_u16(), 201);
    let created: Value = create_resp.json().await.unwrap();
    let endpoint_id = created["id"].as_str().unwrap();

    let dash_resp = client
        .get(format!("http://{}/api/dashboard/endpoints", server.addr()))
        .header("authorization", format!("Bearer {}", jwt))
        .send()
        .await
        .unwrap();

    assert_eq!(dash_resp.status().as_u16(), 200);
    let endpoints: Vec<Value> = dash_resp.json().await.unwrap();
    let endpoint = endpoints
        .into_iter()
        .find(|ep| ep["id"].as_str() == Some(endpoint_id))
        .expect("Endpoint should exist in dashboard list");

    assert_eq!(endpoint["endpoint_type"], "xllm");
}
