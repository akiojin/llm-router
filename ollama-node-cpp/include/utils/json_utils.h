// json_utils.h - helpers for safe JSON parsing and extraction
#pragma once

#include <nlohmann/json.hpp>
#include <optional>
#include <string>
#include <vector>

namespace ollama_node {

// Parse JSON string; returns std::nullopt on error and fills error message if provided.
std::optional<nlohmann::json> parse_json(const std::string& body, std::string* error = nullptr);

// Get value if present and convertible; otherwise fallback is returned.
template <typename T>
T get_or(const nlohmann::json& j, const std::string& key, const T& fallback) {
    if (!j.contains(key)) return fallback;
    try {
        return j.at(key).get<T>();
    } catch (...) {
        return fallback;
    }
}

// Check that all required keys exist; returns true if all present.
// missing_key receives the first missing key when provided.
bool has_required_keys(const nlohmann::json& j,
                       const std::vector<std::string>& keys,
                       std::string* missing_key = nullptr);

// Compact JSON to string (no indentation).
std::string json_to_string(const nlohmann::json& j);

}  // namespace ollama_node
