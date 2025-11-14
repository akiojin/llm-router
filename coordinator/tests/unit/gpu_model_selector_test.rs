//! GPU能力ベースモデル選択ユニットテスト
//!
//! TDD GREEN: GPUメモリに基づいて最適なモデルを選択するロジック

#[cfg(test)]
mod tests {
    use ollama_coordinator_coordinator::models::gpu_selector::select_model_by_gpu_memory;

    /// T021: 16GB GPUには gpt-oss:20b を選択
    #[test]
    fn test_select_model_by_gpu_memory_16gb() {
        let gpu_memory: u64 = 16_000_000_000;
        let model = select_model_by_gpu_memory(gpu_memory);
        assert_eq!(model, "gpt-oss:20b", "16GB GPU should select gpt-oss:20b");
    }

    /// T021: 8GB GPUには gpt-oss:7b を選択
    #[test]
    fn test_select_model_by_gpu_memory_8gb() {
        let gpu_memory: u64 = 8_000_000_000;
        let model = select_model_by_gpu_memory(gpu_memory);
        assert_eq!(model, "gpt-oss:7b", "8GB GPU should select gpt-oss:7b");
    }

    /// T021: 4.5GB GPUには gpt-oss:3b を選択
    #[test]
    fn test_select_model_by_gpu_memory_4_5gb() {
        let gpu_memory: u64 = 4_500_000_000;
        let model = select_model_by_gpu_memory(gpu_memory);
        assert_eq!(model, "gpt-oss:3b", "4.5GB GPU should select gpt-oss:3b");
    }

    /// T021: 4.5GB未満のGPUには gpt-oss:1b を選択
    #[test]
    fn test_select_model_by_gpu_memory_small() {
        let gpu_memory: u64 = 2_000_000_000;
        let model = select_model_by_gpu_memory(gpu_memory);
        assert_eq!(model, "gpt-oss:1b", "Small GPU should select gpt-oss:1b");
    }

    /// T021: 境界値テスト - ちょうど16GB
    #[test]
    fn test_select_model_boundary_16gb() {
        let gpu_memory: u64 = 16_000_000_000;
        let model = select_model_by_gpu_memory(gpu_memory);
        assert_eq!(model, "gpt-oss:20b");
    }

    /// T021: 境界値テスト - ちょうど8GB
    #[test]
    fn test_select_model_boundary_8gb() {
        let gpu_memory: u64 = 8_000_000_000;
        let model = select_model_by_gpu_memory(gpu_memory);
        assert_eq!(model, "gpt-oss:7b");
    }

    /// T021: 境界値テスト - ちょうど4.5GB
    #[test]
    fn test_select_model_boundary_4_5gb() {
        let gpu_memory: u64 = 4_500_000_000;
        let model = select_model_by_gpu_memory(gpu_memory);
        assert_eq!(model, "gpt-oss:3b");
    }

    /// T021: 非常に大きなGPUメモリ (24GB)
    #[test]
    fn test_select_model_very_large_gpu() {
        let gpu_memory: u64 = 24_000_000_000;
        let model = select_model_by_gpu_memory(gpu_memory);
        assert_eq!(model, "gpt-oss:20b");
    }
}
