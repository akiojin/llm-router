#include "utils/logger.h"

#include <algorithm>
#include <spdlog/sinks/basic_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>
#include <spdlog/sinks/rotating_file_sink.h>

namespace ollama_node::logger {

spdlog::level::level_enum parse_level(const std::string& level_text) {
    std::string lower = level_text;
    std::transform(lower.begin(), lower.end(), lower.begin(), [](unsigned char c) { return std::tolower(c); });
    if (lower == "trace") return spdlog::level::trace;
    if (lower == "debug") return spdlog::level::debug;
    if (lower == "info") return spdlog::level::info;
    if (lower == "warn" || lower == "warning") return spdlog::level::warn;
    if (lower == "error") return spdlog::level::err;
    if (lower == "critical" || lower == "fatal") return spdlog::level::critical;
    if (lower == "off") return spdlog::level::off;
    return spdlog::level::info;
}

void init(const std::string& level,
          const std::string& pattern,
          const std::string& file_path,
          std::vector<spdlog::sink_ptr> additional_sinks) {
    std::vector<spdlog::sink_ptr> sinks = std::move(additional_sinks);

    if (sinks.empty()) {
        sinks.push_back(std::make_shared<spdlog::sinks::stdout_color_sink_mt>());
    }
    if (!file_path.empty()) {
        sinks.push_back(std::make_shared<spdlog::sinks::basic_file_sink_mt>(file_path, true));
    }

    auto logger = std::make_shared<spdlog::logger>("ollama-node", sinks.begin(), sinks.end());
    spdlog::set_default_logger(logger);

    spdlog::set_pattern(pattern);
    spdlog::set_level(parse_level(level));
    spdlog::flush_on(spdlog::level::info);
}

void init_from_env() {
    std::string level = "info";
    if (const char* env = std::getenv("LOG_LEVEL")) level = env;

    std::string file_path;
    if (const char* env = std::getenv("LOG_FILE")) file_path = env;

    bool json = false;
    if (const char* env = std::getenv("LOG_FORMAT")) {
        std::string fmt = env;
        std::transform(fmt.begin(), fmt.end(), fmt.begin(), ::tolower);
        if (fmt == "json") json = true;
    }

    size_t max_size = 10 * 1024 * 1024;
    size_t max_files = 3;
    if (const char* env = std::getenv("LOG_MAX_SIZE_MB")) {
        try {
            auto mb = std::stoll(env);
            if (mb > 0 && mb < 1024) max_size = static_cast<size_t>(mb) * 1024 * 1024;
        } catch (...) {}
    }
    if (const char* env = std::getenv("LOG_MAX_FILES")) {
        try {
            auto n = std::stoll(env);
            if (n > 0 && n < 50) max_files = static_cast<size_t>(n);
        } catch (...) {}
    }

    std::string pattern = "[%Y-%m-%d %T.%e] [%l] %v";
    if (json) {
        pattern = R"({"ts":"%Y-%m-%dT%H:%M:%S.%e","level":"%l","msg":"%v"})";
    }

    std::vector<spdlog::sink_ptr> sinks;
    sinks.push_back(std::make_shared<spdlog::sinks::stdout_color_sink_mt>());
    if (!file_path.empty()) {
        auto sink = std::make_shared<spdlog::sinks::rotating_file_sink_mt>(file_path, max_size, max_files);
        sinks.push_back(sink);
    }

    init(level, pattern, "", sinks);
}

}  // namespace ollama_node::logger
