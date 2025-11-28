#include "utils/config.h"
#include <cstdlib>
#include <filesystem>
#include <optional>
#include <algorithm>
#include <cctype>
#include <fstream>
#include <nlohmann/json.hpp>
#include <sstream>
#include "utils/file_lock.h"

namespace ollama_node {

DownloadConfig loadDownloadConfig() {
    DownloadConfig cfg;

    auto info = loadDownloadConfigWithLog();
    return info.first;
}

std::pair<DownloadConfig, std::string> loadDownloadConfigWithLog() {
    DownloadConfig cfg;
    std::ostringstream log;
    bool used_file = false;
    bool used_env = false;

    // Optional JSON config file: path from LLM_DL_CONFIG or ~/.ollama/config.json
    auto load_from_file = [&](const std::filesystem::path& path) {
        if (!std::filesystem::exists(path)) return false;
        try {
            FileLock lock(path);
            std::ifstream ifs(path);
            if (!ifs.is_open()) return false;

            nlohmann::json j;
            ifs >> j;
            if (j.contains("max_retries")) cfg.max_retries = j.value("max_retries", cfg.max_retries);
            if (j.contains("backoff_ms")) cfg.backoff = std::chrono::milliseconds(j.value("backoff_ms", cfg.backoff.count()));
            if (j.contains("concurrency")) cfg.max_concurrency = j.value("concurrency", cfg.max_concurrency);
            if (j.contains("max_bps")) cfg.max_bytes_per_sec = j.value("max_bps", cfg.max_bytes_per_sec);
            if (j.contains("chunk")) cfg.chunk_size = j.value("chunk", cfg.chunk_size);
            log << "file=" << path << " ";
            return true;
        } catch (...) {
            return false;
        }
    };

    if (const char* env = std::getenv("LLM_DL_CONFIG")) {
        if (load_from_file(env)) {
            used_file = true;
        }
    } else {
        try {
            std::filesystem::path home = std::getenv("HOME") ? std::getenv("HOME") : "";
            auto path = home / std::filesystem::path(".ollama/config.json");
            if (load_from_file(path)) {
                used_file = true;
            }
        } catch (...) {}
    }

    if (const char* env = std::getenv("LLM_DL_MAX_RETRIES")) {
        try {
            int v = std::stoi(env);
            if (v >= 0) cfg.max_retries = v;
            log << "env:MAX_RETRIES=" << v << " ";
            used_env = true;
        } catch (...) {}
    }

    if (const char* env = std::getenv("LLM_DL_BACKOFF_MS")) {
        try {
            long long ms = std::stoll(env);
            if (ms >= 0) cfg.backoff = std::chrono::milliseconds(ms);
            log << "env:BACKOFF_MS=" << ms << " ";
            used_env = true;
        } catch (...) {}
    }

    if (const char* env = std::getenv("LLM_DL_CONCURRENCY")) {
        try {
            long long v = std::stoll(env);
            if (v > 0 && v < 64) cfg.max_concurrency = static_cast<size_t>(v);
            log << "env:CONCURRENCY=" << v << " ";
            used_env = true;
        } catch (...) {}
    }

    if (const char* env = std::getenv("LLM_DL_MAX_BPS")) {
        try {
            long long v = std::stoll(env);
            if (v > 0) cfg.max_bytes_per_sec = static_cast<size_t>(v);
            log << "env:MAX_BPS=" << v << " ";
            used_env = true;
        } catch (...) {}
    }

    if (const char* env = std::getenv("LLM_DL_CHUNK")) {
        try {
            long long v = std::stoll(env);
            if (v > 0 && v <= 1 << 20) cfg.chunk_size = static_cast<size_t>(v);
            log << "env:CHUNK=" << v << " ";
            used_env = true;
        } catch (...) {}
    }

    if (log.tellp() > 0) log << "|";
    log << "sources=";
    if (used_env) log << "env";
    if (used_file) {
        if (used_env) log << ",";
        log << "file";
    }
    if (!used_env && !used_file) log << "default";

    return {cfg, log.str()};
}

namespace {

std::filesystem::path defaultConfigPath() {
    try {
        std::filesystem::path home = std::getenv("HOME") ? std::getenv("HOME") : "";
        if (!home.empty()) return home / ".ollama/config.json";
    } catch (...) {
    }
    return std::filesystem::path();
}

bool readJsonWithLock(const std::filesystem::path& path, nlohmann::json& out) {
    if (!std::filesystem::exists(path)) return false;
    FileLock lock(path);
    if (!lock.locked()) return false;
    try {
        std::ifstream ifs(path);
        if (!ifs.is_open()) return false;
        ifs >> out;
        return true;
    } catch (...) {
        return false;
    }
}

}  // namespace

std::pair<NodeConfig, std::string> loadNodeConfigWithLog() {
    NodeConfig cfg;
    cfg.bind_address = "0.0.0.0";
    std::ostringstream log;
    bool used_env = false;
    bool used_file = false;

    // defaults
    cfg.models_dir = defaultConfigPath().empty() ? ".ollama/models" : (defaultConfigPath().parent_path() / "models").string();

    auto apply_json = [&](const nlohmann::json& j) {
        if (j.contains("router_url") && j["router_url"].is_string()) cfg.router_url = j["router_url"].get<std::string>();
        if (j.contains("models_dir") && j["models_dir"].is_string()) cfg.models_dir = j["models_dir"].get<std::string>();
        if (j.contains("node_port") && j["node_port"].is_number()) cfg.node_port = j["node_port"].get<int>();
        if (j.contains("heartbeat_interval_sec") && j["heartbeat_interval_sec"].is_number()) {
            cfg.heartbeat_interval_sec = j["heartbeat_interval_sec"].get<int>();
        }
        if (j.contains("require_gpu") && j["require_gpu"].is_boolean()) cfg.require_gpu = j["require_gpu"].get<bool>();
        if (j.contains("bind_address") && j["bind_address"].is_string()) cfg.bind_address = j["bind_address"].get<std::string>();
    };

    // file
    std::filesystem::path cfg_path;
    if (const char* env = std::getenv("LLM_NODE_CONFIG")) {
        cfg_path = env;
    } else {
        cfg_path = defaultConfigPath();
    }

    if (!cfg_path.empty()) {
        nlohmann::json j;
        if (readJsonWithLock(cfg_path, j)) {
            apply_json(j);
            log << "file=" << cfg_path << " ";
            used_file = true;
        }
    }

    // env overrides
    auto getenv_str = [](const char* key) -> std::optional<std::string> {
        if (const char* v = std::getenv(key)) return std::string(v);
        return std::nullopt;
    };

    if (auto v = getenv_str("LLM_ROUTER_URL")) {
        cfg.router_url = *v;
        log << "env:ROUTER_URL=" << *v << " ";
        used_env = true;
    }
    if (auto v = getenv_str("LLM_MODELS_DIR")) {
        cfg.models_dir = *v;
        log << "env:MODELS_DIR=" << *v << " ";
        used_env = true;
    }
    if (auto v = getenv_str("LLM_NODE_PORT")) {
        try {
            cfg.node_port = std::stoi(*v);
            log << "env:NODE_PORT=" << cfg.node_port << " ";
            used_env = true;
        } catch (...) {}
    }
    if (auto v = getenv_str("LLM_HEARTBEAT_SECS")) {
        try {
            cfg.heartbeat_interval_sec = std::stoi(*v);
            log << "env:HEARTBEAT_SECS=" << cfg.heartbeat_interval_sec << " ";
            used_env = true;
        } catch (...) {}
    }
    if (auto v = getenv_str("LLM_ALLOW_NO_GPU")) {
        std::string s = *v;
        std::transform(s.begin(), s.end(), s.begin(), ::tolower);
        if (s == "1" || s == "true" || s == "yes") {
            cfg.require_gpu = false;
            log << "env:ALLOW_NO_GPU=1 ";
            used_env = true;
        }
    }

    if (auto v = getenv_str("LLM_BIND_ADDRESS")) {
        cfg.bind_address = *v;
        log << "env:BIND_ADDRESS=" << *v << " ";
        used_env = true;
    }

    if (auto v = getenv_str("LLM_NODE_IP")) {
        cfg.ip_address = *v;
        log << "env:NODE_IP=" << *v << " ";
        used_env = true;
    }

    if (auto v = getenv_str("LLM_AUTO_REPAIR")) {
        std::string s = *v;
        std::transform(s.begin(), s.end(), s.begin(), ::tolower);
        if (s == "1" || s == "true" || s == "yes") {
            cfg.auto_repair = true;
            log << "env:AUTO_REPAIR=1 ";
            used_env = true;
        }
    }

    if (auto v = getenv_str("LLM_REPAIR_TIMEOUT_SECS")) {
        try {
            cfg.repair_timeout_secs = std::stoi(*v);
            log << "env:REPAIR_TIMEOUT_SECS=" << cfg.repair_timeout_secs << " ";
            used_env = true;
        } catch (...) {}
    }

    if (log.tellp() > 0) log << "|";
    log << "sources=";
    if (used_env) log << "env";
    if (used_file) {
        if (used_env) log << ",";
        log << "file";
    }
    if (!used_env && !used_file) log << "default";

    return {cfg, log.str()};
}

NodeConfig loadNodeConfig() {
    auto info = loadNodeConfigWithLog();
    return info.first;
}

}  // namespace ollama_node
