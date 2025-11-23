#pragma once

#include <string>
#include <unordered_set>
#include <memory>
#include <vector>

namespace ollama_node {

struct LlamaContext {
    std::string model;
    size_t gpu_layers{0};
};

class LlamaManager {
public:
    explicit LlamaManager(std::string models_dir);

    // ダミー実装: モデルファイルが存在すればロード済みにする
    bool loadModel(const std::string& model_path);

    // コンテキスト生成（モデルがロード済みなら生成）
    std::shared_ptr<LlamaContext> createContext(const std::string& model) const;

    size_t loadedCount() const;

    // GPU/CPU レイヤー分割の設定
    void setGpuLayerSplit(size_t layers);
    size_t getGpuLayerSplit() const;

    // メモリ管理（ダミー数値）
    size_t memoryUsageBytes() const;

    // モデルのアンロード（存在すればメモリを減算）
    bool unloadModel(const std::string& model_path);

    // ロード済みモデルの一覧（フルパス）
    std::vector<std::string> getLoadedModels() const;

private:
    std::string models_dir_;
    std::unordered_set<std::string> loaded_;
    size_t gpu_layers_{0};
    size_t memory_bytes_{0};
};

}  // namespace ollama_node
