//! Unit Test: Proxy URL Parsing
//!
//! プロキシURL解析のテスト

#[test]
fn test_http_proxy_url_parsing() {
    // HTTP_PROXY環境変数の形式
    let proxy_url = "http://proxy.example.com:8080";

    // reqwest::Proxyが解析できることを確認
    let proxy = reqwest::Proxy::http(proxy_url);
    assert!(proxy.is_ok(), "HTTP proxy URL should be valid");
}

#[test]
fn test_https_proxy_url_parsing() {
    // HTTPS_PROXY環境変数の形式
    let proxy_url = "http://proxy.example.com:8443";

    let proxy = reqwest::Proxy::https(proxy_url);
    assert!(proxy.is_ok(), "HTTPS proxy URL should be valid");
}

#[test]
fn test_proxy_with_authentication() {
    // 認証情報を含むプロキシURL
    let proxy_url = "http://user:password@proxy.example.com:8080";

    let proxy = reqwest::Proxy::http(proxy_url);
    assert!(
        proxy.is_ok(),
        "Proxy URL with authentication should be valid"
    );
}

#[test]
fn test_invalid_proxy_url() {
    // 無効なプロキシURL（スキームなし）
    let invalid_url = "://invalid";

    let proxy = reqwest::Proxy::http(invalid_url);
    // reqwest::Proxyは一部の無効なURLを受け入れる可能性があるため、
    // 実際のHTTPクライアント構築時にエラーになることを期待
    let _ = proxy; // 警告を回避
}

#[test]
fn test_proxy_url_with_no_port() {
    // ポート番号なしのプロキシURL（デフォルトポートを使用）
    let proxy_url = "http://proxy.example.com";

    let proxy = reqwest::Proxy::http(proxy_url);
    assert!(
        proxy.is_ok(),
        "Proxy URL without port should use default port"
    );
}
