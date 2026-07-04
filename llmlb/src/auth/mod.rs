// 認証モジュール

/// パスワードハッシュ化・検証（bcrypt）
pub mod password;

/// JWT生成・検証（jsonwebtoken）
pub mod jwt;

/// 認証ミドルウェア（JWT, APIキー, ノードトークン）
pub mod middleware;

/// 初回起動時の管理者アカウント作成
pub mod bootstrap;

/// ダッシュボードJWT Cookie名
pub const DASHBOARD_JWT_COOKIE: &str = "llmlb_jwt";
/// ダッシュボードCSRF Cookie名
pub const DASHBOARD_CSRF_COOKIE: &str = "llmlb_csrf";

/// JWT Cookieヘッダーを生成
pub fn build_jwt_cookie(token: &str, max_age_secs: usize, secure: bool) -> String {
    let mut cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        DASHBOARD_JWT_COOKIE, token, max_age_secs
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// CSRF Cookieヘッダーを生成（フロントで読み取るためHttpOnlyは付与しない）
pub fn build_csrf_cookie(token: &str, max_age_secs: usize, secure: bool) -> String {
    let mut cookie = format!(
        "{}={}; Path=/; SameSite=Lax; Max-Age={}",
        DASHBOARD_CSRF_COOKIE, token, max_age_secs
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// JWT Cookieを削除するためのヘッダーを生成
pub fn clear_jwt_cookie(secure: bool) -> String {
    let mut cookie = format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        DASHBOARD_JWT_COOKIE
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// CSRF Cookieを削除するためのヘッダーを生成
pub fn clear_csrf_cookie(secure: bool) -> String {
    let mut cookie = format!(
        "{}=; Path=/; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        DASHBOARD_CSRF_COOKIE
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// ランダムトークン生成
pub fn generate_random_token(length: usize) -> String {
    use rand::RngExt;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..length)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// リクエストが HTTPS 経由かを判定する（リバースプロキシの x-forwarded-proto /
/// forwarded ヘッダを考慮）。Secure cookie 属性の付与要否判定に使う。
/// api/auth.rs と auth/middleware.rs で共有する唯一の実装。
pub(crate) fn request_is_secure(headers: &axum::http::HeaderMap) -> bool {
    if let Some(proto) = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
    {
        if proto.eq_ignore_ascii_case("https") {
            return true;
        }
    }
    if let Some(forwarded) = headers
        .get("forwarded")
        .and_then(|value| value.to_str().ok())
    {
        let lowered = forwarded.to_ascii_lowercase();
        if lowered.contains("proto=https") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod request_is_secure_tests {
    use super::request_is_secure;
    use axum::http::HeaderMap;

    #[test]
    fn returns_false_for_empty_headers() {
        assert!(!request_is_secure(&HeaderMap::new()));
    }

    #[test]
    fn returns_true_for_x_forwarded_proto_https() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        assert!(request_is_secure(&headers));
    }

    #[test]
    fn returns_false_for_x_forwarded_proto_http() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "http".parse().unwrap());
        assert!(!request_is_secure(&headers));
    }

    #[test]
    fn case_insensitive_https() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "HTTPS".parse().unwrap());
        assert!(request_is_secure(&headers));
    }

    #[test]
    fn forwarded_header_proto_https() {
        let mut headers = HeaderMap::new();
        headers.insert("forwarded", "proto=https".parse().unwrap());
        assert!(request_is_secure(&headers));
    }

    #[test]
    fn forwarded_header_proto_http() {
        let mut headers = HeaderMap::new();
        headers.insert("forwarded", "proto=http".parse().unwrap());
        assert!(!request_is_secure(&headers));
    }

    #[test]
    fn forwarded_complex_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "forwarded",
            "for=192.0.2.60;proto=https;by=203.0.113.43"
                .parse()
                .unwrap(),
        );
        assert!(request_is_secure(&headers));
    }
}
