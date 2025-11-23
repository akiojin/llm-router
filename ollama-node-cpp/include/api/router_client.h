#pragma once

#include <string>
#include <optional>
#include <vector>
#include <chrono>

namespace ollama_node {

struct NodeInfo {
    std::string hostname;
    std::string address;
    int port;
    size_t total_gpu_memory_bytes;
    double capability_score;
    std::vector<std::string> ready_models;
    bool initializing{false};
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
    std::string error;
};

class RouterClient {
public:
    explicit RouterClient(std::string base_url, std::chrono::milliseconds timeout = std::chrono::milliseconds(5000));

    NodeRegistrationResult registerNode(const NodeInfo& info);

    bool sendHeartbeat(const std::string& node_id,
                       const std::optional<std::string>& status = std::nullopt,
                       const std::optional<HeartbeatMetrics>& metrics = std::nullopt,
                       int max_retries = 2);

private:
    std::string base_url_;
    std::chrono::milliseconds timeout_;
};

}  // namespace ollama_node
