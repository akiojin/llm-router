#include <gtest/gtest.h>
#include <httplib.h>
#include <thread>
#include <atomic>
#include <nlohmann/json.hpp>

#include "api/router_client.h"

using namespace llm_node;

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

        server_.Post("/api/health", [this](const httplib::Request& req, httplib::Response& res) {
            last_heartbeat_body = req.body;
            last_heartbeat_token = req.get_header_value("X-Agent-Token");
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
    std::string register_response_body{R"({"node_id":"node-1","agent_token":"test-token-123"})"};
    std::string last_register_body;

    int heartbeat_status{200};
    std::string last_heartbeat_body;
    std::string last_heartbeat_token;
};

TEST(RouterClientTest, RegisterNodeSuccess) {
    RouterServer server;
    server.start(18081);

    RouterClient client("http://127.0.0.1:18081");
    NodeInfo info;
    info.machine_name = "test-host";
    info.ip_address = "127.0.0.1";
    info.runtime_version = "1.0.0";
    info.runtime_port = 11434;
    info.gpu_available = true;
    info.gpu_devices = {{.model = "Test GPU", .count = 1, .memory = 8ull * 1024 * 1024 * 1024}};
    info.gpu_count = 1;
    info.gpu_model = "Test GPU";

    auto result = client.registerNode(info);

    server.stop();

    EXPECT_TRUE(result.success);
    EXPECT_EQ(result.node_id, "node-1");
    EXPECT_EQ(result.agent_token, "test-token-123");
    EXPECT_FALSE(server.last_register_body.empty());

    // Verify JSON structure matches router protocol
    auto body = nlohmann::json::parse(server.last_register_body);
    EXPECT_EQ(body["machine_name"], "test-host");
    EXPECT_EQ(body["ip_address"], "127.0.0.1");
    EXPECT_EQ(body["runtime_version"], "1.0.0");
    EXPECT_EQ(body["runtime_port"], 11434);
    EXPECT_EQ(body["gpu_available"], true);
    EXPECT_EQ(body["gpu_devices"].size(), 1);
    EXPECT_EQ(body["gpu_devices"][0]["model"], "Test GPU");
}

TEST(RouterClientTest, RegisterNodeFailureWhenServerReturnsError) {
    RouterServer server;
    server.register_status = 400;
    server.register_response_body = "invalid";
    server.start(18082);

    RouterClient client("http://127.0.0.1:18082");
    NodeInfo info;
    info.machine_name = "test-host";
    info.ip_address = "127.0.0.1";
    info.runtime_version = "1.0.0";
    info.runtime_port = 11434;
    info.gpu_available = false;

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
    bool ok = client.sendHeartbeat("node-xyz", "test-agent-token", "initializing");

    server.stop();

    EXPECT_TRUE(ok);
    EXPECT_FALSE(server.last_heartbeat_body.empty());
    EXPECT_EQ(server.last_heartbeat_token, "test-agent-token");

    // Verify new HealthCheckRequest format
    auto body = nlohmann::json::parse(server.last_heartbeat_body);
    EXPECT_EQ(body["node_id"], "node-xyz");
    EXPECT_TRUE(body.contains("cpu_usage"));
    EXPECT_TRUE(body.contains("memory_usage"));
    EXPECT_TRUE(body.contains("active_requests"));
    EXPECT_TRUE(body.contains("loaded_models"));
    EXPECT_EQ(body["initializing"], false);
}

TEST(RouterClientTest, HeartbeatRetriesOnFailureAndSendsMetrics) {
    RouterServer server;
    server.heartbeat_status = 500;
    int hit_count = 0;
    server.server_.Post("/api/health", [&](const httplib::Request& req, httplib::Response& res) {
        hit_count++;
        server.last_heartbeat_body = req.body;
        server.last_heartbeat_token = req.get_header_value("X-Agent-Token");
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
    bool ok = client.sendHeartbeat("node-xyz", "retry-token", "ready", m, 2);

    server.stop();

    EXPECT_TRUE(ok);
    EXPECT_GE(hit_count, 2);
    EXPECT_EQ(server.last_heartbeat_token, "retry-token");
    auto body = nlohmann::json::parse(server.last_heartbeat_body);
    // New format uses cpu_usage/memory_usage instead of metrics object
    EXPECT_DOUBLE_EQ(body["cpu_usage"].get<double>(), 12.5);
    // memory_usage is calculated as percentage: (1024/2048) * 100 = 50
    EXPECT_DOUBLE_EQ(body["memory_usage"].get<double>(), 50.0);
}

}  // namespace
