#include "models/model_repair.h"

#include <spdlog/spdlog.h>
#include <filesystem>
#include <fstream>

#include "models/model_sync.h"
#include "models/model_downloader.h"
#include "models/model_storage.h"

namespace fs = std::filesystem;

namespace llm_node {

ModelRepair::ModelRepair(ModelSync& sync, ModelDownloader& downloader, ModelStorage& storage)
    : sync_(sync), downloader_(downloader), storage_(storage) {}

bool ModelRepair::needsRepair(const std::string& model_path) const {
    // ファイルが存在しない場合は修復が必要
    if (!fs::exists(model_path)) {
        return true;
    }

    // ファイルサイズが1KB未満の場合は破損とみなす
    std::error_code ec;
    auto size = fs::file_size(model_path, ec);
    if (ec || size < 1024) {
        return true;
    }

    // GGUFヘッダーを検証
    if (!validateGgufHeader(model_path)) {
        return true;
    }

    return false;
}

bool ModelRepair::validateGgufHeader(const std::string& path) {
    std::ifstream file(path, std::ios::binary);
    if (!file.is_open()) {
        return false;
    }

    // GGUFマジックナンバー: "GGUF" (0x46554747)
    char magic[4];
    file.read(magic, 4);
    if (!file || file.gcount() != 4) {
        return false;
    }

    // リトルエンディアンで "GGUF"
    return magic[0] == 'G' && magic[1] == 'G' && magic[2] == 'U' && magic[3] == 'F';
}

RepairResult ModelRepair::repair(const std::string& model_name,
                                  std::chrono::milliseconds timeout,
                                  ProgressCallback progress_cb) {
    auto start_time = std::chrono::steady_clock::now();

    spdlog::info("Starting auto-repair for model: {}", model_name);

    // 既に修復中か確認
    {
        std::unique_lock<std::mutex> lock(mutex_);
        auto it = repairing_models_.find(model_name);
        if (it != repairing_models_.end() && !it->second->completed) {
            spdlog::info("Model {} is already being repaired, waiting...", model_name);
            // 既存の修復タスクの完了を待機
            bool completed = cv_.wait_for(lock, timeout, [&]() {
                auto iter = repairing_models_.find(model_name);
                return iter == repairing_models_.end() || iter->second->completed;
            });

            if (!completed) {
                return RepairResult{
                    RepairStatus::Failed,
                    "Repair timeout while waiting for existing repair",
                    model_name,
                    std::chrono::duration_cast<std::chrono::milliseconds>(
                        std::chrono::steady_clock::now() - start_time)
                };
            }

            // 既存の修復結果を返す
            if (it != repairing_models_.end() && it->second->completed) {
                return it->second->result;
            }
        }
    }

    // 新しい修復タスクを開始
    auto task = startRepairTask(model_name);

    // モデルをダウンロード
    bool success = sync_.downloadModel(downloader_, model_name, progress_cb);

    auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now() - start_time);

    RepairResult result;
    result.model_path = model_name;
    result.elapsed = elapsed;

    if (success) {
        result.status = RepairStatus::Success;
        spdlog::info("Auto-repair completed: {} (elapsed: {}ms)", model_name, elapsed.count());
    } else {
        result.status = RepairStatus::Failed;
        result.error_message = "Failed to download model";
        spdlog::error("Auto-repair failed: {} - {}", model_name, result.error_message);
    }

    // 修復タスクを完了
    completeRepairTask(model_name, result);

    return result;
}

bool ModelRepair::isRepairing(const std::string& model_name) const {
    std::lock_guard<std::mutex> lock(mutex_);
    auto it = repairing_models_.find(model_name);
    return it != repairing_models_.end() && !it->second->completed;
}

bool ModelRepair::waitForRepair(const std::string& model_name, std::chrono::milliseconds timeout) {
    std::unique_lock<std::mutex> lock(mutex_);
    return cv_.wait_for(lock, timeout, [&]() {
        auto it = repairing_models_.find(model_name);
        return it == repairing_models_.end() || it->second->completed;
    });
}

void ModelRepair::setDefaultTimeout(std::chrono::milliseconds timeout) {
    default_timeout_ = timeout;
}

std::chrono::milliseconds ModelRepair::getDefaultTimeout() const {
    return default_timeout_;
}

std::shared_ptr<RepairTask> ModelRepair::startRepairTask(const std::string& model_name) {
    std::lock_guard<std::mutex> lock(mutex_);
    auto task = std::make_shared<RepairTask>();
    task->model_name = model_name;
    task->started_at = std::chrono::system_clock::now();
    task->completed = false;
    repairing_models_[model_name] = task;
    return task;
}

void ModelRepair::completeRepairTask(const std::string& model_name, const RepairResult& result) {
    {
        std::lock_guard<std::mutex> lock(mutex_);
        auto it = repairing_models_.find(model_name);
        if (it != repairing_models_.end()) {
            it->second->completed = true;
            it->second->result = result;
        }
    }
    cv_.notify_all();
}

}  // namespace llm_node
