#include "utils/logger.h"

#include <algorithm>
#include <chrono>
#include <ctime>
#include <filesystem>
#include <iomanip>
#include <sstream>
#include <spdlog/sinks/basic_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>

namespace fs = std::filesystem;

namespace llm_node::logger {

namespace {
    constexpr const char* LOG_FILE_BASE = "llm-node.jsonl";
    constexpr const char* DEFAULT_DATA_DIR = ".llm-router";
    constexpr const char* LOG_SUBDIR = "logs";
    constexpr int DEFAULT_RETENTION_DAYS = 7;

    // New environment variable names (LLM_NODE_* prefix)
    constexpr const char* LLM_NODE_LOG_DIR_ENV = "LLM_NODE_LOG_DIR";
    constexpr const char* LLM_NODE_LOG_LEVEL_ENV = "LLM_NODE_LOG_LEVEL";
    constexpr const char* LLM_NODE_LOG_RETENTION_DAYS_ENV = "LLM_NODE_LOG_RETENTION_DAYS";

    // Deprecated environment variable names (fallback)
    constexpr const char* LEGACY_LOG_DIR_ENV = "LLM_LOG_DIR";
    constexpr const char* LEGACY_LOG_LEVEL_ENV = "LLM_LOG_LEVEL";
    constexpr const char* LEGACY_LOG_RETENTION_DAYS_ENV = "LLM_LOG_RETENTION_DAYS";
    constexpr const char* LEGACY_LEVEL_ENV = "LOG_LEVEL";

    std::string get_today_date() {
        auto now = std::chrono::system_clock::now();
        auto time_t_now = std::chrono::system_clock::to_time_t(now);
        std::tm tm_now{};
        localtime_r(&time_t_now, &tm_now);
        std::ostringstream oss;
        oss << std::put_time(&tm_now, "%Y-%m-%d");
        return oss.str();
    }

    std::string get_home_dir() {
        if (const char* home = std::getenv("HOME")) {
            return home;
        }
        if (const char* userprofile = std::getenv("USERPROFILE")) {
            return userprofile;
        }
        return "/tmp";
    }
}  // namespace

spdlog::level::level_enum parse_level(const std::string& level_text) {
    std::string lower = level_text;
    std::transform(lower.begin(), lower.end(), lower.begin(),
                   [](unsigned char c) { return std::tolower(c); });
    if (lower == "trace") return spdlog::level::trace;
    if (lower == "debug") return spdlog::level::debug;
    if (lower == "info") return spdlog::level::info;
    if (lower == "warn" || lower == "warning") return spdlog::level::warn;
    if (lower == "error") return spdlog::level::err;
    if (lower == "critical" || lower == "fatal") return spdlog::level::critical;
    if (lower == "off") return spdlog::level::off;
    return spdlog::level::info;
}

std::string get_log_dir() {
    // New env var takes priority
    if (const char* env = std::getenv(LLM_NODE_LOG_DIR_ENV)) {
        return env;
    }
    // Fallback to deprecated env var
    if (const char* env = std::getenv(LEGACY_LOG_DIR_ENV)) {
        // Note: Can't log warning here because logger not initialized yet
        return env;
    }

    // Default: ~/.llm-router/logs
    return (fs::path(get_home_dir()) / DEFAULT_DATA_DIR / LOG_SUBDIR).string();
}

std::string get_log_file_path() {
    std::string today = get_today_date();
    std::string filename = std::string(LOG_FILE_BASE) + "." + today;
    return (fs::path(get_log_dir()) / filename).string();
}

int get_retention_days() {
    // New env var takes priority
    if (const char* env = std::getenv(LLM_NODE_LOG_RETENTION_DAYS_ENV)) {
        try {
            int days = std::stoi(env);
            if (days > 0 && days < 365) {
                return days;
            }
        } catch (...) {}
    }
    // Fallback to deprecated env var
    if (const char* env = std::getenv(LEGACY_LOG_RETENTION_DAYS_ENV)) {
        try {
            int days = std::stoi(env);
            if (days > 0 && days < 365) {
                return days;
            }
        } catch (...) {}
    }
    return DEFAULT_RETENTION_DAYS;
}

void cleanup_old_logs(const std::string& log_dir, int retention_days) {
    if (!fs::exists(log_dir)) {
        return;
    }

    auto now = std::chrono::system_clock::now();
    auto cutoff = now - std::chrono::hours(24 * retention_days);
    auto cutoff_time_t = std::chrono::system_clock::to_time_t(cutoff);
    std::tm cutoff_tm{};
    localtime_r(&cutoff_time_t, &cutoff_tm);
    std::ostringstream oss;
    oss << std::put_time(&cutoff_tm, "%Y-%m-%d");
    std::string cutoff_str = oss.str();

    std::string prefix = std::string(LOG_FILE_BASE) + ".";

    for (const auto& entry : fs::directory_iterator(log_dir)) {
        if (!entry.is_regular_file()) continue;

        std::string filename = entry.path().filename().string();
        if (filename.rfind(prefix, 0) != 0) continue;

        std::string date_part = filename.substr(prefix.length());
        if (date_part < cutoff_str) {
            std::error_code ec;
            fs::remove(entry.path(), ec);
        }
    }
}

void init(const std::string& level,
          const std::string& pattern,
          const std::string& file_path,
          std::vector<spdlog::sink_ptr> additional_sinks) {
    std::vector<spdlog::sink_ptr> sinks = std::move(additional_sinks);

    // File sink only (no stdout)
    if (!file_path.empty() && sinks.empty()) {
        sinks.push_back(
            std::make_shared<spdlog::sinks::basic_file_sink_mt>(file_path, false));
    }

    if (sinks.empty()) {
        // Fallback: if no file path and no sinks, create file sink with default
        std::string default_path = get_log_file_path();
        fs::create_directories(fs::path(default_path).parent_path());
        sinks.push_back(
            std::make_shared<spdlog::sinks::basic_file_sink_mt>(default_path, false));
    }

    auto logger = std::make_shared<spdlog::logger>("llm-node", sinks.begin(), sinks.end());
    spdlog::set_default_logger(logger);

    spdlog::set_pattern(pattern);
    spdlog::set_level(parse_level(level));
    spdlog::flush_on(spdlog::level::info);
}

void init_from_env() {
    // Get log level (priority: LLM_NODE_LOG_LEVEL > LLM_LOG_LEVEL > LOG_LEVEL)
    std::string level = "info";
    if (const char* env = std::getenv(LLM_NODE_LOG_LEVEL_ENV)) {
        level = env;
    } else if (const char* env = std::getenv(LEGACY_LOG_LEVEL_ENV)) {
        level = env;
    } else if (const char* env = std::getenv(LEGACY_LEVEL_ENV)) {
        level = env;
    }

    // Get log directory and create it
    std::string log_dir = get_log_dir();
    fs::create_directories(log_dir);

    // Cleanup old logs
    int retention_days = get_retention_days();
    cleanup_old_logs(log_dir, retention_days);

    // Get today's log file path
    std::string log_path = get_log_file_path();

    // Create sinks: stdout + file
    std::vector<spdlog::sink_ptr> sinks;

    // Stdout sink (human-readable format)
    auto stdout_sink = std::make_shared<spdlog::sinks::stdout_color_sink_mt>();
    stdout_sink->set_pattern("[%Y-%m-%d %T.%e] [%l] %v");
    sinks.push_back(stdout_sink);

    // File sink (JSON format for structured logging)
    auto file_sink = std::make_shared<spdlog::sinks::basic_file_sink_mt>(log_path, false);
    file_sink->set_pattern(R"({"ts":"%Y-%m-%dT%H:%M:%S.%e","level":"%l","msg":"%v"})");
    sinks.push_back(file_sink);

    // Use stdout pattern as default (file sink has its own pattern)
    std::string pattern = "[%Y-%m-%d %T.%e] [%l] %v";

    init(level, pattern, "", sinks);

    spdlog::info("Node logs initialized: {}", log_path);
}

}  // namespace llm_node::logger
