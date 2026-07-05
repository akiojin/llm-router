//! 監査ログミドルウェア (SPEC-8301d106)
//!
//! 全HTTPリクエストのメタデータを自動記録する。
//! WebSocket・静的アセット・ヘルスチェック等のノイズパスは除外。

#[cfg(test)]
use crate::audit::types::ActorType;
use crate::audit::types::{AuditLogEntry, AuthFailureInfo, TokenUsage};
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

mod actor;
use actor::{extract_actor_info, extract_actor_info_from};
mod exclusion;
use exclusion::should_exclude;

/// 監査ログミドルウェア
///
/// リクエストのHTTPメソッド・パス・ステータスコード・処理時間等を記録し、
/// `AuditLogWriter` 経由でバッファに送信する。
pub async fn audit_middleware(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // 除外判定
    if should_exclude(&path) {
        return next.run(request).await;
    }

    // arch-review [M10]: 認証ミドルウェアが解決したアクターをロバストに受け取るため、
    // request にアクタースロットを挿入する。認証ミドルウェアが record() で書き込む。
    let actor_slot = AuditActorSlot::default();
    request.extensions_mut().insert(actor_slot.clone());

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

    // アクター情報: スロット(primary)→response extensions(fallback) の順で取得する。
    // arch-review [M10]: response 差し替えでアクター帰属が失われる脆さを排除。
    let actor_snapshot = actor_slot.snapshot();
    let (actor_type, actor_id, actor_username, api_key_owner_id) =
        if actor_snapshot.claims.is_some() || actor_snapshot.api_key.is_some() {
            extract_actor_info_from(
                actor_snapshot.claims.as_ref(),
                actor_snapshot.api_key.as_ref(),
            )
        } else {
            extract_actor_info(&response)
        };

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

/// 認証ミドルウェアが解決したアクターを監査へロバストに伝搬するためのスロット。
///
/// arch-review [M10]: 従来はアクター情報を response extensions へ再スタンプして
/// 監査ミドルウェアが読み取っていたが、ハンドラが response を差し替えると帰属が
/// 失われる脆さがあった。監査ミドルウェアが request に本スロットを挿入し、認証
/// ミドルウェアが [`AuditActorSlot::record`] で解決結果を書き込む。監査は next 実行後に
/// スロット(primary)→response extensions(fallback) の順で読むため、response 差し替えに
/// 影響されない。fallback を残すため、スロット未書込の経路でも従来挙動に退化するのみ。
#[derive(Clone, Default)]
pub struct AuditActorSlot(std::sync::Arc<std::sync::Mutex<AuditActorSnapshot>>);

#[derive(Clone, Default)]
struct AuditActorSnapshot {
    claims: Option<Claims>,
    api_key: Option<ApiKeyAuthContext>,
}

impl AuditActorSlot {
    /// 認証ミドルウェアが解決したアクター（Claims / ApiKeyAuthContext）を記録する。
    pub fn record(&self, claims: Option<Claims>, api_key: Option<ApiKeyAuthContext>) {
        if let Ok(mut snap) = self.0.lock() {
            if claims.is_some() {
                snap.claims = claims;
            }
            if api_key.is_some() {
                snap.api_key = api_key;
            }
        }
    }

    fn snapshot(&self) -> AuditActorSnapshot {
        self.0.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
