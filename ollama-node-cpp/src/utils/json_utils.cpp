#include "utils/json_utils.h"

#include <sstream>

namespace ollama_node {

std::optional<nlohmann::json> parse_json(const std::string& body, std::string* error) {
    try {
        auto j = nlohmann::json::parse(body);
        return j;
    } catch (const std::exception& ex) {
        if (error) *error = ex.what();
        return std::nullopt;
    }
}

bool has_required_keys(const nlohmann::json& j,
                       const std::vector<std::string>& keys,
                       std::string* missing_key) {
    for (const auto& k : keys) {
        if (!j.contains(k)) {
            if (missing_key) *missing_key = k;
            return false;
        }
    }
    return true;
}

std::string json_to_string(const nlohmann::json& j) {
    return j.dump();
}

}  // namespace ollama_node
