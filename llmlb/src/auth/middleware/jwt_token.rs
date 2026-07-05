//! JWT トークンのヘッダ/Cookie 抽出と検証
//!
//! arch-review [H6] round2: auth/middleware.rs から JWT 抽出/検証ヘルパーを分離。

use crate::common::auth::Claims;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use jsonwebtoken::decode_header;

pub(crate) fn token_looks_like_jwt(token: &str) -> bool {
    let mut parts = token.split('.');
    let (first, second, third, extra) = (parts.next(), parts.next(), parts.next(), parts.next());
    if extra.is_some() {
        return false;
    }
    if matches!((first, second, third), (Some(a), Some(b), Some(c)) if !a.is_empty() && !b.is_empty() && !c.is_empty())
    {
        return decode_header(token).is_ok();
    }
    false
}

pub(crate) fn extract_jwt_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(auth_header) = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Some(token) = auth_header.strip_prefix("Bearer ") {
            if token_looks_like_jwt(token) {
                return Some(token.to_string());
            }
        }
    }
    extract_jwt_cookie(headers)
}

#[allow(clippy::result_large_err)]
pub(crate) fn verify_jwt_claims(token: &str, jwt_secret: &str) -> Result<Claims, Response> {
    crate::auth::jwt::verify_jwt(token, jwt_secret).map_err(|e| {
        tracing::warn!("JWT verification failed: {}", e);
        (StatusCode::UNAUTHORIZED, format!("Invalid token: {}", e)).into_response()
    })
}

pub(crate) fn extract_jwt_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some(value) =
            trimmed.strip_prefix(&format!("{}=", crate::auth::DASHBOARD_JWT_COOKIE))
        {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}
