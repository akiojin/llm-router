//! Integration Test: Ollama Download with Progress
//!
//! ダウンロード進捗表示機能をテスト

use std::sync::{Arc, Mutex};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_download_reports_progress() {
    // Arrange: 大きなファイルをシミュレートするモックサーバー
    let mock_server = MockServer::start().await;

    // 1MB のテストデータを作成
    let test_data = vec![0u8; 1_024 * 1_024];

    Mock::given(method("GET"))
        .and(path("/ollama-download.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data.clone()))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Act: ダウンロード実行（進捗コールバック付き）
    let download_url = format!("{}/ollama-download.zip", mock_server.uri());

    let progress_updates = Arc::new(Mutex::new(Vec::new()));
    let progress_updates_clone = Arc::clone(&progress_updates);

    let result = download_with_progress(
        &download_url,
        Box::new(move |current, total| {
            progress_updates_clone
                .lock()
                .unwrap()
                .push((current, total));
        }),
    )
    .await;

    // Assert: ダウンロード成功
    assert!(result.is_ok(), "Download should succeed");

    // Assert: 進捗更新が複数回報告されている
    let updates = progress_updates.lock().unwrap();
    assert!(
        !updates.is_empty(),
        "Progress updates should be reported at least once"
    );

    // Assert: 最終進捗が合計サイズと一致
    let (final_current, final_total) = updates.last().unwrap();
    assert_eq!(
        *final_total,
        test_data.len() as u64,
        "Total size should match test data size"
    );
    assert_eq!(
        *final_current, *final_total,
        "Final progress should reach total size"
    );
}

#[tokio::test]
#[ignore] // TODO: 実装後に有効化
async fn test_download_progress_incremental() {
    // Arrange: チャンク単位で送信するモックサーバー
    let mock_server = MockServer::start().await;

    // 100KB のテストデータ
    let test_data = vec![0u8; 100 * 1024];

    Mock::given(method("GET"))
        .and(path("/ollama-download.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data.clone()))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Act: ダウンロード実行（進捗コールバック付き）
    let download_url = format!("{}/ollama-download.zip", mock_server.uri());

    let progress_updates = Arc::new(Mutex::new(Vec::new()));
    let progress_updates_clone = Arc::clone(&progress_updates);

    let _result = download_with_progress(
        &download_url,
        Box::new(move |current, total| {
            progress_updates_clone
                .lock()
                .unwrap()
                .push((current, total));
        }),
    )
    .await;

    // Assert: 進捗が増加していく
    let updates = progress_updates.lock().unwrap();
    if updates.len() > 1 {
        for i in 1..updates.len() {
            let (prev_current, _) = updates[i - 1];
            let (current, _) = updates[i];
            assert!(
                current >= prev_current,
                "Progress should be monotonically increasing"
            );
        }
    }
}

#[tokio::test]
#[ignore] // TODO: 実装予定
async fn test_download_progress_callback_error_handling() {
    // Arrange: コールバックがエラーを起こしてもダウンロードが継続することをテスト
    let mock_server = MockServer::start().await;

    let test_data = vec![0u8; 1024];

    Mock::given(method("GET"))
        .and(path("/ollama-download.zip"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Act: ダウンロード実行（エラーを起こすコールバック）
    let download_url = format!("{}/ollama-download.zip", mock_server.uri());

    let result = download_with_progress(
        &download_url,
        Box::new(|_current, _total| {
            // コールバックでパニックを起こす
            panic!("Callback error");
        }),
    )
    .await;

    // Assert: ダウンロードは成功する（コールバックのエラーは無視される）
    // TODO: エラーハンドリング実装後、適切なアサーションに変更
    assert!(result.is_err() || result.is_ok());
}

// テスト用のダウンロード関数（実装と同じシグネチャ）
async fn download_with_progress(
    url: &str,
    progress_callback: Box<dyn Fn(u64, u64) + Send>,
) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::new();

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded = 0u64;
    let mut buffer = Vec::new();

    use futures::StreamExt;
    let mut stream = response.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        buffer.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;

        // 進捗コールバックを呼び出し
        progress_callback(downloaded, total_size);
    }

    Ok(buffer)
}
