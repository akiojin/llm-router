#include "api/router_client.h"

#include <httplib.h>
#include <nlohmann/json.hpp>
#include <thread>

namespace ollama_node {

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

    json payload = {
        {"hostname", info.hostname},
        {"address", info.address},
        {"port", info.port},
        {"gpu_total_memory_bytes", info.total_gpu_memory_bytes},
        {"capability_score", info.capability_score},
        {"initializing", info.initializing},
        {"ready_models", info.ready_models},
    };

    auto res = cli->Post("/api/nodes", payload.dump(), "application/json");

    NodeRegistrationResult result;
    if (!res) {
        result.error = "connection failed";
        return result;
    }

    if (res->status >= 200 && res->status < 300) {
        try {
            auto body = json::parse(res->body);
            result.node_id = body.value("node_id", "");
            result.success = !result.node_id.empty();
            if (!result.success) {
                result.error = "missing node_id";
            }
        } catch (const std::exception& e) {
            result.error = e.what();
        }
    } else {
        result.error = res->body;
    }

    return result;
}

bool RouterClient::sendHeartbeat(const std::string& node_id, const std::optional<std::string>& status_opt,
                                 const std::optional<HeartbeatMetrics>& metrics, int max_retries) {
    auto cli = make_client(base_url_, timeout_);

    json payload = {
        {"node_id", node_id},
        {"status", status_opt.value_or("ready")},
    };

    if (metrics.has_value()) {
        payload["metrics"] = {
            {"cpu_utilization", metrics->cpu_utilization},
            {"gpu_utilization", metrics->gpu_utilization},
            {"mem_used_bytes", metrics->mem_used_bytes},
            {"mem_total_bytes", metrics->mem_total_bytes},
        };
    }

    for (int attempt = 0; attempt <= max_retries; ++attempt) {
        auto res = cli->Post("/api/nodes/heartbeat", payload.dump(), "application/json");
        if (res && res->status >= 200 && res->status < 300) return true;
        if (attempt == max_retries) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(100 * (attempt + 1)));
    }
    return false;
}

}  // namespace ollama_node
