#include <gtest/gtest.h>
#include <httplib.h>
#include <thread>
#include <atomic>
#include <nlohmann/json.hpp>

#include "api/router_client.h"

using namespace ollama_node;

namespace {

class RouterServer {
public:
    RouterServer() = default;

    void start(int port) {
        stop_flag_ = false;
        server_.Post("/api/nodes", [this](const httplib::Request& req, httplib::Response& res) {
            last_register_body = req.body;
            res.status = register_status;
            res.set_content(register_response_body, "application/json");
        });

        server_.Post("/api/nodes/heartbeat", [this](const httplib::Request& req, httplib::Response& res) {
            last_heartbeat_body = req.body;
            res.status = heartbeat_status;
            res.set_content("ok", "text/plain");
        });

        thread_ = std::thread([this, port]() { server_.listen("127.0.0.1", port); });

        // 待機してサーバー起動を保証
        while (!server_.is_running()) {
            std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
    }

    void stop() {
        server_.stop();
        if (thread_.joinable()) thread_.join();
        stop_flag_ = true;
    }

    ~RouterServer() { stop(); }

    httplib::Server server_;
    std::thread thread_;
    std::atomic<bool> stop_flag_{true};

    int register_status{200};
    std::string register_response_body{"{\"node_id\":\"node-1\"}"};
    std::string last_register_body;

    int heartbeat_status{200};
    std::string last_heartbeat_body;
};

TEST(RouterClientTest, RegisterNodeSuccess) {
    RouterServer server;
    server.start(18081);

    RouterClient client("http://127.0.0.1:18081");
    NodeInfo info{"host", "127.0.0.1", 11434, 8ull * 1024 * 1024 * 1024, 10.5, {"m1"}, true};

    auto result = client.registerNode(info);

    server.stop();

    EXPECT_TRUE(result.success);
    EXPECT_EQ(result.node_id, "node-1");
    EXPECT_FALSE(server.last_register_body.empty());
}

TEST(RouterClientTest, RegisterNodeFailureWhenServerReturnsError) {
    RouterServer server;
    server.register_status = 400;
    server.register_response_body = "invalid";
    server.start(18082);

    RouterClient client("http://127.0.0.1:18082");
    NodeInfo info{"host", "127.0.0.1", 11434, 0, 0.0, {}, false};

    auto result = client.registerNode(info);
    server.stop();

    EXPECT_FALSE(result.success);
    EXPECT_EQ(result.node_id, "");
    EXPECT_EQ(result.error, "invalid");
}

TEST(RouterClientTest, HeartbeatSucceeds) {
    RouterServer server;
    server.start(18083);

    RouterClient client("http://127.0.0.1:18083");
    bool ok = client.sendHeartbeat("node-xyz", "initializing");

    server.stop();

    EXPECT_TRUE(ok);
    EXPECT_FALSE(server.last_heartbeat_body.empty());
}

TEST(RouterClientTest, HeartbeatRetriesOnFailureAndSendsMetrics) {
    RouterServer server;
    server.heartbeat_status = 500;
    int hit_count = 0;
    server.server_.Post("/api/nodes/heartbeat", [&](const httplib::Request& req, httplib::Response& res) {
        hit_count++;
        server.last_heartbeat_body = req.body;
        if (hit_count >= 2) {
            res.status = 200;
            res.set_content("ok", "text/plain");
        } else {
            res.status = server.heartbeat_status;
            res.set_content("fail", "text/plain");
        }
    });
    server.start(18084);

    RouterClient client("http://127.0.0.1:18084");
    HeartbeatMetrics m{12.5, 34.5, 1024, 2048};
    bool ok = client.sendHeartbeat("node-xyz", "ready", m, 2);

    server.stop();

    EXPECT_TRUE(ok);
    EXPECT_GE(hit_count, 2);
    auto body = nlohmann::json::parse(server.last_heartbeat_body);
    ASSERT_TRUE(body.contains("metrics"));
    EXPECT_DOUBLE_EQ(body["metrics"]["cpu_utilization"], 12.5);
    EXPECT_DOUBLE_EQ(body["metrics"]["gpu_utilization"], 34.5);
    EXPECT_EQ(body["metrics"]["mem_used_bytes"], 1024);
    EXPECT_EQ(body["metrics"]["mem_total_bytes"], 2048);
}

}  // namespace
