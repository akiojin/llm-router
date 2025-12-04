//! ノードランタイム（llama.cpp）とのAPI通信
//!
//! モデル一覧の取得と管理を行うクライアント実装

pub mod client;

pub use client::RuntimeClient;
