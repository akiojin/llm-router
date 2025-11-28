#pragma once

#include <chrono>
#include <condition_variable>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>

namespace ollama_node {

class ModelSync;
class ModelDownloader;
class OllamaCompat;

/// モデルロード時のエラー種別
enum class ModelLoadError {
    None,           ///< エラーなし（成功）
    FileNotFound,   ///< ファイルが存在しない
    InvalidFormat,  ///< 拡張子が.ggufでない
    Corrupted,      ///< ファイル破損（ロード失敗）
    ContextFailed,  ///< コンテキスト作成失敗
    Unknown         ///< その他のエラー
};

/// 修復処理の状態
enum class RepairStatus {
    Idle,       ///< 修復処理なし
    InProgress, ///< 修復中
    Success,    ///< 修復成功
    Failed      ///< 修復失敗
};

/// 修復処理の結果
struct RepairResult {
    RepairStatus status{RepairStatus::Idle};
    std::string error_message;                  ///< 失敗時のエラーメッセージ
    std::string model_path;                     ///< 修復対象のモデルパス
    std::chrono::milliseconds elapsed{0};       ///< 処理時間
};

/// 進行中の修復タスク（内部用）
struct RepairTask {
    std::string model_name;
    std::chrono::system_clock::time_point started_at;
    bool completed{false};
    RepairResult result;
};

/// モデル自動修復クラス
///
/// モデルファイルが破損している場合に自動的に再ダウンロードする機能を提供。
/// 重複修復防止と待機機能を含む。
class ModelRepair {
public:
    using ProgressCallback = std::function<void(size_t downloaded, size_t total)>;

    /// コンストラクタ
    /// @param sync モデル同期オブジェクト
    /// @param downloader モデルダウンローダー
    /// @param compat Ollama互換レイヤー
    ModelRepair(ModelSync& sync, ModelDownloader& downloader, OllamaCompat& compat);

    ~ModelRepair() = default;

    // コピー禁止
    ModelRepair(const ModelRepair&) = delete;
    ModelRepair& operator=(const ModelRepair&) = delete;

    /// モデルファイルが修復を必要とするか判定
    /// @param model_path モデルファイルのパス
    /// @return 修復が必要な場合true
    bool needsRepair(const std::string& model_path) const;

    /// モデルを修復（再ダウンロード）
    /// @param model_name モデル名（例: "gpt-oss:7b"）
    /// @param timeout タイムアウト
    /// @param progress_cb 進捗コールバック（オプション）
    /// @return 修復結果
    RepairResult repair(const std::string& model_name,
                        std::chrono::milliseconds timeout,
                        ProgressCallback progress_cb = nullptr);

    /// モデルが現在修復中かどうか
    /// @param model_name モデル名
    /// @return 修復中の場合true
    bool isRepairing(const std::string& model_name) const;

    /// 修復完了を待機
    /// @param model_name モデル名
    /// @param timeout 待機タイムアウト
    /// @return 修復が完了した場合true、タイムアウトした場合false
    bool waitForRepair(const std::string& model_name, std::chrono::milliseconds timeout);

    /// デフォルトタイムアウトを設定
    void setDefaultTimeout(std::chrono::milliseconds timeout);

    /// デフォルトタイムアウトを取得
    std::chrono::milliseconds getDefaultTimeout() const;

private:
    ModelSync& sync_;
    ModelDownloader& downloader_;
    OllamaCompat& compat_;

    mutable std::mutex mutex_;
    std::condition_variable cv_;
    std::unordered_map<std::string, std::shared_ptr<RepairTask>> repairing_models_;

    std::chrono::milliseconds default_timeout_{std::chrono::seconds(300)};

    /// 修復タスクを開始（内部用）
    std::shared_ptr<RepairTask> startRepairTask(const std::string& model_name);

    /// 修復タスクを完了（内部用）
    void completeRepairTask(const std::string& model_name, const RepairResult& result);

    /// GGUFファイルのマジックナンバーを検証
    static bool validateGgufHeader(const std::string& path);
};

}  // namespace ollama_node
