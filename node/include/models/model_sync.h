#pragma once

#include <string>
#include <vector>
#include <chrono>
#include <unordered_map>
#include <mutex>
#include <optional>

#include "models/model_downloader.h"

namespace ollama_node {

enum class SyncState {
    Idle,
    Running,
    Success,
    Failed,
};

struct SyncStatusInfo {
    SyncState state{SyncState::Idle};
    std::chrono::system_clock::time_point updated_at{};
    std::vector<std::string> last_to_download;
    std::vector<std::string> last_to_delete;
};

struct ModelSyncResult {
    std::vector<std::string> to_download;
    std::vector<std::string> to_delete;
};

struct DownloadHint {
    std::string etag;
    std::optional<size_t> size;
};

struct RemoteModel {
    std::string id;
    std::string path;
    std::string download_url;
    std::string chat_template;
};

class ModelSync {
public:
    ModelSync(std::string base_url, std::string models_dir,
              std::chrono::milliseconds timeout = std::chrono::milliseconds(5000));

    ModelSyncResult sync();

    std::vector<RemoteModel> fetchRemoteModels();
    std::vector<std::string> listLocalModels() const;

    // キャッシュされたETagを取得（存在しなければ空文字）
    // ETagをキャッシュに保存/取得
    std::string getCachedEtag(const std::string& model_id) const;
    void setCachedEtag(const std::string& model_id, std::string etag);

    // サイズキャッシュ
    std::optional<size_t> getCachedSize(const std::string& model_id) const;
    void setCachedSize(const std::string& model_id, size_t size);

    // ダウンロード時のヒントを取得（ETag/サイズ）
    DownloadHint getDownloadHint(const std::string& model_id) const;

    // Downloaderにヒントを自動適用してダウンロードを実行
    std::string downloadWithHint(ModelDownloader& downloader,
                                 const std::string& model_id,
                                 const std::string& blob_url,
                                 const std::string& filename,
                                 ProgressCallback cb = nullptr,
                                 const std::string& expected_sha256 = "") const;

    // manifestを取得し、files配列のエントリをまとめてダウンロード
    bool downloadModel(ModelDownloader& downloader,
                       const std::string& model_id,
                       ProgressCallback cb = nullptr) const;

    // モデルごとにチャンクサイズや帯域を上書きする設定（オプション）
    struct ModelOverrides {
        size_t chunk_size{0};
        size_t max_bps{0};
    };

    void setModelOverrides(std::unordered_map<std::string, ModelOverrides> overrides);

    // 並列ダウンロードの同時実行数（デフォルト4）。環境変数 LLM_DL_CONCURRENCY で上書き。
    static size_t defaultConcurrency();

    // 直近の同期ステータスを取得
    SyncStatusInfo getStatus() const;

    // Getter methods for paths
    const std::string& getModelsDir() const { return models_dir_; }
    const std::string& getBaseUrl() const { return base_url_; }

private:
    std::string base_url_;
    std::string models_dir_;
    std::chrono::milliseconds timeout_;

    mutable std::mutex etag_mutex_;
    std::unordered_map<std::string, std::string> etag_cache_;
    std::unordered_map<std::string, size_t> size_cache_;
    std::unordered_map<std::string, ModelOverrides> model_overrides_;
    std::unordered_map<std::string, RemoteModel> remote_models_;

    mutable std::mutex status_mutex_;
    SyncStatusInfo status_;
};

}  // namespace ollama_node
