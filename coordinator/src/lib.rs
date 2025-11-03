//! Ollama Coordinator Server
//!
//! 複数Ollamaインスタンスを管理する中央サーバー

#![warn(missing_docs)]

/// REST APIハンドラー
pub mod api;

/// ロードバランサー（ラウンドロビン、負荷ベースのロードバランシング）
pub mod balancer;

/// ヘルスチェック監視
pub mod health;

/// エージェント登録管理
pub mod registry;

/// データベースアクセス
pub mod db;

/// メトリクス収集・管理
pub mod metrics;

/// 設定管理（設定ファイル読み込み）
///
/// 将来的にTOMLベースの設定ファイル対応を追加予定（SPEC-32e2b31a T055）
pub mod config {
    // 未実装: 設定ファイル読み込み
}

/// アプリケーション状態
#[derive(Clone)]
pub struct AppState {
    /// エージェントレジストリ
    pub registry: registry::AgentRegistry,
    /// ロードマネージャー
    pub load_manager: balancer::LoadManager,
    /// リクエスト履歴ストレージ
    pub request_history: std::sync::Arc<db::request_history::RequestHistoryStorage>,
}
