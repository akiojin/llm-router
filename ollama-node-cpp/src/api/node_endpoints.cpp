#include "api/node_endpoints.h"

#include <stdexcept>
#include <nlohmann/json.hpp>

namespace ollama_node {

NodeEndpoints::NodeEndpoints() : health_status_("ok") {}

void NodeEndpoints::registerRoutes(httplib::Server& server) {
    start_time_ = std::chrono::steady_clock::now();

    server.Post("/pull", [this](const httplib::Request&, httplib::Response& res) {
        pull_count_.fetch_add(1);
        nlohmann::json body = {{"status", "accepted"}};
        res.set_content(body.dump(), "application/json");
    });

    server.Get("/health", [this](const httplib::Request&, httplib::Response& res) {
        nlohmann::json body = {{"status", health_status_}};
        res.set_content(body.dump(), "application/json");
    });

    server.Get("/metrics", [this](const httplib::Request&, httplib::Response& res) {
        auto uptime = std::chrono::duration_cast<std::chrono::seconds>(
            std::chrono::steady_clock::now() - start_time_).count();
        nlohmann::json body = {
            {"uptime_seconds", uptime},
            {"pull_count", pull_count_.load()}
        };
        res.set_content(body.dump(), "application/json");
    });

    server.Get("/internal-error", [](const httplib::Request&, httplib::Response&) {
        throw std::runtime_error("boom");
    });
}

}  // namespace ollama_node
