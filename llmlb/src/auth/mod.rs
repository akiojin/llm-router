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
