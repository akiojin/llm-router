//! CSRF トークン Cookie 処理と same-origin（Origin/Referer vs Host）検証
//!
//! arch-review [H6]: auth/middleware.rs から CSRF/同一オリジン検証の
//! ヘッダ/レスポンスヘルパーを分離。csrf_protect_middleware から参照される。

use axum::http::{header, HeaderMap};
use axum::response::Response;
use std::str::FromStr;

pub(crate) fn extract_csrf_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_header = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie_header.split(';') {
        let trimmed = part.trim();
        if let Some(value) =
            trimmed.strip_prefix(&format!("{}=", crate::auth::DASHBOARD_CSRF_COOKIE))
        {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

pub(crate) fn method_requires_csrf(method: &axum::http::Method) -> bool {
    matches!(
        *method,
        axum::http::Method::POST
            | axum::http::Method::PUT
            | axum::http::Method::PATCH
            | axum::http::Method::DELETE
    )
}

pub(crate) fn expected_origin(headers: &HeaderMap) -> Option<String> {
    let host_raw = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|value| value.to_str().ok())?;
    let host = host_raw
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let proto_raw = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let proto = proto_raw
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("http");
    Some(format!("{}://{}", proto, host))
}

pub(crate) fn origin_or_referer(headers: &HeaderMap) -> Option<String> {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        return Some(origin.to_string());
    }
    let referer = headers
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())?;
    if let Some((scheme, rest)) = referer.split_once("://") {
        let host = rest.split('/').next().unwrap_or_default();
        if !host.is_empty() {
            return Some(format!("{}://{}", scheme, host));
        }
    }
    None
}

pub(crate) fn origin_matches(headers: &HeaderMap) -> bool {
    let expected = match expected_origin(headers) {
        Some(value) => value,
        None => return false,
    };
    let provided = match origin_or_referer(headers) {
        Some(value) => value,
        None => return false,
    };
    match (
        normalize_origin_for_compare(&provided),
        normalize_origin_for_compare(&expected),
    ) {
        (Some(provided), Some(expected)) => provided == expected,
        _ => false,
    }
}

pub(crate) fn normalize_origin_for_compare(origin: &str) -> Option<(String, String, u16)> {
    let (scheme, rest) = origin.split_once("://")?;
    let authority = rest.split('/').next()?.trim();
    if authority.is_empty() {
        return None;
    }
    let authority = axum::http::uri::Authority::from_str(authority).ok()?;
    let scheme = scheme.trim().to_ascii_lowercase();
    let host = authority.host().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let port = authority
        .port_u16()
        .or_else(|| default_port_for_scheme(&scheme))?;
    Some((scheme, host, port))
}

pub(crate) fn default_port_for_scheme(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

pub(crate) fn response_sets_csrf_cookie(response: &Response) -> bool {
    response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| value.starts_with(crate::auth::DASHBOARD_CSRF_COOKIE))
}
