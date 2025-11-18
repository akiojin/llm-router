//! GPU能力ベースモデル選択
//!
//! GPUメモリに基づいて最適なモデルを自動選択

/// GPUメモリに基づいて最適なモデルを選択
///
/// # 選択ルール
/// - 16GB以上: gpt-oss:20b
/// - 8GB以上: gpt-oss:7b
/// - 4.5GB以上: gpt-oss:3b
/// - 4.5GB未満: gpt-oss:1b
///
/// # Arguments
/// * `gpu_memory` - GPUメモリ容量（バイト単位）
///
/// # Returns
/// 推奨されるモデル名
pub fn select_model_by_gpu_memory(gpu_memory: u64) -> String {
    const GB_16: u64 = 16_000_000_000;
    const GB_8: u64 = 8_000_000_000;
    const GB_4_5: u64 = 4_500_000_000;

    if gpu_memory >= GB_16 {
        "gpt-oss:20b".to_string()
    } else if gpu_memory >= GB_8 {
        "gpt-oss:7b".to_string()
    } else if gpu_memory >= GB_4_5 {
        "gpt-oss:3b".to_string()
    } else {
        "gpt-oss:1b".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_16gb_selects_20b() {
        assert_eq!(select_model_by_gpu_memory(16_000_000_000), "gpt-oss:20b");
    }

    #[test]
    fn test_24gb_selects_20b() {
        assert_eq!(select_model_by_gpu_memory(24_000_000_000), "gpt-oss:20b");
    }

    #[test]
    fn test_8gb_selects_7b() {
        assert_eq!(select_model_by_gpu_memory(8_000_000_000), "gpt-oss:7b");
    }

    #[test]
    fn test_12gb_selects_7b() {
        assert_eq!(select_model_by_gpu_memory(12_000_000_000), "gpt-oss:7b");
    }

    #[test]
    fn test_4_5gb_selects_3b() {
        assert_eq!(select_model_by_gpu_memory(4_500_000_000), "gpt-oss:3b");
    }

    #[test]
    fn test_6gb_selects_3b() {
        assert_eq!(select_model_by_gpu_memory(6_000_000_000), "gpt-oss:3b");
    }

    #[test]
    fn test_2gb_selects_1b() {
        assert_eq!(select_model_by_gpu_memory(2_000_000_000), "gpt-oss:1b");
    }

    #[test]
    fn test_4gb_selects_1b() {
        assert_eq!(select_model_by_gpu_memory(4_000_000_000), "gpt-oss:1b");
    }
}
