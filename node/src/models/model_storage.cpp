// SPEC-dcaeaec4: ModelStorage implementation
// Simple model file management without LLM runtime dependency
#include "models/model_storage.h"

#include <filesystem>
#include <fstream>
#include <algorithm>
#include <spdlog/spdlog.h>

namespace fs = std::filesystem;
using json = nlohmann::json;

namespace llm_node {

ModelStorage::ModelStorage(std::string models_dir) : models_dir_(std::move(models_dir)) {}

std::string ModelStorage::modelNameToDir(const std::string& model_name) {
    if (model_name.empty()) {
        return "_latest";
    }

    std::string result = model_name;

    // Replace all colons with underscores
    std::replace(result.begin(), result.end(), ':', '_');

    // If no tag was present (no colon), append _latest
    if (model_name.find(':') == std::string::npos) {
        result += "_latest";
    }

    return result;
}

std::string ModelStorage::dirNameToModel(const std::string& dir_name) {
    std::string result = dir_name;

    // Find the last underscore and replace with colon
    auto last_underscore = result.rfind('_');
    if (last_underscore != std::string::npos) {
        result[last_underscore] = ':';
    }

    return result;
}

std::string ModelStorage::resolveGguf(const std::string& model_name) const {
    const std::string dir_name = modelNameToDir(model_name);
    const auto gguf_path = fs::path(models_dir_) / dir_name / "model.gguf";

    spdlog::debug("ModelStorage::resolveGguf: model={}, dir={}, path={}, exists={}",
        model_name, dir_name, gguf_path.string(), fs::exists(gguf_path));

    if (fs::exists(gguf_path)) {
        return gguf_path.string();
    }

    return "";
}

std::vector<ModelInfo> ModelStorage::listAvailable() const {
    std::vector<ModelInfo> out;

    if (!fs::exists(models_dir_)) {
        spdlog::debug("ModelStorage::listAvailable: models_dir does not exist: {}", models_dir_);
        return out;
    }

    for (const auto& entry : fs::directory_iterator(models_dir_)) {
        if (!entry.is_directory()) continue;

        const auto dir_name = entry.path().filename().string();
        const auto gguf_path = entry.path() / "model.gguf";

        if (!fs::exists(gguf_path)) {
            spdlog::debug("ModelStorage::listAvailable: skipping {} (no model.gguf)", dir_name);
            continue;
        }

        ModelInfo info;
        info.name = dirNameToModel(dir_name);
        info.gguf_path = gguf_path.string();
        info.valid = true;

        out.push_back(std::move(info));
    }

    spdlog::debug("ModelStorage::listAvailable: found {} models", out.size());
    return out;
}

std::optional<nlohmann::json> ModelStorage::loadMetadata(const std::string& model_name) const {
    const std::string dir_name = modelNameToDir(model_name);
    const auto metadata_path = fs::path(models_dir_) / dir_name / "metadata.json";

    if (!fs::exists(metadata_path)) {
        return std::nullopt;
    }

    try {
        std::ifstream ifs(metadata_path);
        json j = json::parse(ifs);
        return j;
    } catch (const std::exception& e) {
        spdlog::warn("ModelStorage::loadMetadata: failed to parse {}: {}", metadata_path.string(), e.what());
        return std::nullopt;
    }
}

bool ModelStorage::validateModel(const std::string& model_name) const {
    const std::string dir_name = modelNameToDir(model_name);
    const auto gguf_path = fs::path(models_dir_) / dir_name / "model.gguf";

    return fs::exists(gguf_path) && fs::is_regular_file(gguf_path);
}

}  // namespace llm_node
