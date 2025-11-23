#include <gtest/gtest.h>
#include <httplib.h>
#include <filesystem>
#include <fstream>

#include "models/model_sync.h"
#include "models/model_downloader.h"

using namespace ollama_node;
namespace fs = std::filesystem;

class TempDirGuard {
public:
    TempDirGuard() {
        auto base = fs::temp_directory_path();
        for (int i = 0; i < 10; ++i) {
            auto candidate = base / fs::path("msdl-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()) + "-" + std::to_string(i));
            std::error_code ec;
            if (fs::create_directories(candidate, ec)) {
                path = candidate;
                return;
            }
        }
    }
    ~TempDirGuard() {
        if (!path.empty()) {
            std::error_code ec;
            fs::remove_all(path, ec);
        }
    }
    fs::path path;
};

class RouterAndRegistryServer {
public:
    void start(int router_port, int registry_port) {
        router_.Get("/v1/models", [this](const httplib::Request&, httplib::Response& res) {
            res.status = 200;
            res.set_content(R"({"data":[{"id":"gpt-oss:7b","etag":"\"etag-1\"","size":3}]})", "application/json");
        });

        registry_.Get(R"(/gpt-oss:7b/manifest.json)", [this](const httplib::Request&, httplib::Response& res) {
            res.status = 200;
            res.set_content(R"({"model":"gpt-oss:7b","files":[{"name":"blob.bin","digest":"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"}]})",
                            "application/json");
        });

        registry_.Get("/blob.bin", [this](const httplib::Request& req, httplib::Response& res) {
            const std::string body = "abc";
            auto inm = req.get_header_value("If-None-Match");
            if (inm == "\"etag-1\"" || inm == "etag-1") {
                res.status = 304;
                res.set_header("Content-Length", "0");
                return;
            }
            res.status = 200;
            res.set_header("Content-Length", std::to_string(body.size()));
            res.set_header("ETag", "\"etag-1\"");
            res.set_content(body, "application/octet-stream");
        });

        router_thread_ = std::thread([this, router_port]() { router_.listen("127.0.0.1", router_port); });
        registry_thread_ = std::thread([this, registry_port]() { registry_.listen("127.0.0.1", registry_port); });

        while (!router_.is_running() || !registry_.is_running()) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    void stop() {
        router_.stop();
        registry_.stop();
        if (router_thread_.joinable()) router_thread_.join();
        if (registry_thread_.joinable()) registry_thread_.join();
    }

    ~RouterAndRegistryServer() { stop(); }

private:
    httplib::Server router_;
    httplib::Server registry_;
    std::thread router_thread_;
    std::thread registry_thread_;
};

TEST(ModelSyncIntegrationTest, SyncsAndDownloadsMissingModel) {
    RouterAndRegistryServer server;
    server.start(18110, 18111);

    TempDirGuard tmp;

    ModelSync sync("http://127.0.0.1:18110", tmp.path.string());
    auto diff = sync.sync();
    ASSERT_EQ(diff.to_download.size(), 1u);
    EXPECT_EQ(diff.to_download[0], "gpt-oss:7b");

    auto cached = sync.getCachedEtag("gpt-oss:7b");
    EXPECT_EQ(cached, "\"etag-1\"");
    auto cached_size = sync.getCachedSize("gpt-oss:7b");
    ASSERT_TRUE(cached_size.has_value());
    EXPECT_EQ(*cached_size, 3u);

    ModelDownloader dl("http://127.0.0.1:18111", tmp.path.string());

    ASSERT_TRUE(sync.downloadModel(dl, "gpt-oss:7b"));

    auto hint = sync.getDownloadHint("gpt-oss:7b");
    ASSERT_FALSE(hint.etag.empty());
    ASSERT_TRUE(hint.size.has_value());

    // 2nd run with matching ETag should avoid re-download
    ASSERT_TRUE(sync.downloadModel(dl, "gpt-oss:7b"));
    auto blob_path = tmp.path / "gpt-oss:7b/blob.bin";
    EXPECT_TRUE(fs::exists(blob_path));

    server.stop();
}
