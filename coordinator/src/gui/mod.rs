//! GUI utilities for the coordinator (Windows/macOS only).

#![cfg(any(target_os = "windows", target_os = "macos"))]

pub mod tray;
