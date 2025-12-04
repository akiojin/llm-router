// SPEC-dcaeaec4: ModelStorage - Simple model file management
// Replaces legacy compat layer with simpler directory structure:
// ~/.llm-router/models/<model_name>/model.gguf
#pragma once

#include <string>
#include <vector>
#include <optional>
#include <nlohmann/json.hpp>

namespace llm_node {

struct ModelInfo {
    std::string name;       // Model name (e.g., "gpt-oss:20b")
    std::string gguf_path;  // Full path to model.gguf
    bool valid{false};      // Whether the model file exists and is valid
};

class ModelStorage {
public:
    explicit ModelStorage(std::string models_dir);

    // FR-2: Convert model name to directory name (colon to underscore)
    // e.g., "gpt-oss:20b" -> "gpt-oss_20b"
    static std::string modelNameToDir(const std::string& model_name);

    // Reverse conversion: directory name to model name
    // e.g., "gpt-oss_20b" -> "gpt-oss:20b"
    static std::string dirNameToModel(const std::string& dir_name);

    // FR-3: Resolve GGUF file path for a model
    // Returns empty string if model not found
    std::string resolveGguf(const std::string& model_name) const;

    // FR-4: List all available models
    std::vector<ModelInfo> listAvailable() const;

    // FR-5: Load optional metadata from metadata.json
    std::optional<nlohmann::json> loadMetadata(const std::string& model_name) const;

    // Validate model (check if model.gguf exists)
    bool validateModel(const std::string& model_name) const;

private:
    std::string models_dir_;
};


}  // namespace llm_node
