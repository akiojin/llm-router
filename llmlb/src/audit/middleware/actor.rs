//! 認証結果（Claims / ApiKeyAuthContext）から監査アクター情報を抽出
//!
//! arch-review [H6] round2: audit/middleware.rs からアクター抽出ロジックを分離。

use crate::audit::types::ActorType;
use crate::auth::middleware::ApiKeyAuthContext;
use crate::common::auth::Claims;
use axum::response::Response;

/// Claims / ApiKeyAuthContext からアクター情報を抽出する（ソース非依存）。
pub(super) fn extract_actor_info_from(
    claims: Option<&Claims>,
    api_ctx: Option<&ApiKeyAuthContext>,
) -> (ActorType, Option<String>, Option<String>, Option<String>) {
    // JWT認証済み（Claims）
    if let Some(claims) = claims {
        // APIキー認証の場合はApiKeyAuthContextも存在する
        if let Some(api_ctx) = api_ctx {
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
            // arch-review [L8]: JWT に載せたユーザー名を actor_username として使用する。
            claims.username.clone(),
            None,
        );
    }

    // APIキー認証のみ（Claimsなし）
    if let Some(api_ctx) = api_ctx {
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

/// response extensions からアクター情報を抽出する（fallback / テスト互換）。
pub(super) fn extract_actor_info(
    response: &Response,
) -> (ActorType, Option<String>, Option<String>, Option<String>) {
    let extensions = response.extensions();
    extract_actor_info_from(
        extensions.get::<Claims>(),
        extensions.get::<ApiKeyAuthContext>(),
    )
}
