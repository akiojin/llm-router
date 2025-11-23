#include <gtest/gtest.h>
#include <httplib.h>
#include <thread>

#include "api/router_client.h"

using namespace ollama_node;

class RouterContractFixture : public ::testing::Test {
protected:
    void SetUp() override {
        server.Post("/api/nodes", [this](const httplib::Request& req, httplib::Response& res) {
            last_register = req.body;
            res.status = 200;
            res.set_content("{\"node_id\":\"node-123\"}", "application/json");
        });
        server.Post("/api/nodes/heartbeat", [this](const httplib::Request& req, httplib::Response& res) {
            last_heartbeat = req.body;
            res.status = 200;
            res.set_content("ok", "text/plain");
        });
        thread = std::thread([this]() { server.listen("127.0.0.1", 18091); });
        while (!server.is_running()) std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }

    void TearDown() override {
        server.stop();
        if (thread.joinable()) thread.join();
    }

    httplib::Server server;
    std::thread thread;
    std::string last_register;
    std::string last_heartbeat;
};

TEST_F(RouterContractFixture, RegisterNodeReturnsId) {
    RouterClient client("http://127.0.0.1:18091");
    NodeInfo info{"host", "127.0.0.1", 11434, 4ull * 1024 * 1024 * 1024, 2.5, {}, false};
    auto result = client.registerNode(info);
    EXPECT_TRUE(result.success);
    EXPECT_EQ(result.node_id, "node-123");
    EXPECT_FALSE(last_register.empty());
}

TEST_F(RouterContractFixture, HeartbeatSendsStatus) {
    RouterClient client("http://127.0.0.1:18091");
    bool ok = client.sendHeartbeat("node-123");
    EXPECT_TRUE(ok);
    EXPECT_NE(last_heartbeat.find("node-123"), std::string::npos);
}
