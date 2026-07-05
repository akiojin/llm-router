//! デバイス/GPU 情報型（/api/system から取得する DeviceInfo とその構成型）
//!
//! arch-review [H6]: types/endpoint.rs から凝集した DTO 群を分離。親は
//! `pub use device::*` で再エクスポートし既存パス・テストの参照を維持する。

use serde::{Deserialize, Serialize};

/// デバイスタイプ（SPEC-f8e3a1b7）
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceType {
    /// CPU推論
    #[default]
    Cpu,
    /// GPU推論
    Gpu,
}

/// デバイス情報（SPEC-f8e3a1b7）
///
/// /api/system APIから取得したデバイス情報を格納
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceInfo {
    /// デバイスタイプ（CPU/GPU）
    pub device_type: DeviceType,
    /// GPUデバイス情報（GPU推論の場合のみ）
    #[serde(default)]
    pub gpu_devices: Vec<GpuDevice>,
}

/// GPU デバイス情報（SPEC-f8e3a1b7）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDevice {
    /// デバイス名（例: "NVIDIA RTX 4090"）
    pub name: String,
    /// 総メモリ（バイト）
    pub total_memory_bytes: u64,
    /// 使用中メモリ（バイト）
    #[serde(default)]
    pub used_memory_bytes: u64,
}
