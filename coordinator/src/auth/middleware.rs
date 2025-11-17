// T047-T049: 認証ミドルウェア実装

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};

/// JWT認証ミドルウェア
///
/// Authorizationヘッダーから "Bearer {token}" を抽出してJWT検証を行う
///
/// # Arguments
/// * `State(jwt_secret)` - JWT署名検証用のシークレットキー
/// * `request` - HTTPリクエスト
/// * `next` - 次のミドルウェア/ハンドラー
///
/// # Returns
/// * `Ok(Response)` - 認証成功、requestにClaimsを追加
/// * `Err(Response)` - 認証失敗、401 Unauthorized
pub async fn jwt_auth_middleware(
    State(jwt_secret): State<String>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    // Authorizationヘッダーを取得
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Missing Authorization header".to_string(),
            )
                .into_response()
        })?;

    // "Bearer {token}" から token を抽出
    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Invalid Authorization header format".to_string(),
            )
                .into_response()
        })?;

    // JWTを検証
    let claims = crate::auth::jwt::verify_jwt(token, &jwt_secret).map_err(|e| {
        tracing::warn!("JWT verification failed: {}", e);
        (StatusCode::UNAUTHORIZED, format!("Invalid token: {}", e)).into_response()
    })?;

    // 検証済みのClaimsをrequestの拡張データに格納
    request.extensions_mut().insert(claims);

    // 次のミドルウェア/ハンドラーに進む
    Ok(next.run(request).await)
}

/// APIキー認証ミドルウェア
///
/// X-API-Keyヘッダーからキーを抽出してSHA-256で検証を行う
///
/// # Arguments
/// * `State(pool)` - データベース接続プール
/// * `request` - HTTPリクエスト
/// * `next` - 次のミドルウェア/ハンドラー
///
/// # Returns
/// * `Ok(Response)` - 認証成功
/// * `Err(Response)` - 認証失敗、401 Unauthorized
pub async fn api_key_auth_middleware(
    State(pool): State<sqlx::SqlitePool>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    // X-API-Keyヘッダーを取得
    let api_key = request
        .headers()
        .get("X-API-Key")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Missing X-API-Key header".to_string(),
            )
                .into_response()
        })?;

    // SHA-256ハッシュ化
    let key_hash = hash_with_sha256(api_key);

    // データベースでAPIキーを検証
    let api_key_record = crate::db::api_keys::find_by_hash(&pool, &key_hash)
        .await
        .map_err(|e| {
            tracing::warn!("API key verification failed: {}", e);
            (StatusCode::UNAUTHORIZED, "Invalid API key".to_string()).into_response()
        })?
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, "Invalid API key".to_string()).into_response()
        })?;

    // 有効期限チェック
    if let Some(expires_at) = api_key_record.expires_at {
        if expires_at < chrono::Utc::now() {
            return Err((StatusCode::UNAUTHORIZED, "API key expired".to_string()).into_response());
        }
    }

    // APIキーIDをrequestの拡張データに格納
    request.extensions_mut().insert(api_key_record.id);

    Ok(next.run(request).await)
}

/// エージェントトークン認証ミドルウェア
///
/// X-Agent-Tokenヘッダーからトークンを抽出してSHA-256で検証を行う
///
/// # Arguments
/// * `State(pool)` - データベース接続プール
/// * `request` - HTTPリクエスト
/// * `next` - 次のミドルウェア/ハンドラー
///
/// # Returns
/// * `Ok(Response)` - 認証成功
/// * `Err(Response)` - 認証失敗、401 Unauthorized
pub async fn agent_token_auth_middleware(
    State(pool): State<sqlx::SqlitePool>,
    mut request: Request,
    next: Next,
) -> Result<Response, Response> {
    // X-Agent-Tokenヘッダーを取得
    let agent_token = request
        .headers()
        .get("X-Agent-Token")
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Missing X-Agent-Token header".to_string(),
            )
                .into_response()
        })?;

    // SHA-256ハッシュ化
    let token_hash = hash_with_sha256(agent_token);

    // データベースでエージェントトークンを検証
    let agent_token_record = crate::db::agent_tokens::find_by_hash(&pool, &token_hash)
        .await
        .map_err(|e| {
            tracing::warn!("Agent token verification failed: {}", e);
            (StatusCode::UNAUTHORIZED, "Invalid agent token".to_string()).into_response()
        })?
        .ok_or_else(|| {
            (StatusCode::UNAUTHORIZED, "Invalid agent token".to_string()).into_response()
        })?;

    // エージェントIDをrequestの拡張データに格納
    request.extensions_mut().insert(agent_token_record.agent_id);

    Ok(next.run(request).await)
}

/// SHA-256ハッシュ化ヘルパー関数
///
/// # Arguments
/// * `input` - ハッシュ化する文字列
///
/// # Returns
/// * `String` - 16進数表現のSHA-256ハッシュ（64文字）
fn hash_with_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_with_sha256() {
        let input = "test_api_key_12345";
        let hash = hash_with_sha256(input);

        // SHA-256ハッシュは64文字の16進数
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        // 同じ入力は同じハッシュを生成
        let hash2 = hash_with_sha256(input);
        assert_eq!(hash, hash2);

        // 異なる入力は異なるハッシュを生成
        let hash3 = hash_with_sha256("different_input");
        assert_ne!(hash, hash3);
    }
}
