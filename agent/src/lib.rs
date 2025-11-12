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

/// GUI連携（システムトレイなど）
pub mod gui;

/// 設定管理（Webパネル+永続化）
pub mod settings;

/// 設定管理（将来的にTOML対応予定）
pub mod config;
