#pragma once

#include <string>
#include <unordered_map>
#include <unordered_set>
#include <memory>
#include <vector>
#include <mutex>
#include <chrono>
#include <optional>

// llama.cpp forward declarations
struct llama_model;
struct llama_context;

namespace ollama_node {

/// llama.cpp モデルとコンテキストを保持する構造体
struct LlamaContext {
    std::string model_path;
    llama_model* model{nullptr};
    llama_context* ctx{nullptr};
    size_t gpu_layers{0};

    // デストラクタでリソース解放
    ~LlamaContext();

    // コピー禁止
    LlamaContext() = default;
    LlamaContext(const LlamaContext&) = delete;
    LlamaContext& operator=(const LlamaContext&) = delete;

    // ムーブ許可
    LlamaContext(LlamaContext&& other) noexcept;
    LlamaContext& operator=(LlamaContext&& other) noexcept;
};

class LlamaManager {
public:
    explicit LlamaManager(std::string models_dir);
    ~LlamaManager();

    // llama.cpp バックエンド初期化/終了（main.cpp で1回呼び出し）
    static void initBackend();
    static void freeBackend();

    // モデルロード（llama.cpp API使用）
    bool loadModel(const std::string& model_path);

    // モデルがロード済みか確認
    bool isLoaded(const std::string& model_path) const;

    // コンテキスト取得（推論エンジンが使用）
    llama_context* getContext(const std::string& model_path) const;
    llama_model* getModel(const std::string& model_path) const;

    // コンテキスト生成（モデルがロード済みなら生成）- 旧APIとの互換性
    std::shared_ptr<LlamaContext> createContext(const std::string& model) const;

    size_t loadedCount() const;

    // GPU/CPU レイヤー分割の設定
    void setGpuLayerSplit(size_t layers);
    size_t getGpuLayerSplit() const;

    // メモリ管理（実際のモデルサイズ）
    size_t memoryUsageBytes() const;

    // モデルのアンロード
    bool unloadModel(const std::string& model_path);

    // ロード済みモデルの一覧（フルパス）
    std::vector<std::string> getLoadedModels() const;

    // オンデマンドロード: モデルが未ロードなら自動ロード
    bool loadModelIfNeeded(const std::string& model_path);

    // アイドルタイムアウト設定
    void setIdleTimeout(std::chrono::milliseconds timeout);
    std::chrono::milliseconds getIdleTimeout() const;

    // アイドルモデルのアンロード
    size_t unloadIdleModels();

    // 最大ロード数設定
    void setMaxLoadedModels(size_t max_models);
    size_t getMaxLoadedModels() const;

    // ロード可能かチェック（最大数未満か）
    bool canLoadMore() const;

    // メモリ制限設定
    void setMaxMemoryBytes(size_t max_bytes);
    size_t getMaxMemoryBytes() const;

    // 最終アクセス時刻取得
    std::optional<std::chrono::steady_clock::time_point> getLastAccessTime(
        const std::string& model_path) const;

    // LRU: 最も古くアクセスされたモデルを取得
    std::optional<std::string> getLeastRecentlyUsedModel() const;

private:
    std::string models_dir_;
    mutable std::mutex mutex_;
    std::unordered_map<std::string, std::unique_ptr<LlamaContext>> loaded_models_;
    size_t gpu_layers_{0};
    size_t memory_bytes_{0};

    // オンデマンドロード用の設定
    std::chrono::milliseconds idle_timeout_{std::chrono::minutes(5)};
    size_t max_loaded_models_{0};  // 0 = 無制限
    size_t max_memory_bytes_{0};   // 0 = 無制限

    // アクセス時刻追跡
    std::unordered_map<std::string, std::chrono::steady_clock::time_point> last_access_;

    // 正規化されたパスを取得
    std::string canonicalizePath(const std::string& path) const;

    // アクセス時刻を更新
    void updateAccessTime(const std::string& model_path);
};

}  // namespace ollama_node
