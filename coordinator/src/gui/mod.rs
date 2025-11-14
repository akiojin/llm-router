//! GUIユーティリティ（トレイアイコンなど、Windows/macOSのみ）。

#![cfg(any(target_os = "windows", target_os = "macos"))]

pub mod tray;
