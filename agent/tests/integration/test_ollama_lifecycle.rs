//! Integration Test: Ollama Lifecycle Management
//!
//! Ollamaの自動ダウンロード・起動・停止のライフサイクルをテスト

use ollama_coordinator_agent::ollama::OllamaManager;

#[tokio::test]
#[ignore] // 実際のOllamaダウンロードが必要なため、通常は無視
async fn test_ollama_ensure_running_auto_download() {
    // Arrange: Ollamaが未インストールの環境を想定
    let mut manager = OllamaManager::new(11435); // 別ポートを使用して干渉を避ける

    // Act: ensure_running() を呼び出す
    // 未インストール時は自動ダウンロード・起動が実行される
    let result = manager.ensure_running().await;

    // Assert: 正常に起動すること
    assert!(
        result.is_ok(),
        "Ollama should be downloaded and started: {:?}",
        result
    );

    // Assert: is_running() がtrueを返すこと
    assert!(
        manager.is_running().await,
        "Ollama should be running after ensure_running()"
    );

    // Cleanup: 停止
    let _ = manager.stop();
}

#[tokio::test]
async fn test_ollama_is_installed_check() {
    // Arrange
    let manager = OllamaManager::new(11434);

    // Act
    let is_installed = manager.is_installed();

    // Assert: インストール状態をチェックできること
    // （実際の値は環境依存だが、エラーが発生しないこと）
    println!("Ollama installed: {}", is_installed);
}

#[tokio::test]
#[ignore] // 実際のOllamaプロセス起動が必要
async fn test_ollama_start_and_stop() {
    // Arrange: Ollamaがインストール済みと仮定
    let mut manager = OllamaManager::new(11436);

    // Act: 起動
    let start_result = manager.ensure_running().await;
    assert!(
        start_result.is_ok(),
        "Failed to start Ollama: {:?}",
        start_result
    );

    // Assert: 起動確認
    assert!(manager.is_running().await, "Ollama should be running");

    // Act: 停止
    let stop_result = manager.stop();
    assert!(
        stop_result.is_ok(),
        "Failed to stop Ollama: {:?}",
        stop_result
    );

    // Assert: 停止確認（少し待ってから確認）
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    assert!(
        !manager.is_running().await,
        "Ollama should be stopped after stop()"
    );
}

#[tokio::test]
#[ignore] // バージョン取得にはOllamaインストールが必要
async fn test_ollama_get_version() {
    // Arrange: Ollamaがインストール済みと仮定
    let manager = OllamaManager::new(11434);

    // Act
    let version_result = manager.get_version().await;

    // Assert: バージョン情報が取得できること
    match version_result {
        Ok(version) => {
            println!("Ollama version: {}", version);
            assert!(!version.is_empty(), "Version should not be empty");
        }
        Err(e) => {
            // Ollamaが未インストールの場合はエラーが期待される
            println!("Ollama not installed (expected): {}", e);
        }
    }
}
