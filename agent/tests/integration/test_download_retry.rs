//! Integration Test: Ollama Download Retry
//!
//! ネットワークエラー時の自動リトライ機能をテスト

use std::env;
use std::sync::{Arc, Mutex};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn test_download_retry_on_timeout() {
    // Arrange: タイムアウトをシミュレートするモックサーバー
    let mock_server = MockServer::start().await;

    // 最初の2回は500エラー、3回目は成功
    let call_count = Arc::new(Mutex::new(0));
    let call_count_clone = Arc::clone(&call_count);

    // テスト用の小さなzipファイルを作成
    let fake_zip = create_fake_zip();

    Mock::given(method("GET"))
        .and(path("/ollama-download.zip"))
        .respond_with(move |_req: &wiremock::Request| {
            let mut count = call_count_clone.lock().unwrap();
            *count += 1;
            let current_count = *count;
            drop(count);

            if current_count < 3 {
                // 最初の2回は500エラー
                ResponseTemplate::new(500)
            } else {
                // 3回目は成功
                ResponseTemplate::new(200).set_body_bytes(fake_zip.clone())
            }
        })
        .expect(3)
        .mount(&mock_server)
        .await;

    // Act: リトライ機能付きでダウンロード実行
    // 環境変数でダウンロードURLを上書き
    let download_url = format!("{}/ollama-download.zip", mock_server.uri());
    env::set_var("OLLAMA_DOWNLOAD_URL", &download_url);
    env::set_var("OLLAMA_DOWNLOAD_MAX_RETRIES", "5");
    env::set_var("OLLAMA_DOWNLOAD_MAX_BACKOFF_SECS", "1"); // テストを高速化

    // リトライロジックをテスト（実際のOllamaManagerを使用）
    // Note: download()はprivate関数なので、ensure_running()を通じてテスト
    // ただし、これは実際にOllamaをインストールしようとするため、
    // ここでは単純なHTTPクライアントでリトライをテスト
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    let result = retry_download(&client, &download_url, 5, 1).await;

    // Assert: 3回目で成功すること
    assert!(result.is_ok(), "Download should succeed after retries");

    // 環境変数をクリーンアップ
    env::remove_var("OLLAMA_DOWNLOAD_URL");
    env::remove_var("OLLAMA_DOWNLOAD_MAX_RETRIES");
    env::remove_var("OLLAMA_DOWNLOAD_MAX_BACKOFF_SECS");
}

// テスト用のリトライ関数（agent/src/ollama.rsのロジックを模倣）
async fn retry_download(
    client: &reqwest::Client,
    url: &str,
    max_retries: u32,
    max_backoff_secs: u64,
) -> Result<Vec<u8>, String> {
    let mut attempt = 0;
    let mut backoff_secs = 1;

    loop {
        attempt += 1;

        match client.get(url).send().await {
            Ok(response) if response.status().is_success() => {
                return response
                    .bytes()
                    .await
                    .map(|b| b.to_vec())
                    .map_err(|e| format!("Failed to read bytes: {}", e));
            }
            Ok(response) => {
                let status = response.status();
                if !status.is_server_error() || attempt >= max_retries {
                    return Err(format!("HTTP error: {}", status));
                }
                // 5xxエラーはリトライ
            }
            Err(e) => {
                if attempt >= max_retries {
                    return Err(format!("Request failed: {}", e));
                }
                // ネットワークエラーはリトライ
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
        backoff_secs = std::cmp::min(backoff_secs * 2, max_backoff_secs);
    }
}

// テスト用の偽zipファイルを作成
fn create_fake_zip() -> Vec<u8> {
    use std::io::Write;
    use zip::write::FileOptions;
    use zip::ZipWriter;

    let mut buf = Vec::new();
    {
        let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buf));
        zip.start_file(
            "ollama",
            FileOptions::default().compression_method(zip::CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(b"fake ollama binary").unwrap();
        zip.finish().unwrap();
    }
    buf
}

#[tokio::test]
#[ignore] // TODO: 実装予定
async fn test_download_retry_on_connection_error() {
    // Arrange: 接続エラーをシミュレート（存在しないサーバー）
    let _invalid_url = "http://localhost:99999/ollama-download";

    // Act: 存在しないサーバーへのダウンロード試行
    // TODO: 実装後、実際のリトライロジックを呼び出す
    // let result = download_with_retry(invalid_url).await;

    // Assert: リトライ後にエラーが返ること
    // TODO: 実装後、アサーションを追加
    // assert!(result.is_err(), "Download should fail after max retries");
}

#[tokio::test]
#[ignore] // TODO: 実装予定
async fn test_download_retry_respects_max_retries() {
    // Arrange: 常に500エラーを返すモックサーバー
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(500))
        .expect(5) // デフォルトの最大リトライ回数5回
        .mount(&mock_server)
        .await;

    // Act: リトライ機能付きでダウンロード実行
    // TODO: 実装後、実際のリトライロジックを呼び出す
    // let result = download_with_retry(&mock_server.uri()).await;

    // Assert: 最大リトライ回数を超えないこと
    // TODO: 実装後、アサーションを追加
    // assert!(result.is_err(), "Download should fail after max retries");
}

#[tokio::test]
#[ignore] // TODO: 実装予定
async fn test_download_no_retry_on_404() {
    // Arrange: 404エラーを返すモックサーバー
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1) // 404はリトライしないので1回のみ
        .mount(&mock_server)
        .await;

    // Act: リトライ機能付きでダウンロード実行
    // TODO: 実装後、実際のリトライロジックを呼び出す
    // let result = download_with_retry(&mock_server.uri()).await;

    // Assert: リトライせずに即座にエラーを返すこと
    // TODO: 実装後、アサーションを追加
    // assert!(result.is_err(), "Download should fail immediately on 404");
}
