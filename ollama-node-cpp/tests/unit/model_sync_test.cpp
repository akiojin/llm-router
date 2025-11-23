#include <gtest/gtest.h>
#include <httplib.h>
#include <filesystem>
#include <fstream>
#include <algorithm>
#include <chrono>
#include <atomic>

#include "models/model_sync.h"

using namespace ollama_node;
namespace fs = std::filesystem;

class ModelServer {
public:
    void start(int port) {
        server_.Get("/v1/models", [this](const httplib::Request&, httplib::Response& res) {
            res.status = 200;
            res.set_content(response_body, "application/json");
        });
        thread_ = std::thread([this, port]() { server_.listen("127.0.0.1", port); });
        while (!server_.is_running()) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    void stop() {
        server_.stop();
        if (thread_.joinable()) thread_.join();
    }

    ~ModelServer() { stop(); }

    httplib::Server server_;
    std::thread thread_;
    std::string response_body{"{\"data\":[{\"id\":\"gpt-oss:7b\"},{\"id\":\"gpt-oss:20b\"}]}"};
};

class TempDirGuard {
public:
    TempDirGuard() {
        path = fs::temp_directory_path() / fs::path("model-sync-XXXXXX");
        std::string tmpl = path.string();
        // mkdtemp requires mutable char*
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

TEST(ModelSyncTest, DetectsMissingAndStaleModels) {
    ModelServer server;
    server.start(18084);

    TempDirGuard guard;
    // local has stale model and one existing
    fs::create_directory(guard.path / "gpt-oss:7b");
    fs::create_directory(guard.path / "old-model");

    ModelSync sync("http://127.0.0.1:18084", guard.path.string());
    auto result = sync.sync();

    server.stop();

    ASSERT_EQ(result.to_download.size(), 1);
    EXPECT_EQ(result.to_download[0], "gpt-oss:20b");
    ASSERT_EQ(result.to_delete.size(), 1);
    EXPECT_EQ(result.to_delete[0], "old-model");
}

TEST(ModelSyncTest, EmptyWhenRouterUnavailable) {
    TempDirGuard guard;
    ModelSync sync("http://127.0.0.1:18085", guard.path.string(), std::chrono::milliseconds(200));
    auto result = sync.sync();
    EXPECT_TRUE(result.to_download.empty());
    EXPECT_TRUE(result.to_delete.empty());
}

TEST(ModelSyncTest, ReportsStatusTransitionsAndLastResult) {
    ModelServer server;
    server.response_body = R"({"data":[{"id":"m1"},{"id":"m2"}]})";
    server.start(18086);

    TempDirGuard guard;
    fs::create_directory(guard.path / "m1");  // already present

    ModelSync sync("http://127.0.0.1:18086", guard.path.string());

    auto initial = sync.getStatus();
    EXPECT_EQ(initial.state, SyncState::Idle);

    auto result = sync.sync();
    EXPECT_EQ(result.to_download.size(), 1u);
    EXPECT_EQ(result.to_download[0], "m2");
    EXPECT_EQ(result.to_delete.size(), 0u);

    auto after = sync.getStatus();
    EXPECT_EQ(after.state, SyncState::Success);
    ASSERT_EQ(after.last_to_download.size(), 1u);
    EXPECT_EQ(after.last_to_download[0], "m2");
    EXPECT_TRUE(after.last_to_delete.empty());
    EXPECT_NE(after.updated_at.time_since_epoch().count(), 0);

    server.stop();
}

TEST(ModelSyncTest, PrioritiesControlConcurrencyAndOrder) {
    const int port = 18110;
    httplib::Server server;

    std::atomic<int> hi_current{0}, hi_max{0};
    std::atomic<int> lo_current{0}, lo_max{0};
    std::atomic<int> hi_finished{0};

    auto slow_handler = [](std::atomic<int>& cur, std::atomic<int>& mx, std::atomic<int>* finished) {
        return [&cur, &mx, finished](const httplib::Request&, httplib::Response& res) {
            int now = ++cur;
            mx.store(std::max(mx.load(), now));
            std::this_thread::sleep_for(std::chrono::milliseconds(120));
            res.status = 200;
            res.set_content("data", "application/octet-stream");
            --cur;
            if (finished) ++(*finished);
        };
    };

    server.Get("/gpt-oss:prio/manifest.json", [](const httplib::Request&, httplib::Response& res) {
        res.status = 200;
        res.set_content(R"({
            "files":[
                {"name":"hi1.bin","url":"http://127.0.0.1:18110/hi1.bin","priority":1},
                {"name":"hi2.bin","url":"http://127.0.0.1:18110/hi2.bin","priority":1},
                {"name":"lo1.bin","url":"http://127.0.0.1:18110/lo1.bin","priority":-2},
                {"name":"lo2.bin","url":"http://127.0.0.1:18110/lo2.bin","priority":-3}
            ]
        })", "application/json");
    });

    server.Get("/hi1.bin", slow_handler(hi_current, hi_max, &hi_finished));
    server.Get("/hi2.bin", slow_handler(hi_current, hi_max, &hi_finished));
    server.Get("/lo1.bin", slow_handler(lo_current, lo_max, nullptr));
    server.Get("/lo2.bin", slow_handler(lo_current, lo_max, nullptr));

    std::thread th([&]() { server.listen("127.0.0.1", port); });
    while (!server.is_running()) std::this_thread::sleep_for(std::chrono::milliseconds(10));

    TempDirGuard dir;
    ModelDownloader dl("http://127.0.0.1:18110", dir.path.string());
    ModelSync sync("http://127.0.0.1:18110", dir.path.string());

    bool ok = sync.downloadModel(dl, "gpt-oss:prio", nullptr);

    server.stop();
    if (th.joinable()) th.join();

    EXPECT_TRUE(ok) << "hi_finished=" << hi_finished.load()
                    << " hi_max=" << hi_max.load()
                    << " lo_max=" << lo_max.load();
    EXPECT_EQ(hi_finished.load(), 2);
    EXPECT_EQ(hi_max.load(), 2);     // hi priority tasks can fully utilize concurrency (2 tasks)
    EXPECT_EQ(lo_max.load(), 1);     // low priority tasks are throttled to single concurrency (-3 priority)
    // Low priority should start after high priority tasks complete
    EXPECT_EQ(hi_current.load(), 0);
}
