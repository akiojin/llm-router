// logger.h - lightweight logging wrapper around spdlog
#pragma once

#include <spdlog/spdlog.h>
#include <string>
#include <vector>

namespace ollama_node::logger {

// Convert textual level to spdlog level (case-insensitive). Unknown -> info.
spdlog::level::level_enum parse_level(const std::string& level_text);

// Initialize default logger with optional pattern and file sink.
// additional_sinks is mainly for testing (e.g., ostream sink injection).
void init(const std::string& level = "info",
          const std::string& pattern = "[%Y-%m-%d %T.%e] [%l] %v",
          const std::string& file_path = "",
          std::vector<spdlog::sink_ptr> additional_sinks = {});

}  // namespace ollama_node::logger
