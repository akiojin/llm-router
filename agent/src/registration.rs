//! 登録フロー関連のヘルパー
//!
//! GPU情報の検証や登録条件に関する補助ロジックを提供する。

use ollama_coordinator_common::types::GpuDeviceInfo;

/// GPU情報が登録要件を満たしているか判定する。
pub fn gpu_devices_valid(devices: &[GpuDeviceInfo]) -> bool {
    !devices.is_empty() && devices.iter().all(GpuDeviceInfo::is_valid)
}
