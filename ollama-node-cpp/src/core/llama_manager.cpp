#include "core/llama_manager.h"

#include <filesystem>
#include <utility>
#include <vector>

namespace fs = std::filesystem;

namespace ollama_node {

LlamaManager::LlamaManager(std::string models_dir) : models_dir_(std::move(models_dir)) {}

bool LlamaManager::loadModel(const std::string& model_path) {
    fs::path path = model_path;
    if (path.is_relative()) path = fs::path(models_dir_) / path;
    if (path.extension() != ".gguf") return false;
    if (!fs::exists(path)) return false;
    auto canonical = fs::weakly_canonical(path).string();
    if (loaded_.insert(canonical).second) {
        // 仮のメモリ使用量: 512MB 加算（重複ロードを避ける）
        memory_bytes_ += 512ull * 1024ull * 1024ull;
    }
    return true;
}

std::shared_ptr<LlamaContext> LlamaManager::createContext(const std::string& model) const {
    fs::path path = model;
    if (path.is_relative()) path = fs::path(models_dir_) / path;
    auto canonical = fs::weakly_canonical(path).string();
    if (loaded_.count(canonical) == 0) return nullptr;
    auto ctx = std::make_shared<LlamaContext>();
    ctx->model = canonical;
    ctx->gpu_layers = gpu_layers_;
    return ctx;
}

size_t LlamaManager::loadedCount() const {
    return loaded_.size();
}

void LlamaManager::setGpuLayerSplit(size_t layers) {
    gpu_layers_ = layers;
}

size_t LlamaManager::getGpuLayerSplit() const {
    return gpu_layers_;
}

size_t LlamaManager::memoryUsageBytes() const {
    return memory_bytes_;
}

bool LlamaManager::unloadModel(const std::string& model_path) {
    fs::path path = model_path;
    if (path.is_relative()) path = fs::path(models_dir_) / path;
    auto canonical = fs::weakly_canonical(path).string();
    auto it = loaded_.find(canonical);
    if (it == loaded_.end()) return false;
    loaded_.erase(it);
    if (memory_bytes_ >= 512ull * 1024ull * 1024ull) {
        memory_bytes_ -= 512ull * 1024ull * 1024ull;
    } else {
        memory_bytes_ = 0;
    }
    return true;
}

std::vector<std::string> LlamaManager::getLoadedModels() const {
    return std::vector<std::string>(loaded_.begin(), loaded_.end());
}

}  // namespace ollama_node
