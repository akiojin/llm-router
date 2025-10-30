//! Agent Integration Test Runner
//!
//! 統合テスト実行用エントリーポイント
//!
//! 実行方法:
//! - 全テスト（ignored含む）: `cargo test --test integration_tests -- --ignored`
//! - 通常テスト: `cargo test --test integration_tests`
//!
//! 注意:
//! - ignored テストはCoordinatorサーバーまたはOllamaインストールが必要
//! - TEST_COORDINATOR_URL環境変数でCoordinator URLを指定可能

mod integration;
