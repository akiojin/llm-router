//! Integration Test: Ollama Download via Proxy
//!
//! プロキシ経由でのダウンロード機能をテスト

use std::env;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_download_via_http_proxy() {
    // Arrange: プロキシサーバーとターゲットサーバーをモック
    let target_server = MockServer::start().await;
    let test_data = b"fake ollama binary for proxy test";

    // ターゲットサーバーのレスポンス設定
    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&target_server)
        .await;

    // Act: HTTP_PROXY環境変数を設定してダウンロード
    let _download_url = format!("{}/ollama-download", target_server.uri());

    // Note: reqwestは環境変数HTTP_PROXYを自動的に読み取る
    // 実際のプロキシテストはモックプロキシサーバーが必要だが、
    // ここでは環境変数の設定とクライアント構築のみをテスト
    env::set_var("HTTP_PROXY", "http://proxy.example.com:8080");

    // ダウンロード関数を呼び出す
    // let result = OllamaManager::download(&download_url).await;

    // Assert: ダウンロード成功
    // assert!(result.is_ok(), "Download via proxy should succeed");

    // Cleanup
    env::remove_var("HTTP_PROXY");
}

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_download_via_https_proxy() {
    // Arrange: HTTPS_PROXY環境変数を設定
    let target_server = MockServer::start().await;
    let test_data = b"fake ollama binary for https proxy test";

    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&target_server)
        .await;

    // Act: HTTPS_PROXY環境変数を設定
    let _download_url = format!("{}/ollama-download", target_server.uri());
    env::set_var("HTTPS_PROXY", "http://proxy.example.com:8443");

    // ダウンロード関数を呼び出す
    // let result = OllamaManager::download(&download_url).await;

    // Assert: ダウンロード成功
    // assert!(result.is_ok(), "Download via HTTPS proxy should succeed");

    // Cleanup
    env::remove_var("HTTPS_PROXY");
}

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_proxy_authentication() {
    // Arrange: 認証情報を含むプロキシURL
    let target_server = MockServer::start().await;
    let test_data = b"fake ollama binary for authenticated proxy test";

    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&target_server)
        .await;

    // Act: 認証情報付きHTTP_PROXY環境変数を設定
    let _download_url = format!("{}/ollama-download", target_server.uri());
    env::set_var("HTTP_PROXY", "http://user:password@proxy.example.com:8080");

    // ダウンロード関数を呼び出す
    // let result = OllamaManager::download(&download_url).await;

    // Assert: ダウンロード成功
    // assert!(result.is_ok(), "Download via authenticated proxy should succeed");

    // Cleanup
    env::remove_var("HTTP_PROXY");
}

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_no_proxy_exclusion() {
    // Arrange: NO_PROXY環境変数でプロキシをバイパス
    let target_server = MockServer::start().await;
    let test_data = b"fake ollama binary for no-proxy test";

    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&target_server)
        .await;

    // Act: HTTP_PROXYとNO_PROXYを設定
    let _download_url = format!("{}/ollama-download", target_server.uri());
    env::set_var("HTTP_PROXY", "http://proxy.example.com:8080");
    env::set_var("NO_PROXY", "localhost,127.0.0.1,.example.com");

    // ダウンロード関数を呼び出す
    // let result = OllamaManager::download(&download_url).await;

    // Assert: ダウンロード成功（プロキシをバイパス）
    // assert!(result.is_ok(), "Download should bypass proxy with NO_PROXY");

    // Cleanup
    env::remove_var("HTTP_PROXY");
    env::remove_var("NO_PROXY");
}
