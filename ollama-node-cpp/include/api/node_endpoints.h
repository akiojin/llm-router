#pragma once

#include <httplib.h>
#include <string>
#include <atomic>
#include <chrono>

namespace ollama_node {

class NodeEndpoints {
public:
    NodeEndpoints();
    void registerRoutes(httplib::Server& server);

private:
    std::string health_status_;
    std::chrono::steady_clock::time_point start_time_;
    std::atomic<uint64_t> pull_count_{0};
};

}  // namespace ollama_node
