//! Ollama Coordinator Common Library
//!
//! 共通型定義、プロトコル、設定、エラー型を提供

#![warn(missing_docs)]

/// 共通型定義
pub mod types;

/// 通信プロトコル定義
pub mod protocol;

/// 設定管理
pub mod config;

/// エラー型定義
pub mod error;
