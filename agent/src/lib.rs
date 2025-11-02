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

/// 登録フロー補助ロジック
pub mod registration;

/// GUI（システムトレイ、設定ウィンドウ）
///
/// 将来的にGUIアプリケーションとして実装予定（SPEC-32e2b31a T062）
pub mod gui {
    // 未実装: GUIモジュール
}

/// 設定管理（設定ファイル読み込み）
///
/// 将来的にTOMLベースの設定ファイル対応を追加予定（SPEC-32e2b31a T071）
pub mod config {
    // 未実装: 設定ファイル読み込み
}
