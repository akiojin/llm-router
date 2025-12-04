#pragma once

#include <vector>
#include <string>
#include <memory>

namespace llm_node {

struct GpuDevice {
    int id;
    std::string name;
    size_t memory_bytes;
    std::string compute_capability;
    std::string vendor;  // "nvidia", "amd", "apple"
    bool is_available;
};

class GpuDetector {
public:
    GpuDetector();
    ~GpuDetector();

    // Detect all available GPUs
    std::vector<GpuDevice> detect();

    // Check if any GPU is available
    bool hasGpu() const;

    // GPU必須チェック: 少なくとも1つの利用可能GPUがあるか
    bool requireGpu() const;

    // Get GPU by ID
    std::unique_ptr<GpuDevice> getGpuById(int id) const;

    // Get total GPU memory across all devices
    size_t getTotalMemory() const;

    // Get GPU capability score (for router compatibility)
    double getCapabilityScore() const;

private:
    std::vector<GpuDevice> detected_devices_;

    // Platform-specific detection methods
    std::vector<GpuDevice> detectCuda();
    std::vector<GpuDevice> detectMetal();
    std::vector<GpuDevice> detectRocm();

#ifdef LLM_NODE_TESTING
public:
    // テスト専用: 検出結果を直接セットして計算ロジックを検証する
    void setDetectedDevicesForTest(std::vector<GpuDevice> devices);
#endif
};

} // namespace llm_node
