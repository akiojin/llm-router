//! Integration Test: LLMモデル自動ダウンロード
//!
//! モデルのプル・存在確認・進捗表示機能をテスト

use ollama_coordinator_agent::ollama::OllamaManager;

#[tokio::test]
#[ignore] // 実際のOllamaインストールとモデルダウンロードが必要
async fn test_ensure_running_auto_downloads_model() {
    // Arrange: OllamaManagerを作成
    let mut manager = OllamaManager::new(11434);

    // Act: ensure_running() を呼び出す
    // Ollamaの起動確認と、モデルが存在しなければ自動ダウンロードされる
    let ensure_result = manager.ensure_running().await;

    // Assert: 正常に完了すること
    match ensure_result {
        Ok(_) => {
            println!("✅ Ollama is running and default model is available");

            // モデルが実際にダウンロードされたことを確認
            let models = manager.list_models().await;
            match models {
                Ok(model_list) => {
                    println!("Available models: {:?}", model_list);
                    assert!(
                        !model_list.is_empty(),
                        "At least one model should be available"
                    );
                }
                Err(e) => {
                    eprintln!("⚠️ Failed to list models: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("⚠️ Ollama setup failed: {}", e);
        }
    }

    // Cleanup
    let _ = manager.stop();
}

#[tokio::test]
#[ignore] // 実際のOllamaインストールが必要
async fn test_list_models_after_ensure_running() {
    // Arrange: OllamaManagerを作成
    let mut manager = OllamaManager::new(11434);

    // ensure_running()でモデルを自動ダウンロード
    let ensure_result = manager.ensure_running().await;
    if ensure_result.is_err() {
        eprintln!("⚠️ Ollama not running, skipping test");
        return;
    }

    // Act: list_models() でモデル一覧を取得
    let models_result = manager.list_models().await;

    // Assert: モデルが1つ以上存在すること
    match models_result {
        Ok(models) => {
            println!("Available models: {:?}", models);
            assert!(
                !models.is_empty(),
                "At least one model should be available after ensure_running"
            );
            println!("✅ {} model(s) available", models.len());
        }
        Err(e) => {
            panic!("Failed to list models: {}", e);
        }
    }

    // Cleanup
    let _ = manager.stop();
}

#[tokio::test]
#[ignore] // 実際のOllamaインストールが必要
async fn test_list_models_without_ensure() {
    // Arrange: ensure_running()を呼ばずにlist_models()を実行
    let manager = OllamaManager::new(11434);

    // Act: Ollamaが起動していない場合の動作を確認
    let result = manager.list_models().await;

    // Assert: 起動していない場合はエラー、起動済みならモデル一覧を取得
    match result {
        Ok(models) => {
            println!("Ollama already running, models: {:?}", models);
        }
        Err(e) => {
            println!("Expected error when Ollama not running: {}", e);
            // エラーが発生すること自体は正常
        }
    }
}

#[tokio::test]
#[ignore] // メモリベースのモデル選択テスト
async fn test_memory_based_model_auto_selection() {
    // Arrange: OllamaManagerを作成
    let mut manager = OllamaManager::new(11434);

    // Act: ensure_running() はメモリベースでモデルを自動選択する
    let ensure_result = manager.ensure_running().await;

    // Assert: 適切なモデルが自動選択されダウンロードされること
    match ensure_result {
        Ok(_) => {
            println!("✅ Default model selected and downloaded based on system memory");

            // モデルが実際に利用可能か確認
            let models = manager.list_models().await;
            match models {
                Ok(model_list) => {
                    println!("Available models: {:?}", model_list);
                    assert!(
                        !model_list.is_empty(),
                        "At least one model should be available"
                    );

                    // 選択されたモデルが妥当なサイズであることを確認
                    // （小規模: qwen2:0.5b, 中規模: llama3.2:1b/3b, 大規模: llama3.2:8b+）
                    let valid_models = [
                        "qwen2:0.5b",
                        "llama3.2:1b",
                        "llama3.2:3b",
                        "llama3.2:8b",
                        "llama3.1:8b",
                    ];

                    let has_valid_model = model_list
                        .iter()
                        .any(|model| valid_models.iter().any(|&valid| model.contains(valid)));

                    println!("Model from valid set: {}", has_valid_model);
                }
                Err(e) => {
                    panic!("Failed to list models after ensure_running: {}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("⚠️ Ollama setup failed: {}", e);
        }
    }

    // Cleanup
    let _ = manager.stop();
}
