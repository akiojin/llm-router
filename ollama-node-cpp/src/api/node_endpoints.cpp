#include "api/node_endpoints.h"

#include <stdexcept>
#include <nlohmann/json.hpp>
#include "utils/logger.h"

namespace ollama_node {

NodeEndpoints::NodeEndpoints() : health_status_("ok") {}

void NodeEndpoints::registerRoutes(httplib::Server& server) {
    start_time_ = std::chrono::steady_clock::now();

    server.Post("/pull", [this](const httplib::Request&, httplib::Response& res) {
        pull_count_.fetch_add(1);
        exporter_.inc_counter("ollama_node_pull_total", 1.0, "Number of pull requests received");
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

    server.Get("/metrics/prom", [this](const httplib::Request&, httplib::Response& res) {
        auto uptime = std::chrono::duration_cast<std::chrono::seconds>(
            std::chrono::steady_clock::now() - start_time_).count();
        exporter_.set_gauge("ollama_node_uptime_seconds", static_cast<double>(uptime), "Node uptime in seconds");
        exporter_.set_gauge("ollama_node_pulls_total", static_cast<double>(pull_count_.load()), "Total pull requests served");
        res.set_content(exporter_.render(), "text/plain");
    });

    server.Get("/log/level", [](const httplib::Request&, httplib::Response& res) {
        nlohmann::json body = {{"level", spdlog::level::to_string_view(spdlog::get_level()).data()}};
        res.set_content(body.dump(), "application/json");
    });

    server.Post("/log/level", [](const httplib::Request& req, httplib::Response& res) {
        auto j = nlohmann::json::parse(req.body, nullptr, false);
        if (j.is_discarded() || !j.contains("level")) {
            res.status = 400;
            res.set_content(R"({"error":"level required"})", "application/json");
            return;
        }
        auto level_str = j["level"].get<std::string>();
        spdlog::set_level(logger::parse_level(level_str));
        nlohmann::json body = {{"level", spdlog::level::to_string_view(spdlog::get_level()).data()}};
        res.set_content(body.dump(), "application/json");
    });

    server.Get("/internal-error", [](const httplib::Request&, httplib::Response&) {
        throw std::runtime_error("boom");
    });
}

}  // namespace ollama_node
