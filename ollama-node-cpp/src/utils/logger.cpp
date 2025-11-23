#include "utils/logger.h"

#include <algorithm>
#include <spdlog/sinks/basic_file_sink.h>
#include <spdlog/sinks/stdout_color_sinks.h>

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

}  // namespace ollama_node::logger
