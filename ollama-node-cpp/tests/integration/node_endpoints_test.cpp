#include <gtest/gtest.h>
#include <httplib.h>

#include "api/http_server.h"
#include "api/openai_endpoints.h"
#include "api/node_endpoints.h"
#include "models/model_registry.h"
#include "core/inference_engine.h"

using namespace ollama_node;

TEST(NodeEndpointsTest, PullAndHealth) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18088, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18088);
    auto pull = cli.Post("/pull", "{}", "application/json");
    ASSERT_TRUE(pull);
    EXPECT_EQ(pull->status, 200);
    EXPECT_EQ(pull->get_header_value("Content-Type"), "application/json");

    auto health = cli.Get("/health");
    ASSERT_TRUE(health);
    EXPECT_EQ(health->status, 200);
    EXPECT_NE(health->body.find("ok"), std::string::npos);

    server.stop();
}

TEST(NodeEndpointsTest, MetricsReportsUptimeAndCounts) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18089, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18089);

    cli.Post("/pull", "{}", "application/json");
    auto metrics = cli.Get("/metrics");
    ASSERT_TRUE(metrics);
    EXPECT_EQ(metrics->status, 200);
    EXPECT_EQ(metrics->get_header_value("Content-Type"), "application/json");
    EXPECT_NE(metrics->body.find("uptime_seconds"), std::string::npos);
    EXPECT_NE(metrics->body.find("pull_count"), std::string::npos);

    server.stop();
}
