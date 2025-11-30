#include "core/llama_manager.h"
#include "include/llama.h"

#include <spdlog/spdlog.h>
#include <filesystem>
#include <utility>

namespace fs = std::filesystem;

namespace ollama_node {

// LlamaContext デストラクタ: リソース解放
LlamaContext::~LlamaContext() {
    if (ctx) {
        llama_free(ctx);
        ctx = nullptr;
    }
    if (model) {
        llama_model_free(model);
        model = nullptr;
    }
}

// LlamaContext ムーブコンストラクタ
LlamaContext::LlamaContext(LlamaContext&& other) noexcept
    : model_path(std::move(other.model_path))
    , model(other.model)
    , ctx(other.ctx)
    , gpu_layers(other.gpu_layers) {
    other.model = nullptr;
    other.ctx = nullptr;
}

// LlamaContext ムーブ代入演算子
LlamaContext& LlamaContext::operator=(LlamaContext&& other) noexcept {
    if (this != &other) {
        // 既存リソースを解放
        if (ctx) llama_free(ctx);
        if (model) llama_model_free(model);

        model_path = std::move(other.model_path);
        model = other.model;
        ctx = other.ctx;
        gpu_layers = other.gpu_layers;

        other.model = nullptr;
        other.ctx = nullptr;
    }
    return *this;
}

// LlamaManager コンストラクタ
LlamaManager::LlamaManager(std::string models_dir)
    : models_dir_(std::move(models_dir)) {}

// LlamaManager デストラクタ
LlamaManager::~LlamaManager() {
    // loaded_models_ が自動的にクリーンアップされる（unique_ptr）
}

// バックエンド初期化（プログラム開始時に1回呼び出し）
void LlamaManager::initBackend() {
    spdlog::info("Initializing llama.cpp backend");
    llama_backend_init();
}

// バックエンド終了（プログラム終了時に1回呼び出し）
void LlamaManager::freeBackend() {
    spdlog::info("Freeing llama.cpp backend");
    llama_backend_free();
}

// パス正規化
std::string LlamaManager::canonicalizePath(const std::string& path) const {
    fs::path p = path;
    if (p.is_relative()) {
        p = fs::path(models_dir_) / p;
    }
    return fs::weakly_canonical(p).string();
}

// Ollamaのblobファイル名かどうかを判定
static bool isOllamaBlobFile(const std::string& filename) {
    // Ollama blob format: sha256-<64 hex chars>
    if (filename.length() < 7) return false;
    if (filename.substr(0, 7) != "sha256-") return false;
    // 残りが16進数文字のみかチェック
    for (size_t i = 7; i < filename.length(); ++i) {
        char c = filename[i];
        if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'))) {
            return false;
        }
    }
    return true;
}

// モデルロード（llama.cpp API使用）
bool LlamaManager::loadModel(const std::string& model_path) {
    std::string canonical = canonicalizePath(model_path);

    // 拡張子チェック（.ggufまたはOllama blobファイル形式を許可）
    fs::path p(canonical);
    std::string filename = p.filename().string();
    if (p.extension() != ".gguf" && !isOllamaBlobFile(filename)) {
        spdlog::error("Invalid model file (expected .gguf or Ollama blob): {}", canonical);
        return false;
    }

    // ファイル存在チェック
    if (!fs::exists(canonical)) {
        spdlog::error("Model file not found: {}", canonical);
        return false;
    }

    std::lock_guard<std::mutex> lock(mutex_);

    // 既にロード済みか確認
    if (loaded_models_.count(canonical) > 0) {
        spdlog::debug("Model already loaded: {}", canonical);
        return true;
    }

    spdlog::info("Loading model: {} (gpu_layers={})", canonical, gpu_layers_);

    // モデルパラメータ設定
    llama_model_params model_params = llama_model_default_params();
    model_params.n_gpu_layers = static_cast<int32_t>(gpu_layers_);

    // モデルロード
    llama_model* model = llama_model_load_from_file(canonical.c_str(), model_params);
    if (!model) {
        spdlog::error("Failed to load model: {}", canonical);
        return false;
    }

    // コンテキストパラメータ設定
    llama_context_params ctx_params = llama_context_default_params();
    ctx_params.n_ctx = 4096;  // コンテキストサイズ
    ctx_params.n_batch = 512; // バッチサイズ

    // コンテキスト作成
    llama_context* ctx = llama_init_from_model(model, ctx_params);
    if (!ctx) {
        spdlog::error("Failed to create context for model: {}", canonical);
        llama_model_free(model);
        return false;
    }

    // LlamaContext構造体に格納
    auto llama_ctx = std::make_unique<LlamaContext>();
    llama_ctx->model_path = canonical;
    llama_ctx->model = model;
    llama_ctx->ctx = ctx;
    llama_ctx->gpu_layers = gpu_layers_;

    // 実メモリ使用量を取得
    uint64_t model_size = llama_model_size(model);
    memory_bytes_ += model_size;

    spdlog::info("Model loaded successfully: {} ({} bytes)", canonical, model_size);

    loaded_models_[canonical] = std::move(llama_ctx);
    return true;
}

// モデルがロード済みか確認
bool LlamaManager::isLoaded(const std::string& model_path) const {
    std::string canonical = canonicalizePath(model_path);
    std::lock_guard<std::mutex> lock(mutex_);
    return loaded_models_.count(canonical) > 0;
}

// コンテキスト取得
llama_context* LlamaManager::getContext(const std::string& model_path) const {
    std::string canonical = canonicalizePath(model_path);
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = loaded_models_.find(canonical);
    if (it == loaded_models_.end()) {
        return nullptr;
    }
    return it->second->ctx;
}

// モデル取得
llama_model* LlamaManager::getModel(const std::string& model_path) const {
    std::string canonical = canonicalizePath(model_path);
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = loaded_models_.find(canonical);
    if (it == loaded_models_.end()) {
        return nullptr;
    }
    return it->second->model;
}

// 旧APIとの互換性: コンテキスト生成
// 注意: 返されるshared_ptrはリソースを所有しない（カスタムデリーター使用）
std::shared_ptr<LlamaContext> LlamaManager::createContext(const std::string& model) const {
    std::string canonical = canonicalizePath(model);
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = loaded_models_.find(canonical);
    if (it == loaded_models_.end()) {
        return nullptr;
    }
    // カスタムデリーター（何もしない）で二重解放を防ぐ
    // 実際のリソースはLlamaManagerが管理
    return std::shared_ptr<LlamaContext>(it->second.get(), [](LlamaContext*) {});
}

size_t LlamaManager::loadedCount() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return loaded_models_.size();
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
    std::string canonical = canonicalizePath(model_path);
    std::lock_guard<std::mutex> lock(mutex_);

    auto it = loaded_models_.find(canonical);
    if (it == loaded_models_.end()) {
        return false;
    }

    // メモリ使用量を減算
    if (it->second->model) {
        uint64_t model_size = llama_model_size(it->second->model);
        if (memory_bytes_ >= model_size) {
            memory_bytes_ -= model_size;
        } else {
            memory_bytes_ = 0;
        }
    }

    spdlog::info("Unloading model: {}", canonical);
    loaded_models_.erase(it);
    return true;
}

std::vector<std::string> LlamaManager::getLoadedModels() const {
    std::lock_guard<std::mutex> lock(mutex_);
    std::vector<std::string> models;
    models.reserve(loaded_models_.size());
    for (const auto& pair : loaded_models_) {
        models.push_back(pair.first);
    }
    return models;
}

// アクセス時刻を更新
void LlamaManager::updateAccessTime(const std::string& model_path) {
    last_access_[model_path] = std::chrono::steady_clock::now();
}

// オンデマンドロード: モデルが未ロードなら自動ロード
bool LlamaManager::loadModelIfNeeded(const std::string& model_path) {
    std::string canonical = canonicalizePath(model_path);

    // 既にロード済みならアクセス時刻を更新して返す
    {
        std::lock_guard<std::mutex> lock(mutex_);
        if (loaded_models_.count(canonical) > 0) {
            updateAccessTime(canonical);
            return true;
        }
    }

    // ロード数制限チェック
    if (!canLoadMore()) {
        // LRUモデルをアンロードしてスペースを確保
        auto lru = getLeastRecentlyUsedModel();
        if (lru.has_value()) {
            spdlog::info("Unloading LRU model to make room: {}", lru.value());
            unloadModel(lru.value());
        }
    }

    // モデルをロード
    bool result = loadModel(model_path);
    if (result) {
        std::lock_guard<std::mutex> lock(mutex_);
        updateAccessTime(canonical);
    }
    return result;
}

// アイドルタイムアウト設定
void LlamaManager::setIdleTimeout(std::chrono::milliseconds timeout) {
    idle_timeout_ = timeout;
}

std::chrono::milliseconds LlamaManager::getIdleTimeout() const {
    return idle_timeout_;
}

// アイドルモデルのアンロード
size_t LlamaManager::unloadIdleModels() {
    auto now = std::chrono::steady_clock::now();
    std::vector<std::string> to_unload;

    {
        std::lock_guard<std::mutex> lock(mutex_);
        for (const auto& pair : loaded_models_) {
            auto it = last_access_.find(pair.first);
            if (it != last_access_.end()) {
                auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                    now - it->second);
                if (elapsed >= idle_timeout_) {
                    to_unload.push_back(pair.first);
                }
            }
        }
    }

    for (const auto& model : to_unload) {
        spdlog::info("Unloading idle model: {}", model);
        unloadModel(model);
    }

    // アクセス時刻情報もクリーンアップ
    {
        std::lock_guard<std::mutex> lock(mutex_);
        for (const auto& model : to_unload) {
            last_access_.erase(model);
        }
    }

    return to_unload.size();
}

// 最大ロード数設定
void LlamaManager::setMaxLoadedModels(size_t max_models) {
    max_loaded_models_ = max_models;
}

size_t LlamaManager::getMaxLoadedModels() const {
    return max_loaded_models_;
}

// ロード可能かチェック
bool LlamaManager::canLoadMore() const {
    if (max_loaded_models_ == 0) {
        return true;  // 制限なし
    }
    std::lock_guard<std::mutex> lock(mutex_);
    return loaded_models_.size() < max_loaded_models_;
}

// メモリ制限設定
void LlamaManager::setMaxMemoryBytes(size_t max_bytes) {
    max_memory_bytes_ = max_bytes;
}

size_t LlamaManager::getMaxMemoryBytes() const {
    return max_memory_bytes_;
}

// 最終アクセス時刻取得
std::optional<std::chrono::steady_clock::time_point> LlamaManager::getLastAccessTime(
    const std::string& model_path) const {
    std::string canonical = canonicalizePath(model_path);
    std::lock_guard<std::mutex> lock(mutex_);

    // モデルがロードされていなければnullopt
    if (loaded_models_.count(canonical) == 0) {
        return std::nullopt;
    }

    auto it = last_access_.find(canonical);
    if (it == last_access_.end()) {
        return std::nullopt;
    }
    return it->second;
}

// LRU: 最も古くアクセスされたモデルを取得
std::optional<std::string> LlamaManager::getLeastRecentlyUsedModel() const {
    std::lock_guard<std::mutex> lock(mutex_);

    if (loaded_models_.empty()) {
        return std::nullopt;
    }

    std::string oldest_model;
    std::chrono::steady_clock::time_point oldest_time = std::chrono::steady_clock::time_point::max();

    for (const auto& pair : loaded_models_) {
        auto it = last_access_.find(pair.first);
        if (it != last_access_.end()) {
            if (it->second < oldest_time) {
                oldest_time = it->second;
                oldest_model = pair.first;
            }
        } else {
            // アクセス時刻がないモデルは最も古いとみなす
            return pair.first;
        }
    }

    if (oldest_model.empty()) {
        return std::nullopt;
    }
    return oldest_model;
}

}  // namespace ollama_node
