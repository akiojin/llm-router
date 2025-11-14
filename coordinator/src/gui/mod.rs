//! GUIユーティリティ（トレイアイコンなど、Windows/macOSのみ）。

#![cfg(any(target_os = "windows", target_os = "macos"))]

/// コーディネーター用システムトレイ機能。
pub mod tray;
