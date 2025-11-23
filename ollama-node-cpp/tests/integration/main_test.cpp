#include <gtest/gtest.h>
#include <httplib.h>
#include <thread>
#include <atomic>
#include <cstdlib>
#include <filesystem>
#include <csignal>

#include "runtime/state.h"

extern "C" int ollama_node_run_for_test();

using namespace std::chrono_literals;

class TempDir {
public:
    TempDir() {
        auto base = std::filesystem::temp_directory_path();
        path = base / ("ollama-main-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
        std::filesystem::create_directories(path);
    }
    ~TempDir() {
        std::error_code ec;
        std::filesystem::remove_all(path, ec);
    }
    std::filesystem::path path;
};

TEST(MainTest, RunsWithStubRouterAndShutsDownOnFlag) {
    const int router_port = 18130;
    const int node_port = 18131;

    // Stub router that accepts register/heartbeat and lists one model
    httplib::Server router;
    router.Post("/api/nodes", [](const httplib::Request&, httplib::Response& res) {
        res.status = 200;
        res.set_content(R"({"node_id":"test-node"})", "application/json");
    });
    router.Post("/api/nodes/heartbeat", [](const httplib::Request&, httplib::Response& res) {
        res.status = 200;
        res.set_content("ok", "text/plain");
    });
    router.Get("/v1/models", [](const httplib::Request&, httplib::Response& res) {
        res.status = 200;
        res.set_content(R"({"data":[{"id":"gpt-oss:7b"}]})", "application/json");
    });

    std::thread router_thread([&]() { router.listen("127.0.0.1", router_port); });
    while (!router.is_running()) std::this_thread::sleep_for(10ms);

    TempDir models;
    setenv("OLLAMA_ROUTER_URL", ("http://127.0.0.1:" + std::to_string(router_port)).c_str(), 1);
    setenv("OLLAMA_NODE_PORT", std::to_string(node_port).c_str(), 1);
    setenv("OLLAMA_MODELS_DIR", models.path.string().c_str(), 1);
    setenv("OLLAMA_ALLOW_NO_GPU", "true", 1);
    setenv("OLLAMA_HEARTBEAT_SECS", "1", 1);

    std::atomic<int> exit_code{0};
    std::thread node_thread([&]() { exit_code = ollama_node_run_for_test(); });

    // wait for node to start and accept a health check
    {
        httplib::Client cli("127.0.0.1", node_port);
        for (int i = 0; i < 50; ++i) {
            if (auto res = cli.Get("/health")) {
                if (res->status == 200) break;
            }
            std::this_thread::sleep_for(50ms);
        }
    }

    ollama_node::request_shutdown();
    node_thread.join();

    router.stop();
    if (router_thread.joinable()) router_thread.join();

    EXPECT_EQ(exit_code.load(), 0);
}

TEST(MainTest, FailsWhenRouterRegistrationFails) {
    const int router_port = 18132;
    const int node_port = 18133;

    httplib::Server router;
    router.Post("/api/nodes", [](const httplib::Request&, httplib::Response& res) {
        res.status = 500;
        res.set_content("error", "text/plain");
    });
    std::thread router_thread([&]() { router.listen("127.0.0.1", router_port); });
    while (!router.is_running()) std::this_thread::sleep_for(10ms);

    TempDir models;
    setenv("OLLAMA_ROUTER_URL", ("http://127.0.0.1:" + std::to_string(router_port)).c_str(), 1);
    setenv("OLLAMA_NODE_PORT", std::to_string(node_port).c_str(), 1);
    setenv("OLLAMA_MODELS_DIR", models.path.string().c_str(), 1);
    setenv("OLLAMA_ALLOW_NO_GPU", "true", 1);
    setenv("OLLAMA_HEARTBEAT_SECS", "1", 1);

    std::atomic<int> exit_code{0};
    std::thread node_thread([&]() { exit_code = ollama_node_run_for_test(); });
    node_thread.join();

    router.stop();
    if (router_thread.joinable()) router_thread.join();

    EXPECT_NE(exit_code.load(), 0);
}
