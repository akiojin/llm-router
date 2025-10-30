//! Ollama Coordinator Agent
//!
//! 各マシン上で動作するエージェントアプリケーション

#![warn(missing_docs)]

/// Coordinator通信クライアント（登録・ハートビート）
pub mod client;

/// Ollama管理（自動ダウンロード・起動）
pub mod ollama;

/// メトリクス収集（CPU/メモリ監視）
pub mod metrics;

/// GUI（システムトレイ、設定ウィンドウ）
pub mod gui {
    // TODO: T062で実装
}

/// 設定管理（設定ファイル読み込み）
pub mod config {
    // TODO: T071で実装
}
