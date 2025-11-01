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

/// GPU能力情報
#[derive(Debug, Clone)]
pub struct GpuCapability {
    /// GPUモデル名
    pub model_name: String,
    /// CUDA計算能力 (major, minor)
    pub compute_capability: (u32, u32),
    /// 最大クロック速度 (MHz)
    pub max_clock_mhz: u32,
    /// メモリ総容量 (MB)
    pub memory_total_mb: u64,
}

impl GpuCapability {
    /// GPU能力スコアを計算
    ///
    /// スコア計算式:
    /// score = (memory_gb * 100) + (max_clock_ghz * 100) + (compute_major * 1000)
    ///
    /// 例:
    /// - RTX 4090 (16GB, 2.5GHz, Compute 8.9): 1600 + 250 + 8000 = 9850
    /// - RTX 3080 (10GB, 1.7GHz, Compute 8.6): 1000 + 170 + 8000 = 9170
    /// - GTX 1660 (6GB, 1.8GHz, Compute 7.5): 600 + 180 + 7000 = 7780
    pub fn calculate_score(memory_mb: u64, max_clock_mhz: u32, compute_capability: (u32, u32)) -> u32 {
        let memory_gb = memory_mb / 1024;
        let clock_ghz = max_clock_mhz as f32 / 1000.0;
        let (compute_major, _compute_minor) = compute_capability;

        let score = (memory_gb * 100) as u32
            + (clock_ghz * 100.0) as u32
            + (compute_major * 1000);

        score
    }

    /// 自身のスコアを計算
    pub fn score(&self) -> u32 {
        Self::calculate_score(
            self.memory_total_mb,
            self.max_clock_mhz,
            self.compute_capability,
        )
    }
}

/// システムメトリクスコレクター
pub struct MetricsCollector {
    system: System,
    gpu: Option<GpuCollector>,
}

impl MetricsCollector {
    /// 新しいメトリクスコレクターを作成
    pub fn new() -> Self {
        Self::with_ollama_path(None)
    }

    /// ollamaバイナリのパスを指定してメトリクスコレクターを作成
    pub fn with_ollama_path(ollama_path: Option<std::path::PathBuf>) -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        // GPUバックエンドを優先順位で試行
        let gpu = GpuCollector::detect_gpu(ollama_path.as_deref());

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

    /// GPU能力情報を取得（静的な情報、初回のみ取得推奨）
    pub fn get_gpu_capability(&self) -> Option<GpuCapability> {
        self.gpu
            .as_ref()
            .and_then(|gpu| gpu.get_capability().ok())
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
    Nvidia(Box<NvidiaGpuCollector>),
    Amd(AmdGpuCollector),
    #[cfg(target_os = "macos")]
    AppleSilicon(AppleSiliconGpuCollector),
}

impl GpuCollector {
    /// GPUを検出（優先順位: 環境変数 → NVIDIA → AMD → Apple Silicon）
    fn detect_gpu(_ollama_path: Option<&std::path::Path>) -> Option<Self> {
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
            return Some(GpuCollector::Nvidia(Box::new(nvidia)));
        }

        // AMD GPUを試行
        if let Ok(amd) = AmdGpuCollector::new() {
            debug!("Detected AMD GPU");
            return Some(GpuCollector::Amd(amd));
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
            GpuCollector::Amd(gpu) => gpu.device_count(),
            #[cfg(target_os = "macos")]
            GpuCollector::AppleSilicon(gpu) => gpu.device_count(),
        }
    }

    fn model_name(&self) -> Option<String> {
        match self {
            GpuCollector::Env(gpu) => gpu.model_name(),
            GpuCollector::Nvidia(gpu) => gpu.model_name(),
            GpuCollector::Amd(gpu) => gpu.model_name(),
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
            GpuCollector::Amd(_gpu) => {
                // AMD GPUs don't provide runtime metrics via sysfs/KFD
                // ROCm SMI would be needed for detailed metrics
                Err(NvmlError::NotSupported)
            }
            #[cfg(target_os = "macos")]
            GpuCollector::AppleSilicon(_gpu) => {
                // Apple Silicon doesn't provide detailed metrics via Metal API
                Err(NvmlError::NotSupported)
            }
        }
    }

    fn get_capability(&self) -> Result<GpuCapability, NvmlError> {
        match self {
            GpuCollector::Nvidia(gpu) => gpu.get_capability(),
            _ => Err(NvmlError::NotSupported),
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
        // 事前チェック: デバイスファイルまたは/proc/driverでNVIDIA GPUの存在を確認
        if !Self::is_nvidia_gpu_present() {
            debug!("No NVIDIA GPU detected (device file check)");
            return Err(NvmlError::NotSupported);
        }

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

    /// NVIDIA GPUの存在をデバイスファイルや/proc/driverで確認
    fn is_nvidia_gpu_present() -> bool {
        use std::path::Path;

        // Method 1: /dev/nvidia0 デバイスファイル確認
        if Path::new("/dev/nvidia0").exists() {
            debug!("Found NVIDIA GPU via /dev/nvidia0");
            return true;
        }

        // Method 2: /proc/driver/nvidia/version 確認
        if Path::new("/proc/driver/nvidia/version").exists() {
            debug!("Found NVIDIA GPU via /proc/driver/nvidia/version");
            return true;
        }

        false
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

    /// GPU能力情報を取得（初回のみ、静的な情報）
    fn get_capability(&self) -> Result<GpuCapability, NvmlError> {
        // 最初のGPUの情報を返す（複数GPU環境では最も強力なGPUを返すべきだが、簡易実装として最初のGPUを使用）
        if self.device_indices.is_empty() {
            return Err(NvmlError::NotSupported);
        }

        let device = self.nvml.device_by_index(self.device_indices[0])?;

        let model_name = device.name()?;
        let cuda_capability = device.cuda_compute_capability()?;
        let compute_capability = (cuda_capability.major as u32, cuda_capability.minor as u32);
        let max_clock_mhz = device.max_clock_info(nvml_wrapper::enum_wrappers::device::Clock::Graphics)?;
        let memory_info = device.memory_info()?;
        let memory_total_mb = memory_info.total / (1024 * 1024);

        Ok(GpuCapability {
            model_name,
            compute_capability,
            max_clock_mhz,
            memory_total_mb,
        })
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
        // Method 1: lscpu コマンドで "Vendor ID: Apple" を確認（Docker環境でも動作）
        if let Ok(device_name) = Self::detect_via_lscpu() {
            return Ok(Self { device_name });
        }

        // Method 2: /proc/cpuinfo で "CPU implementer : 0x61" を確認
        if let Ok(device_name) = Self::detect_via_cpuinfo() {
            return Ok(Self { device_name });
        }

        // Method 3: Metal API（macOSネイティブのみ）
        if let Some(device) = MetalDevice::system_default() {
            let device_name = device.name().to_string();
            return Ok(Self { device_name });
        }

        Err("No Apple Silicon GPU detected".to_string())
    }

    /// lscpuコマンドでApple Siliconを検出
    fn detect_via_lscpu() -> Result<String, String> {
        use std::process::Command;

        let output = Command::new("lscpu")
            .output()
            .map_err(|e| format!("lscpu command failed: {}", e))?;

        if !output.status.success() {
            return Err("lscpu command failed".to_string());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // "Vendor ID: Apple" をチェック
        for line in stdout.lines() {
            if line.contains("Vendor ID") && line.contains("Apple") {
                debug!("Detected Apple Silicon via lscpu");
                return Ok("Apple Silicon".to_string());
            }
        }

        Err("Not Apple Silicon (lscpu)".to_string())
    }

    /// /proc/cpuinfoでApple Siliconを検出
    fn detect_via_cpuinfo() -> Result<String, String> {
        use std::fs;

        let content = fs::read_to_string("/proc/cpuinfo")
            .map_err(|e| format!("Failed to read /proc/cpuinfo: {}", e))?;

        // "CPU implementer : 0x61" (Apple) をチェック
        for line in content.lines() {
            if line.contains("CPU implementer") && line.contains("0x61") {
                debug!("Detected Apple Silicon via /proc/cpuinfo");
                return Ok("Apple Silicon".to_string());
            }
        }

        Err("Not Apple Silicon (cpuinfo)".to_string())
    }

    fn device_count(&self) -> u32 {
        // Apple Siliconは統合GPU（1つとしてカウント）
        1
    }

    fn model_name(&self) -> Option<String> {
        Some(self.device_name.clone())
    }
}

/// AMD GPUコレクター
struct AmdGpuCollector {
    device_name: Option<String>,
    device_count: u32,
}

impl AmdGpuCollector {
    fn new() -> Result<Self, String> {
        // Method 1: KFD Topology (sysfs) で vendor_id 0x1002 を確認（最も確実）
        if let Ok((device_name, count)) = Self::detect_via_kfd_topology() {
            return Ok(Self {
                device_name: Some(device_name),
                device_count: count,
            });
        }

        // Method 2: /dev/kfd デバイスファイル確認
        if Self::detect_via_kfd_device() {
            return Ok(Self {
                device_name: Some("AMD GPU".to_string()),
                device_count: 1,
            });
        }

        // Method 3: DRM デバイスで vendor 0x1002 確認
        if Self::detect_via_drm_device() {
            return Ok(Self {
                device_name: Some("AMD GPU".to_string()),
                device_count: 1,
            });
        }

        Err("No AMD GPU detected".to_string())
    }

    /// KFD Topology (sysfs) でAMD GPUを検出
    fn detect_via_kfd_topology() -> Result<(String, u32), String> {
        use std::fs;
        use std::path::Path;

        let kfd_path = "/sys/class/kfd/kfd/topology/nodes";
        if !Path::new(kfd_path).exists() {
            return Err("KFD topology not found".to_string());
        }

        let entries =
            fs::read_dir(kfd_path).map_err(|e| format!("Failed to read KFD topology: {}", e))?;

        let mut gpu_count = 0u32;

        for entry in entries.flatten() {
            let properties_path = entry.path().join("properties");
            if properties_path.exists() {
                if let Ok(content) = fs::read_to_string(&properties_path) {
                    // vendor_id 0x1002 (AMD) をチェック
                    for line in content.lines() {
                        if line.contains("vendor_id") && line.contains("0x1002") {
                            gpu_count += 1;
                            debug!("Detected AMD GPU via KFD topology");
                            break;
                        }
                    }
                }
            }
        }

        if gpu_count > 0 {
            Ok(("AMD GPU".to_string(), gpu_count))
        } else {
            Err("No AMD GPU found in KFD topology".to_string())
        }
    }

    /// /dev/kfd デバイスファイルでAMD GPUを検出
    fn detect_via_kfd_device() -> bool {
        use std::path::Path;

        if Path::new("/dev/kfd").exists() {
            debug!("Found AMD GPU via /dev/kfd");
            return true;
        }

        false
    }

    /// DRM デバイスでAMD GPUを検出
    fn detect_via_drm_device() -> bool {
        use std::fs;
        use std::path::Path;

        let drm_path = "/sys/class/drm";
        if !Path::new(drm_path).exists() {
            return false;
        }

        if let Ok(entries) = fs::read_dir(drm_path) {
            for entry in entries.flatten() {
                let vendor_path = entry.path().join("device/vendor");
                if vendor_path.exists() {
                    if let Ok(vendor) = fs::read_to_string(&vendor_path) {
                        if vendor.trim() == "0x1002" {
                            debug!("Found AMD GPU via DRM device");
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    fn device_count(&self) -> u32 {
        self.device_count
    }

    fn model_name(&self) -> Option<String> {
        self.device_name.clone()
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
    fn test_calculate_gpu_capability_score() {
        use super::GpuCapability;

        // RTX 4090 example: 16GB, 2.5GHz, Compute 8.9
        let score = GpuCapability::calculate_score(16384, 2520, (8, 9));
        // Expected: (16 * 100) + (2.52 * 100) + (8 * 1000) = 1600 + 252 + 8000 = 9852
        assert!(score >= 9800 && score <= 10000, "Score should be around 9852, got {}", score);

        // RTX 3080 example: 10GB, 1.7GHz, Compute 8.6
        let score = GpuCapability::calculate_score(10240, 1710, (8, 6));
        // Expected: (10 * 100) + (1.71 * 100) + (8 * 1000) = 1000 + 171 + 8000 = 9171
        assert!(score >= 9100 && score <= 9200, "Score should be around 9171, got {}", score);

        // GTX 1660 example: 6GB, 1.8GHz, Compute 7.5
        let score = GpuCapability::calculate_score(6144, 1785, (7, 5));
        // Expected: (6 * 100) + (1.785 * 100) + (7 * 1000) = 600 + 178 + 7000 = 7778
        assert!(score >= 7700 && score <= 7800, "Score should be around 7778, got {}", score);
    }

    #[test]
    fn test_gpu_capability_creation() {
        let capability = GpuCapability {
            model_name: "NVIDIA GeForce RTX 4090".to_string(),
            compute_capability: (8, 9),
            max_clock_mhz: 2520,
            memory_total_mb: 16384,
        };

        assert_eq!(capability.model_name, "NVIDIA GeForce RTX 4090");
        assert_eq!(capability.compute_capability, (8, 9));
        assert_eq!(capability.max_clock_mhz, 2520);
        assert_eq!(capability.memory_total_mb, 16384);
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
                model.contains("Apple")
                    || model.contains("M1")
                    || model.contains("M2")
                    || model.contains("M3")
                    || model.contains("M4"),
                "GPU model should be Apple Silicon: {}",
                model
            );
        }

        // GPU count should be at least 1
        assert!(
            collector.gpu_count().is_some(),
            "Apple Silicon should report GPU count"
        );
    }

    #[test]
    fn test_gpu_detection_cross_platform() {
        let collector = MetricsCollector::new();

        // has_gpu() should return consistent results
        let has_gpu1 = collector.has_gpu();
        let has_gpu2 = collector.has_gpu();
        assert_eq!(has_gpu1, has_gpu2, "GPU detection should be consistent");

        // If GPU is detected, gpu_model() may return Some or None depending on the platform
        if collector.has_gpu() {
            let _model = collector.gpu_model();
            // Note: GPU model can be None for some platforms (e.g., ollama ps detection)
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

        assert!(
            !collector.has_gpu(),
            "Should not detect GPU when env var is false"
        );

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
