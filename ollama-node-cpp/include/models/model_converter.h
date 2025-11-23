#pragma once

#include <string>
#include <filesystem>
#include <unordered_map>
#include <functional>

namespace ollama_node {

class ModelConverter {
public:
    explicit ModelConverter(std::string workspace_dir);

    // PyTorch (.bin) -> GGUF (dummy conversion: copy/create placeholder)
    std::string convertPyTorchToGguf(const std::filesystem::path& src_bin,
                                     const std::string& model_name);

    // safetensors -> GGUF (dummy conversion)
    std::string convertSafetensorsToGguf(const std::filesystem::path& src_st,
                                         const std::string& model_name);

    // Check if GGUF already exists for model
    bool isConverted(const std::string& model_name) const;

    // 変換キャッシュ（model_name -> GGUF path）
    void setCache(const std::unordered_map<std::string, std::string>& cache);
    std::unordered_map<std::string, std::string> getCache() const;

    // 変換進捗コールバック
    using ProgressFn = std::function<void(const std::string& model, double progress)>;

    void setProgressCallback(ProgressFn cb);

private:
    std::filesystem::path workspace_;
    std::unordered_map<std::string, std::string> cache_;
    ProgressFn progress_cb_;
    std::filesystem::path ggufPath(const std::string& model_name) const;
    std::string ensureDir(const std::filesystem::path& p) const;
    std::string writePlaceholder(const std::filesystem::path& src, const std::string& model_name);
};

}  // namespace ollama_node
