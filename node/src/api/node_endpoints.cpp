#include "api/node_endpoints.h"

#include <stdexcept>
#include <thread>
#include <filesystem>
#include <fstream>
#include <nlohmann/json.hpp>
#include "runtime/state.h"
#include "utils/logger.h"
#include "models/model_sync.h"
#include "models/model_downloader.h"
#include "api/router_client.h"

namespace ollama_node {

NodeEndpoints::NodeEndpoints() : health_status_("ok") {}

void NodeEndpoints::registerRoutes(httplib::Server& server) {
    start_time_ = std::chrono::steady_clock::now();

    server.Post("/pull", [this](const httplib::Request& req, httplib::Response& res) {
        pull_count_.fetch_add(1);
        exporter_.inc_counter("ollama_node_pull_total", 1.0, "Number of pull requests received");

        // Parse request JSON
        auto j = nlohmann::json::parse(req.body, nullptr, false);
        if (j.is_discarded() || !j.contains("model")) {
            res.status = 400;
            res.set_content(R"({"error":"model required"})", "application/json");
            return;
        }

        std::string model_name = j["model"].get<std::string>();
        std::string task_id = j.value("task_id", "");

        spdlog::info("Pull request received: model={}, task_id={}", model_name, task_id);

        // Return accepted immediately, process in background
        nlohmann::json body = {{"status", "accepted"}};
        res.set_content(body.dump(), "application/json");

        // Process download in background thread
        if (model_sync_ && router_client_ && !task_id.empty()) {
            auto sync = model_sync_;
            auto client = router_client_;

            // optional fields from request
            std::string path = j.value("path", "");
            std::string download_url = j.value("download_url", "");
            std::string chat_template = j.value("chat_template", "");

            // helper: model name -> dir (colon to underscore, append _latest when tagなし)
            auto modelNameToDir = [](const std::string& name) {
                std::string dir = name;
                std::replace(dir.begin(), dir.end(), ':', '_');
                if (name.find(':') == std::string::npos) {
                    dir += "_latest";
                }
                return dir;
            };

            std::thread([sync, client, model_name, task_id, path, download_url, chat_template, modelNameToDir]() {
                spdlog::info("Starting model pull: model={}, task_id={}, path='{}', download_url='{}'",
                              model_name, task_id, path, download_url);

                const std::string models_dir = sync->getModelsDir();
                const std::string dir_name = modelNameToDir(model_name);
                const std::filesystem::path target_dir = std::filesystem::path(models_dir) / dir_name;
                const std::filesystem::path target_path = target_dir / "model.gguf";

                bool success = false;

                auto progress_cb = [&client, &task_id](size_t downloaded, size_t total) {
                    if (total > 0) {
                        double progress = static_cast<double>(downloaded) / static_cast<double>(total);
                        client->reportProgress(task_id, progress, std::nullopt);
                    }
                };

                // 1) shared path copy
                if (!path.empty()) {
                    std::error_code ec;
                    if (std::filesystem::exists(path, ec) && std::filesystem::is_regular_file(path, ec)) {
                        std::filesystem::create_directories(target_dir, ec);
                        if (!ec) {
                            std::filesystem::copy_file(path, target_path, std::filesystem::copy_options::overwrite_existing, ec);
                            if (!ec) success = true;
                        }
                    }
                }

                // 2) download if needed
                if (!success && !download_url.empty()) {
                    ModelDownloader downloader(sync->getBaseUrl(), models_dir, std::chrono::milliseconds(30000));
                    std::string filename = dir_name + "/model.gguf";
                    auto out = downloader.downloadBlob(download_url, filename, progress_cb);
                    success = !out.empty();
                }

                if (success) {
                    // persist chat_template if provided
                    if (!chat_template.empty()) {
                        std::error_code ec;
                        std::filesystem::create_directories(target_dir, ec);
                        if (!ec) {
                            nlohmann::json meta;
                            meta["chat_template"] = chat_template;
                            std::ofstream ofs(target_dir / "metadata.json", std::ios::binary | std::ios::trunc);
                            ofs << meta.dump();
                        }
                    }
                    spdlog::info("Model pull complete: model={}, task_id={}", model_name, task_id);
                    client->reportProgress(task_id, 1.0, std::nullopt);
                } else {
                    spdlog::error("Model pull failed: model={}, task_id={}", model_name, task_id);
                }
            }).detach();
        } else {
            spdlog::warn("Pull request ignored: model_sync or router_client not set, or no task_id");
        }
    });

    server.Get("/health", [this](const httplib::Request&, httplib::Response& res) {
        nlohmann::json body = {{"status", health_status_}};
        res.set_content(body.dump(), "application/json");
    });

    server.Get("/startup", [](const httplib::Request&, httplib::Response& res) {
        if (ollama_node::is_ready()) {
            res.set_content(R"({"status":"ready"})", "application/json");
        } else {
            res.status = 503;
            res.set_content(R"({"status":"starting"})", "application/json");
        }
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
        exporter_.set_gauge("ollama_node_gpu_devices", static_cast<double>(gpu_devices_), "Detected GPU devices");
        exporter_.set_gauge("ollama_node_gpu_memory_bytes", static_cast<double>(gpu_total_mem_), "Total GPU memory bytes");
        exporter_.set_gauge("ollama_node_gpu_capability", gpu_capability_, "Aggregated GPU capability score");
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
