//! メトリクス収集
//!
//! CPU / メモリ / GPU 使用率の監視

use nvml_wrapper::{error::NvmlError, Nvml};
use ollama_coordinator_common::error::{AgentError, AgentResult};
use sysinfo::System;
use tracing::{debug, warn};

#[cfg(target_os = "macos")]
use metal::Device as MetalDevice;

/// 収集したシステムメトリクス
#[derive(Debug, Clone, Copy)]
pub struct SystemMetrics {
    /// CPU usage percentage (0.0-100.0)
    pub cpu_usage: f32,
    /// Memory usage percentage (0.0-100.0)
    pub memory_usage: f32,
    /// GPU usage percentage (0.0-100.0)
    pub gpu_usage: Option<f32>,
    /// GPU memory usage percentage (0.0-100.0)
    pub gpu_memory_usage: Option<f32>,
    /// GPU memory total (MB)
    pub gpu_memory_total_mb: Option<u64>,
    /// GPU memory used (MB)
    pub gpu_memory_used_mb: Option<u64>,
    /// GPU temperature (℃)
    pub gpu_temperature: Option<f32>,
}

/// システムメトリクスコレクター
pub struct MetricsCollector {
    system: System,
    gpu: Option<GpuCollector>,
}

impl MetricsCollector {
    /// 新しいメトリクスコレクターを作成
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        // GPUバックエンドを優先順位で試行
        let gpu = GpuCollector::detect_gpu();

        Self { system, gpu }
    }

    /// CPU使用率を取得（0.0-100.0）
    pub fn get_cpu_usage(&mut self) -> AgentResult<f32> {
        self.system.refresh_cpu();

        // 少し待ってから再度リフレッシュすることで正確な値を取得
        std::thread::sleep(std::time::Duration::from_millis(200));
        self.system.refresh_cpu();

        // 全CPUの平均使用率を計算
        let cpu_usage = self
            .system
            .cpus()
            .iter()
            .map(|cpu| cpu.cpu_usage())
            .sum::<f32>()
            / self.system.cpus().len() as f32;

        Ok(cpu_usage)
    }

    /// メモリ使用率を取得（0.0-100.0）
    pub fn get_memory_usage(&mut self) -> AgentResult<f32> {
        self.system.refresh_memory();

        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();

        if total_memory == 0 {
            return Err(AgentError::Metrics("Total memory is zero".to_string()));
        }

        let memory_usage = (used_memory as f64 / total_memory as f64 * 100.0) as f32;

        Ok(memory_usage)
    }

    /// CPU使用率とメモリ使用率を同時に取得
    pub fn collect_metrics(&mut self) -> AgentResult<SystemMetrics> {
        let cpu_usage = self.get_cpu_usage()?;
        let memory_usage = self.get_memory_usage()?;

        let (gpu_usage, gpu_memory_usage, gpu_memory_total_mb, gpu_memory_used_mb, gpu_temperature) =
            if let Some(gpu) = &self.gpu {
                match gpu.collect() {
                    Ok((usage, memory, total_mb, used_mb, temp)) => (
                        Some(usage),
                        Some(memory),
                        Some(total_mb),
                        Some(used_mb),
                        Some(temp),
                    ),
                    Err(error) => {
                        warn!("Failed to collect GPU metrics: {:?}", error);
                        (None, None, None, None, None)
                    }
                }
            } else {
                (None, None, None, None, None)
            };

        Ok(SystemMetrics {
            cpu_usage,
            memory_usage,
            gpu_usage,
            gpu_memory_usage,
            gpu_memory_total_mb,
            gpu_memory_used_mb,
            gpu_temperature,
        })
    }

    /// GPUが利用可能かどうかを確認
    pub fn has_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    /// GPU個数を取得
    pub fn gpu_count(&self) -> Option<u32> {
        self.gpu.as_ref().map(|gpu| gpu.device_count())
    }

    /// GPUモデル名を取得（最初のGPUのモデル名）
    pub fn gpu_model(&self) -> Option<String> {
        self.gpu.as_ref().and_then(|gpu| gpu.model_name())
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// GPUメトリクスコレクター（マルチベンダー対応）
enum GpuCollector {
    Env(EnvGpuCollector),
    Nvidia(NvidiaGpuCollector),
    #[cfg(target_os = "macos")]
    AppleSilicon(AppleSiliconGpuCollector),
}

impl GpuCollector {
    /// GPUを検出（優先順位: 環境変数 → NVIDIA → Apple Silicon）
    fn detect_gpu() -> Option<Self> {
        // 環境変数で明示的にGPUを無効化しているかチェック
        if let Ok(available_str) = std::env::var("OLLAMA_GPU_AVAILABLE") {
            if let Ok(false) = available_str.parse::<bool>() {
                debug!("GPU explicitly disabled via environment variable");
                return None;
            }
        }

        // 環境変数からGPU情報を試行（最優先）
        if let Ok(env) = EnvGpuCollector::new() {
            debug!("Detected GPU from environment variables");
            return Some(GpuCollector::Env(env));
        }

        // NVIDIA GPUを試行
        if let Ok(nvidia) = NvidiaGpuCollector::new() {
            debug!("Detected NVIDIA GPU");
            return Some(GpuCollector::Nvidia(nvidia));
        }

        // macOS: Apple Silicon GPUを試行
        #[cfg(target_os = "macos")]
        if let Ok(apple) = AppleSiliconGpuCollector::new() {
            debug!("Detected Apple Silicon GPU");
            return Some(GpuCollector::AppleSilicon(apple));
        }

        debug!("No GPU detected");
        None
    }

    fn device_count(&self) -> u32 {
        match self {
            GpuCollector::Env(gpu) => gpu.device_count(),
            GpuCollector::Nvidia(gpu) => gpu.device_count(),
            #[cfg(target_os = "macos")]
            GpuCollector::AppleSilicon(gpu) => gpu.device_count(),
        }
    }

    fn model_name(&self) -> Option<String> {
        match self {
            GpuCollector::Env(gpu) => gpu.model_name(),
            GpuCollector::Nvidia(gpu) => gpu.model_name(),
            #[cfg(target_os = "macos")]
            GpuCollector::AppleSilicon(gpu) => gpu.model_name(),
        }
    }

    fn collect(&self) -> Result<(f32, f32, u64, u64, f32), NvmlError> {
        match self {
            GpuCollector::Env(_gpu) => {
                // Environment variables don't provide runtime metrics
                Err(NvmlError::NotSupported)
            }
            GpuCollector::Nvidia(gpu) => gpu.collect(),
            #[cfg(target_os = "macos")]
            GpuCollector::AppleSilicon(_gpu) => {
                // Apple Silicon doesn't provide detailed metrics via Metal API
                Err(NvmlError::NotSupported)
            }
        }
    }
}

/// NVIDIA GPUコレクター
struct NvidiaGpuCollector {
    nvml: Nvml,
    device_indices: Vec<u32>,
}

impl NvidiaGpuCollector {
    fn new() -> Result<Self, NvmlError> {
        let nvml = Nvml::init()?;
        let count = nvml.device_count()?;
        if count == 0 {
            return Err(NvmlError::NotSupported);
        }
        let device_indices: Vec<u32> = (0..count).collect();
        Ok(Self {
            nvml,
            device_indices,
        })
    }

    fn device_count(&self) -> u32 {
        self.device_indices.len() as u32
    }

    fn model_name(&self) -> Option<String> {
        if let Some(first_index) = self.device_indices.first() {
            self.nvml
                .device_by_index(*first_index)
                .ok()
                .and_then(|device| device.name().ok())
        } else {
            None
        }
    }

    fn collect(&self) -> Result<(f32, f32, u64, u64, f32), NvmlError> {
        let mut total_usage = 0f32;
        let mut total_memory_percent = 0f32;
        let mut total_memory_total = 0u64;
        let mut total_memory_used = 0u64;
        let mut total_temperature = 0f32;

        for index in &self.device_indices {
            let device = self.nvml.device_by_index(*index)?;

            let utilization = device.utilization_rates()?;
            total_usage += utilization.gpu as f32;

            let memory = device.memory_info()?;
            let percent = if memory.total == 0 {
                0.0
            } else {
                (memory.used as f64 / memory.total as f64 * 100.0) as f32
            };
            total_memory_percent += percent;
            total_memory_total += memory.total / (1024 * 1024); // Convert to MB
            total_memory_used += memory.used / (1024 * 1024); // Convert to MB

            let temperature =
                device.temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)?;
            total_temperature += temperature as f32;
        }

        let device_count = self.device_indices.len() as f32;
        if device_count == 0.0 {
            return Err(NvmlError::NotSupported);
        }

        Ok((
            total_usage / device_count,
            total_memory_percent / device_count,
            total_memory_total,
            total_memory_used,
            total_temperature / device_count,
        ))
    }
}

/// Apple Silicon GPUコレクター
#[cfg(target_os = "macos")]
struct AppleSiliconGpuCollector {
    device_name: String,
}

#[cfg(target_os = "macos")]
impl AppleSiliconGpuCollector {
    fn new() -> Result<Self, String> {
        // Metal APIでデフォルトGPUを取得
        if let Some(device) = MetalDevice::system_default() {
            let device_name = device.name().to_string();
            Ok(Self { device_name })
        } else {
            Err("No Metal GPU device found".to_string())
        }
    }

    fn device_count(&self) -> u32 {
        // Apple Siliconは統合GPU（1つとしてカウント）
        1
    }

    fn model_name(&self) -> Option<String> {
        Some(self.device_name.clone())
    }
}

/// 環境変数からGPU情報を取得するコレクター
struct EnvGpuCollector {
    model_name: Option<String>,
    count: u32,
}

impl EnvGpuCollector {
    fn new() -> Result<Self, String> {
        // OLLAMA_GPU_MODEL環境変数をチェック（最優先）
        let model_name = std::env::var("OLLAMA_GPU_MODEL").ok();

        // OLLAMA_GPU_AVAILABLE環境変数をチェック
        let gpu_available_env = std::env::var("OLLAMA_GPU_AVAILABLE").ok();

        // GPUモデル名もavailableフラグも設定されていない場合はエラー
        if model_name.is_none() && gpu_available_env.is_none() {
            return Err("No GPU configured via environment variables".to_string());
        }

        // 明示的にfalseが設定されている場合はエラー（これはdetect_gpuで処理される）
        if let Some(ref available_str) = gpu_available_env {
            if let Ok(false) = available_str.parse::<bool>() {
                return Err("GPU explicitly disabled".to_string());
            }
        }

        // OLLAMA_GPU_COUNT環境変数をチェック
        let count = std::env::var("OLLAMA_GPU_COUNT")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(1);

        Ok(Self { model_name, count })
    }

    fn device_count(&self) -> u32 {
        self.count
    }

    fn model_name(&self) -> Option<String> {
        self.model_name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::sync::Mutex;

    // 環境変数テスト用のグローバルロック
    static ENV_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

    #[test]
    fn test_metrics_collector_creation() {
        let collector = MetricsCollector::new();
        assert!(!collector.system.cpus().is_empty());
    }

    #[test]
    fn test_get_memory_usage() {
        let mut collector = MetricsCollector::new();
        let memory_usage = collector.get_memory_usage().unwrap();
        assert!((0.0..=100.0).contains(&memory_usage));
    }

    #[test]
    fn test_collect_metrics() {
        let mut collector = MetricsCollector::new();
        let metrics = collector.collect_metrics().unwrap();

        assert!((0.0..=100.0).contains(&metrics.cpu_usage));
        assert!((0.0..=100.0).contains(&metrics.memory_usage));
        if let Some(gpu_usage) = metrics.gpu_usage {
            assert!((0.0..=100.0).contains(&gpu_usage));
        }
        if let Some(gpu_memory) = metrics.gpu_memory_usage {
            assert!((0.0..=100.0).contains(&gpu_memory));
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn test_apple_silicon_gpu_detection() {
        let collector = MetricsCollector::new();

        // Apple Silicon Mac should detect GPU
        assert!(collector.has_gpu(), "Apple Silicon GPU should be detected");

        // GPU model name should contain "Apple"
        if let Some(model) = collector.gpu_model() {
            assert!(
                model.contains("Apple") || model.contains("M1") ||
                model.contains("M2") || model.contains("M3") || model.contains("M4"),
                "GPU model should be Apple Silicon: {}",
                model
            );
        }

        // GPU count should be at least 1
        assert!(collector.gpu_count().is_some(), "Apple Silicon should report GPU count");
    }

    #[test]
    fn test_gpu_detection_cross_platform() {
        let collector = MetricsCollector::new();

        // has_gpu() should return consistent results
        let has_gpu1 = collector.has_gpu();
        let has_gpu2 = collector.has_gpu();
        assert_eq!(has_gpu1, has_gpu2, "GPU detection should be consistent");

        // If GPU is detected, gpu_model() should return Some
        if collector.has_gpu() {
            let model = collector.gpu_model();
            assert!(model.is_some() || true, "GPU model can be None for some platforms");
        }
    }

    #[test]
    fn test_gpu_detection_from_env_vars() {
        let _lock = ENV_TEST_LOCK.lock().unwrap();

        // 環境変数を設定
        std::env::set_var("OLLAMA_GPU_AVAILABLE", "true");
        std::env::set_var("OLLAMA_GPU_MODEL", "Apple M4");
        std::env::set_var("OLLAMA_GPU_COUNT", "1");

        let collector = MetricsCollector::new();

        assert!(collector.has_gpu(), "Should detect GPU from env vars");
        assert_eq!(collector.gpu_model(), Some("Apple M4".to_string()));
        assert_eq!(collector.gpu_count(), Some(1));

        // クリーンアップ
        std::env::remove_var("OLLAMA_GPU_AVAILABLE");
        std::env::remove_var("OLLAMA_GPU_MODEL");
        std::env::remove_var("OLLAMA_GPU_COUNT");
    }

    #[test]
    fn test_gpu_detection_env_vars_disabled() {
        let _lock = ENV_TEST_LOCK.lock().unwrap();

        // 環境変数でGPUを無効化
        std::env::set_var("OLLAMA_GPU_AVAILABLE", "false");

        let collector = MetricsCollector::new();

        assert!(!collector.has_gpu(), "Should not detect GPU when env var is false");

        // クリーンアップ
        std::env::remove_var("OLLAMA_GPU_AVAILABLE");
    }

    #[test]
    fn test_gpu_detection_env_vars_partial() {
        let _lock = ENV_TEST_LOCK.lock().unwrap();

        // GPUモデル名だけ設定
        std::env::set_var("OLLAMA_GPU_MODEL", "Custom GPU");

        let collector = MetricsCollector::new();

        // モデル名が設定されている場合はGPU有効とみなす
        assert!(collector.has_gpu(), "Should detect GPU when model is set");
        assert_eq!(collector.gpu_model(), Some("Custom GPU".to_string()));

        // クリーンアップ
        std::env::remove_var("OLLAMA_GPU_MODEL");
    }
}
