//! Ollama Coordinator Agent
//!
//! 各マシン上で動作するエージェントアプリケーション

#![warn(missing_docs)]

/// Coordinator通信クライアント
pub mod client {
    //! 自己登録、ハートビート送信
    // TODO: T059で実装
}

/// Ollama管理（自動ダウンロード・起動）
pub mod ollama;

/// メトリクス収集
pub mod metrics {
    //! CPU/メモリ監視
    // TODO: T061で実装
}

/// GUI
pub mod gui {
    //! システムトレイ、設定ウィンドウ
    // TODO: T062で実装
}

/// 設定管理
pub mod config {
    //! 設定ファイル読み込み
    // TODO: T071で実装
}
