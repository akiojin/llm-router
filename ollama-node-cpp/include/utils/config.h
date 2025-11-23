#pragma once

#include <chrono>
#include <cstddef>
#include <string>
#include <utility>
#include <filesystem>

namespace ollama_node {

struct DownloadConfig {
    int max_retries{2};
    std::chrono::milliseconds backoff{200};
    size_t max_concurrency{4};
    size_t max_bytes_per_sec{0};
    size_t chunk_size{4096};
};

DownloadConfig loadDownloadConfig();
std::pair<DownloadConfig, std::string> loadDownloadConfigWithLog();

struct NodeConfig {
    std::string router_url{"http://127.0.0.1:11434"};
    std::string models_dir;
    int node_port{11435};
    int heartbeat_interval_sec{10};
    bool require_gpu{true};
};

NodeConfig loadNodeConfig();
std::pair<NodeConfig, std::string> loadNodeConfigWithLog();

}  // namespace ollama_node
