//! GUI関連のモジュール（システムトレイなど）。

#[cfg(any(target_os = "windows", target_os = "macos"))]
/// Windows/macOS向けシステムトレイ機能。
pub mod tray;
