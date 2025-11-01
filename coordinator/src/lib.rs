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

/// 設定管理（設定ファイル読み込み）
pub mod config {
    // TODO: T055で実装
}

/// アプリケーション状態
#[derive(Clone)]
pub struct AppState {
    /// エージェントレジストリ
    pub registry: registry::AgentRegistry,
    /// ロードマネージャー
    pub load_manager: balancer::LoadManager,
}
