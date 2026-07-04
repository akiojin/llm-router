//! LLM load balancer Common Library
//!
//! プロトコル、設定、エラー型を提供

#![warn(missing_docs)]

/// 通信プロトコル定義
pub mod protocol;

/// 設定管理
pub mod config;

/// エラー型定義
pub mod error;

/// ログユーティリティ
pub mod log;

/// 認証・認可関連の型定義
pub mod auth;

/// IPアドレス正規化ユーティリティ
pub mod ip;

/// HTTP クライアント共通ユーティリティ
pub mod http;
