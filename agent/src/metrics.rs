//! メトリクス収集
//!
//! CPU / メモリ / GPU 使用率の監視

use nvml_wrapper::{error::NvmlError, Nvml};
use ollama_coordinator_common::error::{AgentError, AgentResult};
use sysinfo::System;
use tracing::{debug, warn};

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

        let gpu = match GpuCollector::new() {
            Ok(collector) => Some(collector),
            Err(error) => {
                // GPUが存在しない環境やNVMLが利用できない環境ではGPUメトリクスを無効化
                debug!("GPU metrics unavailable: {:?}", error);
                None
            }
        };

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

        let (gpu_usage, gpu_memory_usage) = if let Some(gpu) = &self.gpu {
            match gpu.collect() {
                Ok((usage, memory)) => (Some(usage), Some(memory)),
                Err(error) => {
                    warn!("Failed to collect GPU metrics: {:?}", error);
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(SystemMetrics {
            cpu_usage,
            memory_usage,
            gpu_usage,
            gpu_memory_usage,
        })
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

/// NVIDIA GPUメトリクスコレクター
struct GpuCollector {
    nvml: Nvml,
    device_indices: Vec<u32>,
}

impl GpuCollector {
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

    fn collect(&self) -> Result<(f32, f32), NvmlError> {
        let mut total_usage = 0f32;
        let mut total_memory = 0f32;

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
            total_memory += percent;
        }

        let device_count = self.device_indices.len() as f32;
        if device_count == 0.0 {
            return Err(NvmlError::NotSupported);
        }

        Ok((total_usage / device_count, total_memory / device_count))
    }
}
