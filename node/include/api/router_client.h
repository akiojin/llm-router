#pragma once

#include <string>
#include <optional>
#include <vector>
#include <chrono>

namespace llm_node {

/// GPU device info for registration (matches router protocol)
struct GpuDeviceInfoForRouter {
    std::string model;           // GPU model name (e.g., "Apple M4 Max")
    uint32_t count{1};           // Number of this GPU type
    std::optional<uint64_t> memory;  // Memory in bytes (optional)
};

/// Node registration info (matches router RegisterRequest)
struct NodeInfo {
    std::string machine_name;    // Machine name
    std::string ip_address;      // IP address
    std::string runtime_version;  // LLM runtime version (e.g., "0.1.0")
    uint16_t runtime_port;        // LLM runtime port (default: 11434)
    bool gpu_available{true};    // GPU available flag
    std::vector<GpuDeviceInfoForRouter> gpu_devices;  // GPU device list
    std::optional<uint32_t> gpu_count;   // Total GPU count (optional)
    std::optional<std::string> gpu_model; // Primary GPU model (optional)
};

struct HeartbeatMetrics {
    double cpu_utilization{0.0};
    double gpu_utilization{0.0};
    size_t mem_used_bytes{0};
    size_t mem_total_bytes{0};
};

struct NodeRegistrationResult {
    bool success{false};
    std::string node_id;
    std::string agent_token;
    std::string error;
};

class RouterClient {
public:
    explicit RouterClient(std::string base_url, std::chrono::milliseconds timeout = std::chrono::milliseconds(5000));

    NodeRegistrationResult registerNode(const NodeInfo& info);

    bool sendHeartbeat(const std::string& node_id,
                       const std::string& agent_token,
                       const std::optional<std::string>& status = std::nullopt,
                       const std::optional<HeartbeatMetrics>& metrics = std::nullopt,
                       int max_retries = 2);

    /// T034: Report download progress to router
    bool reportProgress(const std::string& task_id,
                        double progress,
                        std::optional<double> speed = std::nullopt,
                        int max_retries = 2);

private:
    std::string base_url_;
    std::chrono::milliseconds timeout_;
};

}  // namespace llm_node
