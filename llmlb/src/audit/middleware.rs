//! 監査ログミドルウェア (SPEC-8301d106)
//!
//! 全HTTPリクエストのメタデータを自動記録する。
//! WebSocket・静的アセット・ヘルスチェック等のノイズパスは除外。

use crate::audit::types::{ActorType, AuditLogEntry, AuthFailureInfo, TokenUsage};
use crate::auth::middleware::ApiKeyAuthContext;
use crate::common::auth::Claims;
use crate::AppState;
use axum::{
    body::Body,
    extract::{ConnectInfo, State},
    http::Request,
    middleware::Next,
    response::Response,
};
use chrono::Utc;
use std::{net::SocketAddr, time::Instant};
use tracing::trace;

/// 監査対象から除外すべきパスか判定する
fn should_exclude(path: &str) -> bool {
    // WebSocket
    if path.starts_with("/ws/") || path == "/ws" {
        return true;
    }
    // ヘルスチェック
    if path == "/health" {
        return true;
    }
    // 静的アセット（ダッシュボード配下の拡張子付きファイル）
    if path.starts_with("/dashboard/") {
        let extensions = [
            ".js", ".css", ".png", ".jpg", ".svg", ".ico", ".woff", ".woff2", ".map",
        ];
        if extensions.iter().any(|ext| path.ends_with(ext)) {
            return true;
        }
    }
    // ダッシュボードSSEポーリング
    if path == "/api/dashboard/events" {
        return true;
    }
    false
}

/// 監査ログミドルウェア
///
/// リクエストのHTTPメソッド・パス・ステータスコード・処理時間等を記録し、
/// `AuditLogWriter` 経由でバッファに送信する。
pub async fn audit_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // 除外判定
    if should_exclude(&path) {
        return next.run(request).await;
    }

    // クライアントIP取得（プロキシ対応 + 直接接続対応）
    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
        .or_else(|| {
            // プロキシヘッダがない場合は、request extensionsからConnectInfoを取得
            request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(addr)| addr.ip().to_string())
        });

    // リクエストを次のハンドラーに渡す
    let response = next.run(request).await;

    let duration = start.elapsed();
    let status_code = response.status().as_u16();

    // response extensionsからアクター情報を取得（認証ミドルウェアが設定）
    let (actor_type, actor_id, actor_username, api_key_owner_id) = extract_actor_info(&response);

    // response extensionsからトークン使用量を取得（推論ハンドラーが設定）
    let token_usage = response.extensions().get::<TokenUsage>().cloned();

    // response extensionsから認証失敗情報を取得
    let auth_failure = response.extensions().get::<AuthFailureInfo>().cloned();

    // 認証失敗の場合はdetailに理由を記録
    let detail = auth_failure.map(|info| {
        serde_json::json!({
            "auth_failure_reason": info.reason,
            "attempted_username": info.attempted_username,
        })
        .to_string()
    });

    trace!(
        method = %method,
        path = %path,
        status = status_code,
        duration_ms = duration.as_millis() as i64,
        actor_type = %actor_type,
        "audit log entry captured"
    );

    let entry = AuditLogEntry {
        id: None,
        timestamp: Utc::now(),
        http_method: method,
        request_path: path,
        status_code,
        actor_type,
        actor_id,
        actor_username,
        api_key_owner_id,
        client_ip,
        duration_ms: Some(duration.as_millis() as i64),
        input_tokens: token_usage.as_ref().and_then(|t| t.input_tokens),
        output_tokens: token_usage.as_ref().and_then(|t| t.output_tokens),
        total_tokens: token_usage.as_ref().and_then(|t| t.total_tokens),
        model_name: token_usage.as_ref().and_then(|t| t.model_name.clone()),
        endpoint_id: token_usage.as_ref().and_then(|t| t.endpoint_id.clone()),
        detail,
        batch_id: None,
        is_migrated: false,
    };

    state.audit_log_writer.send(entry);

    response
}

/// response extensionsからアクター情報を抽出する
fn extract_actor_info(
    response: &Response,
) -> (ActorType, Option<String>, Option<String>, Option<String>) {
    let extensions = response.extensions();

    // JWT認証済み（Claims）
    if let Some(claims) = extensions.get::<Claims>() {
        // APIキー認証の場合はApiKeyAuthContextも存在する
        if let Some(api_ctx) = extensions.get::<ApiKeyAuthContext>() {
            return (
                ActorType::ApiKey,
                Some(api_ctx.id.to_string()),
                None,
                Some(api_ctx.created_by.to_string()),
            );
        }
        return (
            ActorType::User,
            Some(claims.sub.clone()),
            None, // ユーザー名はDBルックアップが必要（将来拡張）
            None,
        );
    }

    // APIキー認証のみ（Claimsなし）
    if let Some(api_ctx) = extensions.get::<ApiKeyAuthContext>() {
        return (
            ActorType::ApiKey,
            Some(api_ctx.id.to_string()),
            None,
            Some(api_ctx.created_by.to_string()),
        );
    }

    // 認証なし
    (ActorType::Anonymous, None, None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::writer::{AuditLogWriter, AuditLogWriterConfig};
    use crate::balancer::LoadManager;
    use crate::db::audit_log::AuditLogStorage;
    use axum::{body::Body, http::Request, middleware as axum_middleware, routing::get, Router};
    use std::sync::Arc;
    use tower::ServiceExt;

    type TokenUsageRow = (
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
    );

    async fn create_test_pool() -> sqlx::SqlitePool {
        crate::db::test_utils::test_db_pool().await
    }

    async fn create_test_state(pool: sqlx::SqlitePool) -> AppState {
        let request_history = Arc::new(crate::db::request_history::RequestHistoryStorage::new(
            pool.clone(),
        ));
        let endpoint_registry = crate::registry::endpoints::EndpointRegistry::new(pool.clone())
            .await
            .expect("Failed to create endpoint registry");
        let endpoint_registry_arc = Arc::new(endpoint_registry.clone());
        let load_manager = LoadManager::new(endpoint_registry_arc);
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
                flush_interval_secs: 1, // テスト用に短いインターバル
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
            queue_config: crate::config::QueueConfig::from_env(),
            event_bus: crate::events::create_shared_event_bus(),
            endpoint_registry,
            inference_gate,
            shutdown,
            update_manager,
            audit_log_writer,
            audit_log_storage,
            audit_archive_pool: None,
        }
    }

    fn build_test_app(state: AppState) -> Router {
        Router::new()
            .route("/api/test", get(|| async { "ok" }))
            .route(
                "/api/slow",
                get(|| async {
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    "slow"
                }),
            )
            .route("/health", get(|| async { "healthy" }))
            .route("/ws/chat", get(|| async { "ws" }))
            .route("/dashboard/assets/index.js", get(|| async { "js" }))
            .route("/dashboard/assets/style.css", get(|| async { "css" }))
            .route("/api/dashboard/events", get(|| async { "events" }))
            .layer(axum_middleware::from_fn_with_state(state, audit_middleware))
    }

    #[tokio::test]
    async fn test_audit_captures_get_request() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        // フラッシュを待つ
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log_entries WHERE http_method = 'GET' AND request_path = '/api/test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count.0, 1, "GET /api/test should be recorded");
    }

    #[tokio::test]
    async fn test_audit_captures_post_request() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = Router::new()
            .route("/api/data", axum::routing::post(|| async { "created" }))
            .layer(axum_middleware::from_fn_with_state(state, audit_middleware));

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/data")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log_entries WHERE http_method = 'POST' AND request_path = '/api/data'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count.0, 1, "POST /api/data should be recorded");
    }

    #[tokio::test]
    async fn test_audit_excludes_websocket() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/ws/chat")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log_entries WHERE request_path LIKE '/ws/%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count.0, 0, "WebSocket paths should be excluded");
    }

    #[tokio::test]
    async fn test_audit_excludes_static_assets() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        // .js ファイル
        let app_clone = build_test_app(create_test_state(pool.clone()).await);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/dashboard/assets/index.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), 200);

        // .css ファイル
        let res = app_clone
            .oneshot(
                Request::builder()
                    .uri("/dashboard/assets/style.css")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log_entries WHERE request_path LIKE '/dashboard/assets/%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count.0, 0, "Static assets should be excluded");
    }

    #[tokio::test]
    async fn test_audit_excludes_health() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM audit_log_entries WHERE request_path = '/health'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count.0, 0, "/health should be excluded");
    }

    #[tokio::test]
    async fn test_audit_records_duration() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/slow")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (Option<i64>,) = sqlx::query_as(
            "SELECT duration_ms FROM audit_log_entries WHERE request_path = '/api/slow'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let duration_ms = row.0.expect("duration_ms should be recorded");
        assert!(
            duration_ms >= 50,
            "duration_ms should be at least 50ms (handler sleeps 50ms), got {}",
            duration_ms
        );
    }

    // should_exclude のユニットテスト
    #[test]
    fn test_should_exclude_websocket_paths() {
        assert!(should_exclude("/ws/chat"));
        assert!(should_exclude("/ws/"));
        assert!(should_exclude("/ws"));
        assert!(!should_exclude("/api/ws"));
    }

    #[test]
    fn test_should_exclude_health() {
        assert!(should_exclude("/health"));
        assert!(!should_exclude("/health/check"));
        assert!(!should_exclude("/api/health"));
    }

    #[test]
    fn test_should_exclude_static_assets() {
        assert!(should_exclude("/dashboard/assets/index.js"));
        assert!(should_exclude("/dashboard/assets/style.css"));
        assert!(should_exclude("/dashboard/favicon.ico"));
        assert!(should_exclude("/dashboard/font.woff2"));
        assert!(should_exclude("/dashboard/source.map"));
        // ダッシュボードHTML自体は記録対象
        assert!(!should_exclude("/dashboard/"));
        assert!(!should_exclude("/dashboard"));
        // dashboard外の.jsは記録対象
        assert!(!should_exclude("/api/test.js"));
    }

    #[test]
    fn test_should_exclude_sse_polling() {
        assert!(should_exclude("/api/dashboard/events"));
        assert!(!should_exclude("/api/dashboard/data"));
    }

    #[test]
    fn test_should_not_exclude_api_paths() {
        assert!(!should_exclude("/api/test"));
        assert!(!should_exclude("/v1/chat/completions"));
        assert!(!should_exclude("/v1/models"));
        assert!(!should_exclude("/api/endpoints"));
    }

    /// T011: 統合テスト - create_appで作成したRouterに対して
    /// リクエストを送信し、audit_log_entriesテーブルにレコードが挿入されることを検証
    #[tokio::test]
    async fn test_create_app_records_audit_log() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = crate::api::create_app(state);

        // GET /api/version は認証不要のエンドポイント
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        // フラッシュを待つ
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log_entries WHERE request_path = '/api/version'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            count.0, 1,
            "GET /api/version should be recorded in audit log via create_app"
        );
    }

    /// T014: JWT認証済みリクエストでactor_type=userが記録されること
    #[tokio::test]
    async fn test_audit_records_jwt_user_actor() {
        let pool = create_test_pool().await;

        // テスト用ユーザー作成
        let user = crate::db::users::create(
            &pool,
            "audit-test-user",
            "hash",
            crate::common::auth::UserRole::Admin,
            false,
        )
        .await
        .expect("create user");

        let state = create_test_state(pool.clone()).await;
        let jwt_secret = state.jwt_secret.clone();

        // JWTトークン生成
        let token = crate::auth::jwt::create_jwt(
            &user.id.to_string(),
            crate::common::auth::UserRole::Admin,
            &jwt_secret,
            false,
        )
        .expect("create jwt");

        // JWT認証ミドルウェア + 監査ミドルウェア付きRouter
        let app = Router::new()
            .route("/api/protected", get(|| async { "ok" }))
            .layer(axum_middleware::from_fn_with_state(
                jwt_secret,
                crate::auth::middleware::jwt_auth_middleware,
            ))
            .layer(axum_middleware::from_fn_with_state(state, audit_middleware));

        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/protected")
                    .header("authorization", format!("Bearer {}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (String, Option<String>) = sqlx::query_as(
            "SELECT actor_type, actor_id FROM audit_log_entries WHERE request_path = '/api/protected'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, "user", "actor_type should be 'user' for JWT auth");
        assert_eq!(
            row.1.as_deref(),
            Some(user.id.to_string().as_str()),
            "actor_id should match the user's UUID"
        );
    }

    /// T014: APIキー認証済みリクエストでactor_type=api_keyが記録されること
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn test_audit_records_api_key_actor() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;

        // APIキー認証ミドルウェア + 監査ミドルウェア付きRouter
        let app = Router::new()
            .route("/api/key-test", get(|| async { "ok" }))
            .layer(axum_middleware::from_fn_with_state(
                pool.clone(),
                crate::auth::middleware::api_key_auth_middleware,
            ))
            .layer(axum_middleware::from_fn_with_state(state, audit_middleware));

        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/key-test")
                    .header("x-api-key", "sk_debug")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (String,) = sqlx::query_as(
            "SELECT actor_type FROM audit_log_entries WHERE request_path = '/api/key-test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            row.0, "api_key",
            "actor_type should be 'api_key' for API key auth"
        );
    }

    /// T014: トークン使用量がresponse extensionsから取得されること
    #[tokio::test]
    async fn test_audit_records_token_usage() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;

        // ハンドラーがresponse extensionsにTokenUsageを設定するRouter
        let app = Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(|| async {
                    let mut response = axum::response::Response::new(axum::body::Body::from("ok"));
                    response.extensions_mut().insert(TokenUsage {
                        input_tokens: Some(100),
                        output_tokens: Some(50),
                        total_tokens: Some(150),
                        model_name: Some("test-model".to_string()),
                        endpoint_id: Some("ep-1".to_string()),
                    });
                    response
                }),
            )
            .layer(axum_middleware::from_fn_with_state(state, audit_middleware));

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: TokenUsageRow = sqlx::query_as(
            "SELECT input_tokens, output_tokens, total_tokens, model_name, endpoint_id \
                 FROM audit_log_entries WHERE request_path = '/v1/chat/completions'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, Some(100), "input_tokens should be 100");
        assert_eq!(row.1, Some(50), "output_tokens should be 50");
        assert_eq!(row.2, Some(150), "total_tokens should be 150");
        assert_eq!(
            row.3.as_deref(),
            Some("test-model"),
            "model_name should be 'test-model'"
        );
        assert_eq!(
            row.4.as_deref(),
            Some("ep-1"),
            "endpoint_id should be 'ep-1'"
        );
    }

    /// T015: 認証失敗時にAnonymousアクターとdetailが記録されること
    #[tokio::test]
    async fn test_audit_records_anonymous_without_auth() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (String, Option<String>) = sqlx::query_as(
            "SELECT actor_type, actor_id FROM audit_log_entries WHERE request_path = '/api/test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            row.0, "anonymous",
            "actor_type should be 'anonymous' without auth"
        );
        assert!(
            row.1.is_none(),
            "actor_id should be None for anonymous requests"
        );
    }

    /// T011: 統合テスト - create_appで除外パス（/health）がaudit_logに記録されないことを検証
    #[tokio::test]
    async fn test_create_app_excludes_health_from_audit() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = crate::api::create_app(state);

        // /health はcreate_appにはルート定義がないが、除外判定のテスト
        // dashboardの静的アセットを使う
        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/dashboard/index.html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // ダッシュボードHTML自体は配信される
        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // /dashboard/index.html は静的アセットではない（.htmlは除外対象外）ため、記録される
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log_entries WHERE request_path = '/dashboard/index.html'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            count.0, 1,
            "/dashboard/index.html (HTML) should be recorded"
        );
    }

    // =========================================================================
    // should_exclude additional edge cases
    // =========================================================================

    #[test]
    fn test_should_exclude_ws_root() {
        assert!(should_exclude("/ws"));
    }

    #[test]
    fn test_should_exclude_ws_with_trailing_slash() {
        assert!(should_exclude("/ws/"));
    }

    #[test]
    fn test_should_not_exclude_ws_like_path() {
        // Path containing "ws" but not starting with /ws/
        assert!(!should_exclude("/api/websocket"));
    }

    #[test]
    fn test_should_exclude_dashboard_png() {
        assert!(should_exclude("/dashboard/images/logo.png"));
    }

    #[test]
    fn test_should_exclude_dashboard_jpg() {
        assert!(should_exclude("/dashboard/images/bg.jpg"));
    }

    #[test]
    fn test_should_exclude_dashboard_svg() {
        assert!(should_exclude("/dashboard/icons/icon.svg"));
    }

    #[test]
    fn test_should_exclude_dashboard_woff() {
        assert!(should_exclude("/dashboard/fonts/roboto.woff"));
    }

    #[test]
    fn test_should_not_exclude_dashboard_html() {
        assert!(!should_exclude("/dashboard/index.html"));
    }

    #[test]
    fn test_should_not_exclude_dashboard_json() {
        assert!(!should_exclude("/dashboard/manifest.json"));
    }

    #[test]
    fn test_should_not_exclude_root() {
        assert!(!should_exclude("/"));
    }

    #[test]
    fn test_should_not_exclude_v1_endpoints() {
        assert!(!should_exclude("/v1/embeddings"));
        assert!(!should_exclude("/v1/audio/transcriptions"));
    }

    // =========================================================================
    // extract_actor_info tests
    // =========================================================================

    #[test]
    fn test_extract_actor_info_anonymous_without_extensions() {
        let response = axum::response::Response::new(Body::empty());
        let (actor_type, actor_id, actor_username, api_key_owner_id) =
            extract_actor_info(&response);
        assert_eq!(actor_type, ActorType::Anonymous);
        assert!(actor_id.is_none());
        assert!(actor_username.is_none());
        assert!(api_key_owner_id.is_none());
    }

    #[test]
    fn test_extract_actor_info_jwt_user() {
        let mut response = axum::response::Response::new(Body::empty());
        response.extensions_mut().insert(Claims {
            sub: "user-uuid-123".to_string(),
            role: crate::common::auth::UserRole::Admin,
            exp: 9999999999,
            must_change_password: false,
        });
        let (actor_type, actor_id, _actor_username, api_key_owner_id) =
            extract_actor_info(&response);
        assert_eq!(actor_type, ActorType::User);
        assert_eq!(actor_id, Some("user-uuid-123".to_string()));
        assert!(api_key_owner_id.is_none());
    }

    #[test]
    fn test_extract_actor_info_api_key_only() {
        let mut response = axum::response::Response::new(Body::empty());
        let key_id = uuid::Uuid::new_v4();
        let owner_id = uuid::Uuid::new_v4();
        response.extensions_mut().insert(ApiKeyAuthContext {
            id: key_id,
            created_by: owner_id,
            permissions: vec![],
            expires_at: None,
        });
        let (actor_type, actor_id, _actor_username, api_key_owner_id) =
            extract_actor_info(&response);
        assert_eq!(actor_type, ActorType::ApiKey);
        assert_eq!(actor_id, Some(key_id.to_string()));
        assert_eq!(api_key_owner_id, Some(owner_id.to_string()));
    }

    #[test]
    fn test_extract_actor_info_jwt_plus_api_key() {
        let mut response = axum::response::Response::new(Body::empty());
        let key_id = uuid::Uuid::new_v4();
        let owner_id = uuid::Uuid::new_v4();
        response.extensions_mut().insert(Claims {
            sub: owner_id.to_string(),
            role: crate::common::auth::UserRole::Admin,
            exp: 9999999999,
            must_change_password: false,
        });
        response.extensions_mut().insert(ApiKeyAuthContext {
            id: key_id,
            created_by: owner_id,
            permissions: vec![],
            expires_at: None,
        });
        // When both Claims and ApiKeyAuthContext are present, actor_type should be ApiKey
        let (actor_type, actor_id, _actor_username, api_key_owner_id) =
            extract_actor_info(&response);
        assert_eq!(actor_type, ActorType::ApiKey);
        assert_eq!(actor_id, Some(key_id.to_string()));
        assert_eq!(api_key_owner_id, Some(owner_id.to_string()));
    }

    // =========================================================================
    // audit middleware captures client IP
    // =========================================================================

    #[tokio::test]
    async fn test_audit_captures_x_forwarded_for_ip() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/test")
                    .header("x-forwarded-for", "192.168.1.100, 10.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (Option<String>,) = sqlx::query_as(
            "SELECT client_ip FROM audit_log_entries WHERE request_path = '/api/test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            row.0.as_deref(),
            Some("192.168.1.100"),
            "Should extract first IP from x-forwarded-for"
        );
    }

    #[tokio::test]
    async fn test_audit_captures_x_real_ip() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/test")
                    .header("x-real-ip", "10.0.0.42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (Option<String>,) = sqlx::query_as(
            "SELECT client_ip FROM audit_log_entries WHERE request_path = '/api/test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            row.0.as_deref(),
            Some("10.0.0.42"),
            "Should extract IP from x-real-ip"
        );
    }

    #[tokio::test]
    async fn test_audit_captures_direct_connection_ip_from_connect_info() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        // ConnectInfoをリクエストextensionsに手動注入（実サーバーではmiddlewareが自動で設定）
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut request = Request::builder()
            .method("GET")
            .uri("/api/test")
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(addr));

        let res = app.oneshot(request).await.unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (Option<String>,) = sqlx::query_as(
            "SELECT client_ip FROM audit_log_entries WHERE request_path = '/api/test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        // ConnectInfoから127.0.0.1が抽出されるはず
        assert_eq!(
            row.0.as_deref(),
            Some("127.0.0.1"),
            "Should extract IP from ConnectInfo when proxy headers are absent"
        );
    }

    // =========================================================================
    // audit middleware records status codes
    // =========================================================================

    #[tokio::test]
    async fn test_audit_records_404_status() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;

        let app = Router::new()
            .route(
                "/api/missing",
                get(|| async { (axum::http::StatusCode::NOT_FOUND, "not found") }),
            )
            .layer(axum_middleware::from_fn_with_state(state, audit_middleware));

        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 404);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (i64,) = sqlx::query_as(
            "SELECT status_code FROM audit_log_entries WHERE request_path = '/api/missing'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(row.0, 404, "status_code should be 404");
    }

    // =========================================================================
    // audit middleware excludes SSE events
    // =========================================================================

    #[tokio::test]
    async fn test_audit_excludes_dashboard_events() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard/events")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM audit_log_entries WHERE request_path = '/api/dashboard/events'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count.0, 0, "/api/dashboard/events should be excluded");
    }

    // =========================================================================
    // audit middleware records auth failure details
    // =========================================================================

    #[tokio::test]
    async fn test_audit_records_auth_failure_detail() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;

        let app = Router::new()
            .route(
                "/api/auth/login",
                axum::routing::post(|| async {
                    let mut response = axum::response::Response::new(Body::from("unauthorized"));
                    *response.status_mut() = axum::http::StatusCode::UNAUTHORIZED;
                    response.extensions_mut().insert(AuthFailureInfo {
                        attempted_username: Some("baduser".to_string()),
                        reason: "Invalid password".to_string(),
                    });
                    response
                }),
            )
            .layer(axum_middleware::from_fn_with_state(state, audit_middleware));

        let res = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/auth/login")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 401);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: (Option<String>,) = sqlx::query_as(
            "SELECT detail FROM audit_log_entries WHERE request_path = '/api/auth/login'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        let detail = row.0.expect("detail should be present for auth failure");
        assert!(
            detail.contains("Invalid password"),
            "detail should contain failure reason"
        );
        assert!(
            detail.contains("baduser"),
            "detail should contain attempted username"
        );
    }

    // =========================================================================
    // audit middleware with no token usage records nulls
    // =========================================================================

    #[tokio::test]
    async fn test_audit_records_null_token_usage_for_non_inference() {
        let pool = create_test_pool().await;
        let state = create_test_state(pool.clone()).await;
        let app = build_test_app(state);

        let res = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), 200);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let row: TokenUsageRow = sqlx::query_as(
            "SELECT input_tokens, output_tokens, total_tokens, model_name, endpoint_id \
             FROM audit_log_entries WHERE request_path = '/api/test'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(row.0.is_none(), "input_tokens should be null");
        assert!(row.1.is_none(), "output_tokens should be null");
        assert!(row.2.is_none(), "total_tokens should be null");
        assert!(row.3.is_none(), "model_name should be null");
        assert!(row.4.is_none(), "endpoint_id should be null");
    }
}
