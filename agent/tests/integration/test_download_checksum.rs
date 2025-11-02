//! Integration Test: Ollama Download Checksum Verification
//!
//! ダウンロードしたファイルのSHA256チェックサム検証をテスト

use sha2::{Digest, Sha256};
use std::env;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_download_with_valid_checksum() {
    // Arrange: テストデータとそのSHA256ハッシュを生成
    let test_data = b"fake ollama binary content for testing";
    let mut hasher = Sha256::new();
    hasher.update(test_data);
    let expected_checksum = format!("{:x}", hasher.finalize());

    let mock_server = MockServer::start().await;

    // バイナリファイルをモック
    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&mock_server)
        .await;

    // チェックサムファイルをモック
    Mock::given(method("GET"))
        .and(path("/ollama-download.sha256"))
        .respond_with(ResponseTemplate::new(200).set_body_string(expected_checksum.clone()))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Act: チェックサム検証付きでダウンロード
    let download_url = format!("{}/ollama-download", mock_server.uri());
    env::set_var("OLLAMA_DOWNLOAD_URL", &download_url);
    env::set_var("OLLAMA_VERIFY_CHECKSUM", "true");

    let result = download_with_checksum(&download_url).await;

    // Assert: ダウンロード成功
    assert!(
        result.is_ok(),
        "Download with valid checksum should succeed"
    );

    // Cleanup
    env::remove_var("OLLAMA_DOWNLOAD_URL");
    env::remove_var("OLLAMA_VERIFY_CHECKSUM");
}

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_download_with_invalid_checksum() {
    // Arrange: テストデータと異なるチェックサムを用意
    let test_data = b"fake ollama binary content";
    let wrong_checksum = "0000000000000000000000000000000000000000000000000000000000000000";

    let mock_server = MockServer::start().await;

    // バイナリファイルをモック
    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&mock_server)
        .await;

    // 間違ったチェックサムファイルをモック
    Mock::given(method("GET"))
        .and(path("/ollama-download.sha256"))
        .respond_with(ResponseTemplate::new(200).set_body_string(wrong_checksum))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Act: チェックサム検証付きでダウンロード
    let download_url = format!("{}/ollama-download", mock_server.uri());
    env::set_var("OLLAMA_DOWNLOAD_URL", &download_url);
    env::set_var("OLLAMA_VERIFY_CHECKSUM", "true");

    let result = download_with_checksum(&download_url).await;

    // Assert: チェックサム不一致でエラー
    assert!(
        result.is_err(),
        "Download with invalid checksum should fail"
    );
    assert!(
        result.unwrap_err().contains("checksum"),
        "Error should mention checksum mismatch"
    );

    // Cleanup
    env::remove_var("OLLAMA_DOWNLOAD_URL");
    env::remove_var("OLLAMA_VERIFY_CHECKSUM");
}

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_download_checksum_file_not_found() {
    // Arrange: バイナリはあるがチェックサムファイルがない
    let test_data = b"fake ollama binary";

    let mock_server = MockServer::start().await;

    // バイナリファイルをモック
    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&mock_server)
        .await;

    // チェックサムファイルは404
    Mock::given(method("GET"))
        .and(path("/ollama-download.sha256"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Act: チェックサム検証付きでダウンロード
    let download_url = format!("{}/ollama-download", mock_server.uri());
    env::set_var("OLLAMA_DOWNLOAD_URL", &download_url);
    env::set_var("OLLAMA_VERIFY_CHECKSUM", "true");

    let result = download_with_checksum(&download_url).await;

    // Assert: チェックサムファイルが見つからない場合はエラー
    assert!(
        result.is_err(),
        "Download should fail when checksum file is not found"
    );

    // Cleanup
    env::remove_var("OLLAMA_DOWNLOAD_URL");
    env::remove_var("OLLAMA_VERIFY_CHECKSUM");
}

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_download_without_checksum_verification() {
    // Arrange: チェックサム検証を無効化
    let test_data = b"fake ollama binary";

    let mock_server = MockServer::start().await;

    // バイナリファイルをモック
    Mock::given(method("GET"))
        .and(path("/ollama-download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&mock_server)
        .await;

    // チェックサムファイルは呼ばれない（expect(0)）
    Mock::given(method("GET"))
        .and(path("/ollama-download.sha256"))
        .respond_with(ResponseTemplate::new(200).set_body_string("dummy"))
        .expect(0)
        .mount(&mock_server)
        .await;

    // Act: チェックサム検証なしでダウンロード
    let download_url = format!("{}/ollama-download", mock_server.uri());
    env::set_var("OLLAMA_DOWNLOAD_URL", &download_url);
    env::remove_var("OLLAMA_VERIFY_CHECKSUM"); // 明示的に削除

    let result = download_with_checksum(&download_url).await;

    // Assert: チェックサム検証なしで成功
    assert!(
        result.is_ok(),
        "Download without checksum verification should succeed"
    );

    // Cleanup
    env::remove_var("OLLAMA_DOWNLOAD_URL");
}

// テスト用のダウンロード関数（実装と同じシグネチャ）
async fn download_with_checksum(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::new();

    // バイナリをダウンロード
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read bytes: {}", e))?
        .to_vec();

    // チェックサム検証が有効な場合
    if std::env::var("OLLAMA_VERIFY_CHECKSUM").is_ok() {
        // チェックサムURLを生成
        let checksum_url = format!("{}.sha256", url);

        // チェックサムをダウンロード
        let checksum_response = client
            .get(&checksum_url)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch checksum: {}", e))?;

        if !checksum_response.status().is_success() {
            return Err(format!(
                "Checksum file not found: HTTP {}",
                checksum_response.status()
            ));
        }

        let expected_checksum = checksum_response
            .text()
            .await
            .map_err(|e| format!("Failed to read checksum: {}", e))?
            .trim()
            .to_string();

        // 実際のチェックサムを計算
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual_checksum = format!("{:x}", hasher.finalize());

        // チェックサムを比較
        if actual_checksum != expected_checksum {
            return Err(format!(
                "Checksum mismatch: expected {}, got {}",
                expected_checksum, actual_checksum
            ));
        }
    }

    Ok(bytes)
}
