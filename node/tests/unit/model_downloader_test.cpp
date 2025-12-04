#include <gtest/gtest.h>
#include <httplib.h>
#include <chrono>
#include <filesystem>
#include <fstream>

#include "models/model_downloader.h"

using namespace llm_node;
namespace fs = std::filesystem;

class TempDir {
public:
    TempDir() {
        auto base = fs::temp_directory_path();
        for (int i = 0; i < 10; ++i) {
            auto candidate = base / fs::path("mdldl-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()) + "-" + std::to_string(i));
            std::error_code ec;
            if (fs::create_directories(candidate, ec)) {
                path = candidate;
                return;
            }
        }
        path.clear();
    }
    ~TempDir() {
        if (path.empty()) return;
        if (path.filename().string().rfind("mdldl-", 0) == 0) {
            std::error_code ec;
            fs::remove_all(path, ec);
        }
    }
    fs::path path;
};

class RegistryServer {
public:
    void start(int port) {
        server.Get(R"(/gpt-oss:7b/manifest.json)", [this](const httplib::Request&, httplib::Response& res) {
            res.status = 200;
            res.set_content("{\\\"model\\\":\\\"gpt-oss:7b\\\"}", "application/json");
        });
        server.Get("/blob.bin", [this](const httplib::Request&, httplib::Response& res) {
            res.status = 200;
            res.set_content(std::string(4, 'a'), "application/octet-stream");
        });
        thread = std::thread([this, port]() { server.listen("127.0.0.1", port); });
        while (!server.is_running()) std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    void stop() {
        server.stop();
        if (thread.joinable()) thread.join();
    }
    ~RegistryServer() { stop(); }

    httplib::Server server;
    std::thread thread;
};

class RangeRegistryServer {
public:
    void start(int port) {
        server.Get("/range.bin", [this](const httplib::Request& req, httplib::Response& res) {
            const std::string full = "abcdefgh";
            auto range = req.get_header_value("Range");
            if (!range.empty()) {
                size_t pos = range.find("=");
                size_t dash = range.find("-");
                size_t start = 0;
                if (pos != std::string::npos) {
                    std::string start_str = range.substr(pos + 1, dash == std::string::npos ? std::string::npos : dash - pos - 1);
                    start = static_cast<size_t>(std::stoul(start_str));
                }
                if (start >= full.size()) {
                    res.status = 416;
                    return;
                }
                std::string body = full.substr(start);
                res.status = 206;
                res.set_header("Content-Length", std::to_string(body.size()));
                res.set_content(body, "application/octet-stream");
            } else {
                res.status = 200;
                res.set_header("Content-Length", std::to_string(full.size()));
                res.set_content(full, "application/octet-stream");
            }
        });
        thread = std::thread([this, port]() { server.listen("127.0.0.1", port); });
        while (!server.is_running()) std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    void stop() {
        server.stop();
        if (thread.joinable()) thread.join();
    }

    ~RangeRegistryServer() { stop(); }

    httplib::Server server;
    std::thread thread;
};

class ChecksumRegistryServer {
public:
    void start(int port) {
        server.Get("/checksum.bin", [this](const httplib::Request&, httplib::Response& res) {
            const std::string body = "xyz123";
            res.status = 200;
            res.set_header("Content-Length", std::to_string(body.size()));
            res.set_content(body, "application/octet-stream");
        });
        server.Get("/flaky.bin", [this](const httplib::Request&, httplib::Response& res) {
            call_count++;
            if (call_count == 1) {
                res.status = 500;
                res.set_content("fail", "text/plain");
            } else {
                const std::string body = "aaaa";
                res.status = 200;
                res.set_header("Content-Length", std::to_string(body.size()));
                res.set_content(body, "application/octet-stream");
            }
        });
        server.Get("/etag.bin", [this](const httplib::Request& req, httplib::Response& res) {
            const std::string body = "newdata";
            auto inm = req.get_header_value("If-None-Match");
            if (inm == "\"v2\"" || inm == "v2") {
                res.status = 304;
                res.set_header("Content-Length", "0");
                return;
            }
            res.status = 200;
            res.set_header("ETag", "\"v2\"");
            res.set_header("Content-Length", std::to_string(body.size()));
            res.set_content(body, "application/octet-stream");
        });
        thread = std::thread([this, port]() { server.listen("127.0.0.1", port); });
        while (!server.is_running()) std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    void stop() {
        server.stop();
        if (thread.joinable()) thread.join();
    }

    ~ChecksumRegistryServer() { stop(); }

    httplib::Server server;
    std::thread thread;
    int call_count{0};
};

TEST(ModelDownloaderTest, FetchesManifestToLocalPath) {
    RegistryServer srv;
    srv.start(18092);
    TempDir tmp;

    ModelDownloader dl("http://127.0.0.1:18092", tmp.path.string());
    auto path = dl.fetchManifest("gpt-oss:7b");

    srv.stop();

    ASSERT_FALSE(path.empty());
    EXPECT_TRUE(fs::exists(path));
}

TEST(ModelDownloaderTest, DownloadsBlobAndReportsProgress) {
    RegistryServer srv;
    srv.start(18093);
    TempDir tmp;

    size_t last_downloaded = 0;
    size_t last_total = 0;
    bool called = false;
    ModelDownloader dl("http://127.0.0.1:18093", tmp.path.string());
    auto out = dl.downloadBlob("http://127.0.0.1:18093/blob.bin", "gpt-oss:7b/blob.bin",
                               [&](size_t d, size_t total) {
                                   last_downloaded = d;
                                   last_total = total;
                                   called = true;
                               });

    srv.stop();

    ASSERT_FALSE(out.empty());
    EXPECT_TRUE(fs::exists(out));

    std::ifstream ifs(out, std::ios::binary);
    std::string data((std::istreambuf_iterator<char>(ifs)), {});
    EXPECT_EQ(data, std::string(4, 'a'));
    EXPECT_TRUE(called);
    EXPECT_EQ(last_downloaded, 4u);
    EXPECT_EQ(last_total, 4u);
}

TEST(ModelDownloaderTest, ReturnsEmptyWhenServerRespondsError) {
    RegistryServer srv;
    srv.start(18094);
    TempDir tmp;

    ModelDownloader dl("http://127.0.0.1:18094", tmp.path.string());
    auto out = dl.downloadBlob("http://127.0.0.1:18094/does-not-exist", "missing.bin");

    srv.stop();

    EXPECT_TRUE(out.empty());
    EXPECT_FALSE(fs::exists(tmp.path / "missing.bin"));
}

TEST(ModelDownloaderTest, ResumesDownloadFromPartialFile) {
    RangeRegistryServer srv;
    srv.start(18095);
    TempDir tmp;

    // ensure test server is reachable
    {
        httplib::Client probe("127.0.0.1", 18095);
        auto res = probe.Get("/range.bin");
        ASSERT_TRUE(res);
        ASSERT_EQ(res->status, 200);
    }

    fs::create_directories(tmp.path / "gpt-oss:7b");
    auto partial = tmp.path / "gpt-oss:7b/blob.bin";
    {
        std::ofstream ofs(partial, std::ios::binary | std::ios::trunc);
        ofs << "abc";  // partial content (3/8 bytes)
    }

    size_t last_downloaded = 0;
    size_t last_total = 0;
    bool called = false;
    ModelDownloader dl("http://127.0.0.1:18095", tmp.path.string());
    auto out = dl.downloadBlob("http://127.0.0.1:18095/range.bin", "gpt-oss:7b/blob.bin",
                               [&](size_t d, size_t total) {
                                   last_downloaded = d;
                                   last_total = total;
                                   called = true;
                               });

    srv.stop();

    ASSERT_FALSE(out.empty());
    std::ifstream ifs(out, std::ios::binary);
    std::string data((std::istreambuf_iterator<char>(ifs)), {});
    EXPECT_EQ(data, std::string("abcdefgh"));
    EXPECT_TRUE(called);
    EXPECT_EQ(last_downloaded, 8u);
    EXPECT_EQ(last_total, 8u);
}

TEST(ModelDownloaderTest, VerifiesChecksumSuccess) {
    ChecksumRegistryServer srv;
    srv.start(18096);
    TempDir tmp;

    ModelDownloader dl("http://127.0.0.1:18096", tmp.path.string());
    auto out = dl.downloadBlob("http://127.0.0.1:18096/checksum.bin", "gpt-oss:7b/checksum.bin",
                               nullptr,
                               "f0a72890897acefdb2c6c8c06134339a73cc6205833ca38dba6f9fdc94b60596");

    srv.stop();

    ASSERT_FALSE(out.empty());
    EXPECT_TRUE(fs::exists(out));
}

TEST(ModelDownloaderTest, FailsWhenChecksumMismatch) {
    ChecksumRegistryServer srv;
    srv.start(18097);
    TempDir tmp;

    ModelDownloader dl("http://127.0.0.1:18097", tmp.path.string());
    auto out = dl.downloadBlob("http://127.0.0.1:18097/checksum.bin", "gpt-oss:7b/checksum.bin",
                               nullptr,
                               "0000000000000000000000000000000000000000000000000000000000000000");

    srv.stop();

    EXPECT_TRUE(out.empty());
    EXPECT_FALSE(fs::exists(tmp.path / "gpt-oss:7b/checksum.bin"));
}

TEST(ModelDownloaderTest, RetriesAndSucceedsAfterFailure) {
    ChecksumRegistryServer srv;
    srv.start(18098);
    TempDir tmp;

    ModelDownloader dl("http://127.0.0.1:18098", tmp.path.string(), std::chrono::milliseconds(500), 2, std::chrono::milliseconds(50));
    auto out = dl.downloadBlob("http://127.0.0.1:18098/flaky.bin", "gpt-oss:7b/flaky.bin",
                               nullptr,
                               "61be55a8e2f6b4e172338bddf184d6dbee29c98853e0a0485ecee7f27b9af0b4");

    srv.stop();

    ASSERT_FALSE(out.empty());
    EXPECT_TRUE(fs::exists(out));
    EXPECT_GE(srv.call_count, 2);
}

TEST(ModelDownloaderTest, SkipsDownloadOnNotModifiedWhenEtagMatches) {
    ChecksumRegistryServer srv;
    srv.start(18099);
    TempDir tmp;

    fs::create_directories(tmp.path / "gpt-oss:7b");
    auto path = tmp.path / "gpt-oss:7b/etag.bin";
    std::ofstream(path) << "old";

    // connectivity sanity check
    {
        httplib::Client probe("127.0.0.1", 18099);
        auto res = probe.Get("/etag.bin");
        ASSERT_TRUE(res);
        ASSERT_EQ(res->status, 200);
        httplib::Headers h{{"If-None-Match", "\"v2\""}};
        auto res2 = probe.Get("/etag.bin", h);
        ASSERT_TRUE(res2);
        ASSERT_EQ(res2->status, 304);
    }

    bool called = false;
    ModelDownloader dl("http://127.0.0.1:18099", tmp.path.string());
    auto out = dl.downloadBlob("http://127.0.0.1:18099/etag.bin", "gpt-oss:7b/etag.bin",
                               [&](size_t, size_t) { called = true; },
                               "", "\"v2\"");

    srv.stop();

    ASSERT_FALSE(out.empty());
    std::ifstream ifs(out);
    std::string content((std::istreambuf_iterator<char>(ifs)), {});
    EXPECT_EQ(content, "old");
    EXPECT_FALSE(called);
}

TEST(ModelDownloaderTest, DownloadsWhenEtagDiffers) {
    ChecksumRegistryServer srv;
    srv.start(18100);
    TempDir tmp;

    fs::create_directories(tmp.path / "gpt-oss:7b");
    auto path = tmp.path / "gpt-oss:7b/etag.bin";
    std::ofstream(path) << "old";

    {
        httplib::Client probe("127.0.0.1", 18100);
        auto res = probe.Get("/etag.bin");
        ASSERT_TRUE(res);
        ASSERT_EQ(res->status, 200);
        httplib::Headers h{{"If-None-Match", "\"v2\""}};
        auto res2 = probe.Get("/etag.bin", h);
        ASSERT_TRUE(res2);
        ASSERT_EQ(res2->status, 304);
    }

    ModelDownloader dl("http://127.0.0.1:18100", tmp.path.string());
    auto out = dl.downloadBlob("http://127.0.0.1:18100/etag.bin", "gpt-oss:7b/etag.bin",
                               nullptr,
                               "8a7537e6f466adffc74ddbd0e721d8723c4817e7ee9ef65646e3e98bd2eb5461", // sha256 of newdata
                               "\"v1\"");

    srv.stop();

    ASSERT_FALSE(out.empty());
    std::ifstream ifs(out);
    std::string content((std::istreambuf_iterator<char>(ifs)), {});
    EXPECT_EQ(content, "newdata");
}
