//! LLM Router Server
//!
//! 複数LLMノードを管理する中央サーバー

#![warn(missing_docs)]

/// REST APIハンドラー
pub mod api;

/// ロードバランサー（ラウンドロビン、負荷ベースのロードバランシング）
pub mod balancer;

/// クラウド呼び出しメトリクス
pub mod cloud_metrics;

/// ヘルスチェック監視
pub mod health;

/// ノード登録管理
pub mod registry;

/// データベースアクセス
pub mod db;

/// Ollama公式ライブラリAPI通信
pub mod ollama;

/// ダウンロードタスク管理
pub mod tasks;

/// メトリクス収集・管理
pub mod metrics;

/// モデル管理（GPU選択ロジック）
pub mod models;

/// ロギング初期化ユーティリティ
pub mod logging;

/// GUIユーティリティ（トレイアイコン等）
#[cfg(any(target_os = "windows", target_os = "macos"))]
pub mod gui;

/// 設定管理（環境変数ヘルパー）
pub mod config;

/// JWT秘密鍵管理
pub mod jwt_secret;

/// 認証・認可機能
pub mod auth;

/// CLIインターフェース
pub mod cli;

/// アプリケーション状態
#[derive(Clone)]
pub struct AppState {
    /// ノードレジストリ
    pub registry: registry::NodeRegistry,
    /// ロードマネージャー
    pub load_manager: balancer::LoadManager,
    /// リクエスト履歴ストレージ
    pub request_history: std::sync::Arc<db::request_history::RequestHistoryStorage>,
    /// ダウンロードタスクマネージャー
    pub task_manager: tasks::DownloadTaskManager,
    /// データベース接続プール
    pub db_pool: sqlx::SqlitePool,
    /// JWT秘密鍵
    pub jwt_secret: String,
}
