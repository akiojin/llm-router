#include "api/router_client.h"

#include <httplib.h>
#include <nlohmann/json.hpp>
#include <thread>

namespace llm_node {

using json = nlohmann::json;

namespace {
std::unique_ptr<httplib::Client> make_client(const std::string& base_url, std::chrono::milliseconds timeout) {
    auto client = std::make_unique<httplib::Client>(base_url.c_str());
    const int sec = static_cast<int>(timeout.count() / 1000);
    const int usec = static_cast<int>((timeout.count() % 1000) * 1000);
    client->set_connection_timeout(sec, usec);
    client->set_read_timeout(sec, usec);
    return client;
}
}  // namespace

RouterClient::RouterClient(std::string base_url, std::chrono::milliseconds timeout)
    : base_url_(std::move(base_url)), timeout_(timeout) {}

NodeRegistrationResult RouterClient::registerNode(const NodeInfo& info) {
    auto cli = make_client(base_url_, timeout_);

    // Build gpu_devices array
    json gpu_devices_json = json::array();
    for (const auto& gpu : info.gpu_devices) {
        json device = {
            {"model", gpu.model},
            {"count", gpu.count}
        };
        if (gpu.memory.has_value()) {
            device["memory"] = gpu.memory.value();
        }
        gpu_devices_json.push_back(device);
    }

    // Build payload matching router RegisterRequest
    json payload = {
        {"machine_name", info.machine_name},
        {"ip_address", info.ip_address},
        {"runtime_version", info.runtime_version},
        {"runtime_port", info.runtime_port},
        {"gpu_available", info.gpu_available},
        {"gpu_devices", gpu_devices_json}
    };

    // Add optional fields
    if (info.gpu_count.has_value()) {
        payload["gpu_count"] = info.gpu_count.value();
    }
    if (info.gpu_model.has_value()) {
        payload["gpu_model"] = info.gpu_model.value();
    }

    auto res = cli->Post("/api/nodes", payload.dump(), "application/json");

    NodeRegistrationResult result;
    if (!res) {
        result.error = "connection failed";
        return result;
    }

    if (res->status >= 200 && res->status < 300) {
        try {
            auto body = json::parse(res->body);
            // Router returns node_id as UUID string
            if (body.contains("node_id")) {
                result.node_id = body["node_id"].get<std::string>();
            }
            // Extract agent_token for heartbeat authentication
            if (body.contains("agent_token")) {
                result.agent_token = body["agent_token"].get<std::string>();
            }
            result.success = !result.node_id.empty() && !result.agent_token.empty();
            if (!result.success) {
                if (result.node_id.empty()) {
                    result.error = "missing node_id";
                } else {
                    result.error = "missing agent_token";
                }
            }
        } catch (const std::exception& e) {
            result.error = e.what();
        }
    } else {
        result.error = res->body;
    }

    return result;
}

bool RouterClient::sendHeartbeat(const std::string& node_id, const std::string& agent_token,
                                 const std::optional<std::string>& /*status_opt*/,
                                 const std::optional<HeartbeatMetrics>& metrics, int max_retries) {
    auto cli = make_client(base_url_, timeout_);

    // Build HealthCheckRequest payload matching router protocol
    json payload = {
        {"node_id", node_id},
        {"cpu_usage", metrics.has_value() ? static_cast<float>(metrics->cpu_utilization) : 0.0f},
        {"memory_usage", metrics.has_value() ?
            (metrics->mem_total_bytes > 0 ?
                static_cast<float>(metrics->mem_used_bytes) / static_cast<float>(metrics->mem_total_bytes) * 100.0f : 0.0f)
            : 0.0f},
        {"active_requests", 0},
        {"loaded_models", json::array()},
        {"initializing", false},
    };

    // Add optional gpu_usage if metrics available
    if (metrics.has_value()) {
        payload["gpu_usage"] = static_cast<float>(metrics->gpu_utilization);
    } else {
        payload["gpu_usage"] = nullptr;
    }

    // Set authentication header
    httplib::Headers headers = {
        {"X-Agent-Token", agent_token}
    };

    for (int attempt = 0; attempt <= max_retries; ++attempt) {
        auto res = cli->Post("/api/health", headers, payload.dump(), "application/json");
        if (res && res->status >= 200 && res->status < 300) return true;
        if (attempt == max_retries) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(100 * (attempt + 1)));
    }
    return false;
}

bool RouterClient::reportProgress(const std::string& task_id, double progress,
                                   std::optional<double> speed, int max_retries) {
    auto cli = make_client(base_url_, timeout_);

    json payload = {
        {"progress", progress}
    };

    if (speed.has_value()) {
        payload["speed"] = speed.value();
    }

    std::string path = "/api/tasks/" + task_id + "/progress";

    for (int attempt = 0; attempt <= max_retries; ++attempt) {
        auto res = cli->Post(path.c_str(), payload.dump(), "application/json");
        if (res && res->status >= 200 && res->status < 300) return true;
        if (attempt == max_retries) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(100 * (attempt + 1)));
    }
    return false;
}

}  // namespace llm_node
