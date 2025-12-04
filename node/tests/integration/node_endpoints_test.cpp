#include <gtest/gtest.h>
#include <httplib.h>

#include "api/http_server.h"
#include "api/openai_endpoints.h"
#include "api/node_endpoints.h"
#include "models/model_registry.h"
#include "core/inference_engine.h"
#include "runtime/state.h"

using namespace llm_node;

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

TEST(NodeEndpointsTest, LogLevelGetAndSet) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18087, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18087);
    auto get1 = cli.Get("/log/level");
    ASSERT_TRUE(get1);
    EXPECT_EQ(get1->status, 200);

    auto set = cli.Post("/log/level", R"({"level":"debug"})", "application/json");
    ASSERT_TRUE(set);
    EXPECT_EQ(set->status, 200);
    EXPECT_NE(set->body.find("debug"), std::string::npos);

    server.stop();
}

TEST(NodeEndpointsTest, StartupProbeReflectsReadyFlag) {
    llm_node::set_ready(false);
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18091, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18091);
    auto not_ready = cli.Get("/startup");
    ASSERT_TRUE(not_ready);
    EXPECT_EQ(not_ready->status, 503);

    llm_node::set_ready(true);
    auto ready = cli.Get("/startup");
    ASSERT_TRUE(ready);
    EXPECT_EQ(ready->status, 200);

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

TEST(HttpServerTest, RequestIdGeneratedAndEchoed) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18092, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18092);
    auto resp = cli.Get("/health");
    ASSERT_TRUE(resp);
    auto id = resp->get_header_value("X-Request-Id");
    EXPECT_FALSE(id.empty());

    // Custom request id is echoed
    httplib::Headers h{{"X-Request-Id", "custom-id"}};
    auto resp2 = cli.Get("/health", h);
    ASSERT_TRUE(resp2);
    EXPECT_EQ(resp2->get_header_value("X-Request-Id"), "custom-id");

    server.stop();
}

TEST(HttpServerTest, TraceparentPropagatesTraceId) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18093, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18093);
    std::string incoming = "00-11111111111111111111111111111111-2222222222222222-01";
    httplib::Headers h{{"traceparent", incoming}};
    auto resp = cli.Get("/health", h);
    ASSERT_TRUE(resp);
    auto tp = resp->get_header_value("traceparent");
    EXPECT_FALSE(tp.empty());
    EXPECT_NE(tp.find("11111111111111111111111111111111"), std::string::npos);
    EXPECT_EQ(tp.size(), 55);
    server.stop();
}
