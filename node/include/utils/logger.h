// logger.h - lightweight logging wrapper around spdlog
#pragma once

#include <spdlog/spdlog.h>
#include <string>
#include <vector>

namespace ollama_node::logger {

// Convert textual level to spdlog level (case-insensitive). Unknown -> info.
spdlog::level::level_enum parse_level(const std::string& level_text);

// Get the log directory path (~/.llm-router/logs by default).
std::string get_log_dir();

// Get today's log file path (llm-node.jsonl.YYYY-MM-DD).
std::string get_log_file_path();

// Get retention days from environment (default: 7).
int get_retention_days();

// Cleanup old log files older than retention_days.
void cleanup_old_logs(const std::string& log_dir, int retention_days);

// Initialize default logger with optional pattern and file sink.
// additional_sinks is mainly for testing (e.g., ostream sink injection).
void init(const std::string& level = "info",
          const std::string& pattern = "[%Y-%m-%d %T.%e] [%l] %v",
          const std::string& file_path = "",
          std::vector<spdlog::sink_ptr> additional_sinks = {});

// Initialize using environment variables:
// LLM_LOG_DIR (log directory, default: ~/.llm-router/logs)
// LLM_LOG_LEVEL (trace|debug|info|warn|error|critical|off)
// LLM_LOG_RETENTION_DAYS (retention days, default: 7)
// Legacy: LOG_LEVEL, LOG_FILE are still supported for backward compatibility.
void init_from_env();

}  // namespace ollama_node::logger
