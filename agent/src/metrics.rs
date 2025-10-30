//! メトリクス収集
//!
//! CPU/メモリ使用率の監視

use ollama_coordinator_common::error::{AgentError, AgentResult};
use sysinfo::System;

/// システムメトリクスコレクター
pub struct MetricsCollector {
    system: System,
}

impl MetricsCollector {
    /// 新しいメトリクスコレクターを作成
    pub fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();

        Self { system }
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
    pub fn collect_metrics(&mut self) -> AgentResult<(f32, f32)> {
        let cpu_usage = self.get_cpu_usage()?;
        let memory_usage = self.get_memory_usage()?;

        Ok((cpu_usage, memory_usage))
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
        let (cpu_usage, memory_usage) = collector.collect_metrics().unwrap();

        assert!((0.0..=100.0).contains(&cpu_usage));
        assert!((0.0..=100.0).contains(&memory_usage));
    }
}
