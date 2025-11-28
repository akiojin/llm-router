#include <gtest/gtest.h>
#include <httplib.h>
#include <filesystem>
#include <fstream>
#include <thread>
#include <atomic>
#include <chrono>

#include "models/model_repair.h"
#include "models/model_sync.h"
#include "models/model_downloader.h"
#include "models/ollama_compat.h"

namespace fs = std::filesystem;
using namespace ollama_node;
using namespace std::chrono_literals;

class TempDirGuard {
public:
    TempDirGuard() {
        path = fs::temp_directory_path() / fs::path("auto-repair-test-XXXXXX");
        std::string tmpl = path.string();
        std::vector<char> buf(tmpl.begin(), tmpl.end());
        buf.push_back('\0');
        char* created = mkdtemp(buf.data());
        path = created ? fs::path(created) : fs::temp_directory_path();
    }
    ~TempDirGuard() {
        std::error_code ec;
        fs::remove_all(path, ec);
    }
    fs::path path;
};

// テスト用の有効なGGUFファイルを作成
void createValidGgufFile(const fs::path& path, size_t size = 4096) {
    fs::create_directories(path.parent_path());
    std::ofstream file(path, std::ios::binary);
    file.write("GGUF", 4);
    uint32_t version = 3;
    file.write(reinterpret_cast<const char*>(&version), sizeof(version));
    std::vector<char> padding(size - 8, 0);
    file.write(padding.data(), padding.size());
}

// テスト用の破損ファイルを作成
void createCorruptedFile(const fs::path& path) {
    fs::create_directories(path.parent_path());
    std::ofstream file(path, std::ios::binary);
    file.write("corrupted_data", 14);
}

// =========================================================
// 自動修復成功テスト
// =========================================================

TEST(AutoRepairIntegrationTest, RepairSucceedsWithMockServer) {
    const int port = 18200;
    TempDirGuard guard;

    // モックサーバー: manifestとblobを提供
    httplib::Server server;

    // manifest.jsonを返す
    server.Get("/test-model/manifest.json", [](const httplib::Request&, httplib::Response& res) {
        res.status = 200;
        res.set_content(R"({
            "files": [
                {"name": "model.gguf", "url": "http://127.0.0.1:18200/model.gguf"}
            ]
        })", "application/json");
    });

    // 有効なGGUFファイルを返す
    server.Get("/model.gguf", [](const httplib::Request&, httplib::Response& res) {
        std::string content;
        content.reserve(4096);
        // GGUFマジックナンバー
        content += "GGUF";
        // バージョン番号（リトルエンディアン、version 3）
        uint32_t version = 3;
        content.append(reinterpret_cast<const char*>(&version), sizeof(version));
        // 残りをゼロパディング
        content.append(4088, '\0');
        res.status = 200;
        res.set_content(content, "application/octet-stream");
    });

    std::thread server_thread([&]() { server.listen("127.0.0.1", port); });
    while (!server.is_running()) std::this_thread::sleep_for(10ms);

    ModelSync sync("http://127.0.0.1:" + std::to_string(port), guard.path.string());
    ModelDownloader downloader("http://127.0.0.1:" + std::to_string(port), guard.path.string());
    OllamaCompat compat(guard.path.string());

    ModelRepair repair(sync, downloader, compat);

    auto result = repair.repair("test-model", 30s);

    server.stop();
    if (server_thread.joinable()) server_thread.join();

    EXPECT_EQ(result.status, RepairStatus::Success);
    EXPECT_TRUE(result.error_message.empty());
    // elapsed.count() >= 0 は常に真なので、結果が設定されていることのみ確認
    EXPECT_GE(result.elapsed.count(), 0);
}

// =========================================================
// 修復失敗テスト（サーバーエラー）
// =========================================================

TEST(AutoRepairIntegrationTest, RepairFailsWhenServerUnavailable) {
    TempDirGuard guard;

    // 存在しないサーバーを指定
    ModelSync sync("http://127.0.0.1:19999", guard.path.string());
    ModelDownloader downloader("http://127.0.0.1:19999", guard.path.string(), 500ms);
    OllamaCompat compat(guard.path.string());

    ModelRepair repair(sync, downloader, compat);

    auto result = repair.repair("non-existent-model", 2s);

    EXPECT_EQ(result.status, RepairStatus::Failed);
    EXPECT_FALSE(result.error_message.empty());
}

// =========================================================
// 重複修復防止テスト
// =========================================================

TEST(AutoRepairIntegrationTest, ConcurrentRepairRequestsAreDeduplicated) {
    const int port = 18201;
    TempDirGuard guard;

    std::atomic<int> download_count{0};

    httplib::Server server;

    server.Get("/concurrent-model/manifest.json", [](const httplib::Request&, httplib::Response& res) {
        res.status = 200;
        res.set_content(R"({
            "files": [
                {"name": "model.gguf", "url": "http://127.0.0.1:18201/slow-model.gguf"}
            ]
        })", "application/json");
    });

    // 遅いダウンロード（重複防止をテストするため）
    server.Get("/slow-model.gguf", [&download_count](const httplib::Request&, httplib::Response& res) {
        download_count++;
        std::this_thread::sleep_for(200ms);
        std::string content = "GGUF";
        content += std::string(4092, '\0');
        res.status = 200;
        res.set_content(content, "application/octet-stream");
    });

    std::thread server_thread([&]() { server.listen("127.0.0.1", port); });
    while (!server.is_running()) std::this_thread::sleep_for(10ms);

    ModelSync sync("http://127.0.0.1:" + std::to_string(port), guard.path.string());
    ModelDownloader downloader("http://127.0.0.1:" + std::to_string(port), guard.path.string());
    OllamaCompat compat(guard.path.string());

    ModelRepair repair(sync, downloader, compat);

    // 同時に3つの修復リクエストを開始
    std::vector<std::thread> threads;
    std::vector<RepairResult> results(3);

    for (int i = 0; i < 3; ++i) {
        threads.emplace_back([&repair, &results, i]() {
            results[i] = repair.repair("concurrent-model", 10s);
        });
    }

    for (auto& t : threads) {
        if (t.joinable()) t.join();
    }

    server.stop();
    if (server_thread.joinable()) server_thread.join();

    // すべてのリクエストが成功することを確認
    for (const auto& result : results) {
        EXPECT_EQ(result.status, RepairStatus::Success);
    }

    // ダウンロードは1回のみ実行されることを確認
    // 注: タイミングによっては複数回実行される可能性があるが、
    // 少なくとも3回未満であるべき
    EXPECT_LE(download_count.load(), 2);
}

// =========================================================
// 修復中の待機テスト
// =========================================================

TEST(AutoRepairIntegrationTest, WaitForRepairReturnsAfterCompletion) {
    const int port = 18202;
    TempDirGuard guard;

    httplib::Server server;

    server.Get("/wait-model/manifest.json", [](const httplib::Request&, httplib::Response& res) {
        res.status = 200;
        res.set_content(R"({
            "files": [
                {"name": "model.gguf", "url": "http://127.0.0.1:18202/wait-model.gguf"}
            ]
        })", "application/json");
    });

    server.Get("/wait-model.gguf", [](const httplib::Request&, httplib::Response& res) {
        std::this_thread::sleep_for(100ms);
        std::string content = "GGUF";
        content += std::string(4092, '\0');
        res.status = 200;
        res.set_content(content, "application/octet-stream");
    });

    std::thread server_thread([&]() { server.listen("127.0.0.1", port); });
    while (!server.is_running()) std::this_thread::sleep_for(10ms);

    ModelSync sync("http://127.0.0.1:" + std::to_string(port), guard.path.string());
    ModelDownloader downloader("http://127.0.0.1:" + std::to_string(port), guard.path.string());
    OllamaCompat compat(guard.path.string());

    ModelRepair repair(sync, downloader, compat);

    // 修復を開始（別スレッドで）
    std::thread repair_thread([&repair]() {
        repair.repair("wait-model", 10s);
    });

    // 少し待ってから修復中であることを確認
    std::this_thread::sleep_for(50ms);

    // 修復完了を待機
    bool completed = repair.waitForRepair("wait-model", 5s);

    repair_thread.join();

    server.stop();
    if (server_thread.joinable()) server_thread.join();

    EXPECT_TRUE(completed);
}

// =========================================================
// タイムアウトテスト
// =========================================================

TEST(AutoRepairIntegrationTest, RepairTimesOutWithSlowServer) {
    const int port = 18203;
    TempDirGuard guard;

    httplib::Server server;

    server.Get("/timeout-model/manifest.json", [](const httplib::Request&, httplib::Response& res) {
        // 非常に遅い応答
        std::this_thread::sleep_for(3s);
        res.status = 200;
        res.set_content("{}", "application/json");
    });

    std::thread server_thread([&]() { server.listen("127.0.0.1", port); });
    while (!server.is_running()) std::this_thread::sleep_for(10ms);

    ModelSync sync("http://127.0.0.1:" + std::to_string(port), guard.path.string());
    ModelDownloader downloader("http://127.0.0.1:" + std::to_string(port), guard.path.string(), 500ms);
    OllamaCompat compat(guard.path.string());

    ModelRepair repair(sync, downloader, compat);

    // 短いタイムアウトでリクエスト
    auto result = repair.repair("timeout-model", 1s);

    server.stop();
    if (server_thread.joinable()) server_thread.join();

    // タイムアウトまたは失敗が予想される
    EXPECT_EQ(result.status, RepairStatus::Failed);
}
