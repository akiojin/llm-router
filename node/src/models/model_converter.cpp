#include "models/model_converter.h"

#include <fstream>

namespace fs = std::filesystem;

namespace llm_node {

ModelConverter::ModelConverter(std::string workspace_dir) : workspace_(std::move(workspace_dir)) {}

fs::path ModelConverter::ggufPath(const std::string& model_name) const {
    return workspace_ / model_name / (model_name + ".gguf");
}

std::string ModelConverter::ensureDir(const fs::path& p) const {
    fs::create_directories(p.parent_path());
    return p.string();
}

std::string ModelConverter::convertPyTorchToGguf(const fs::path& src_bin, const std::string& model_name) {
    return writePlaceholder(src_bin, model_name);
}

std::string ModelConverter::convertSafetensorsToGguf(const fs::path& src_st, const std::string& model_name) {
    return writePlaceholder(src_st, model_name);
}

bool ModelConverter::isConverted(const std::string& model_name) const {
    if (auto it = cache_.find(model_name); it != cache_.end()) {
        return fs::exists(it->second);
    }
    auto dest = ggufPath(model_name);
    return fs::exists(dest);
}

void ModelConverter::setCache(const std::unordered_map<std::string, std::string>& cache) {
    cache_ = cache;
}

std::unordered_map<std::string, std::string> ModelConverter::getCache() const {
    return cache_;
}

void ModelConverter::setProgressCallback(ProgressFn cb) {
    progress_cb_ = std::move(cb);
}

std::string ModelConverter::writePlaceholder(const fs::path& src, const std::string& model_name) {
    auto dest = ggufPath(model_name);
    ensureDir(dest);
    std::ofstream ofs(dest, std::ios::binary | std::ios::trunc);
    if (!ofs.is_open()) return "";
    ofs << "GGUF from " << src.filename().string();
    cache_[model_name] = dest.string();
    if (progress_cb_) progress_cb_(model_name, 1.0);
    return dest.string();
}

}  // namespace llm_node
