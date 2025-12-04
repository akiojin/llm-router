#pragma once

#include <httplib.h>
#include <string>
#include <atomic>
#include <chrono>
#include <memory>
#include "metrics/prometheus_exporter.h"

namespace llm_node {

class ModelSync;
class RouterClient;

class NodeEndpoints {
public:
    void setGpuInfo(size_t devices, size_t total_mem_bytes, double capability) { gpu_devices_ = devices; gpu_total_mem_ = total_mem_bytes; gpu_capability_ = capability; }
    void setModelSync(std::shared_ptr<ModelSync> sync) { model_sync_ = std::move(sync); }
    void setRouterClient(std::shared_ptr<RouterClient> client) { router_client_ = std::move(client); }
    NodeEndpoints();
    void registerRoutes(httplib::Server& server);

private:
    std::string health_status_;
    std::chrono::steady_clock::time_point start_time_;
    std::atomic<uint64_t> pull_count_{0};
    metrics::PrometheusExporter exporter_;
    size_t gpu_devices_{0};
    size_t gpu_total_mem_{0};
    double gpu_capability_{0.0};
    std::shared_ptr<ModelSync> model_sync_;
    std::shared_ptr<RouterClient> router_client_;
};

}  // namespace llm_node
