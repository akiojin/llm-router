#include <gtest/gtest.h>
#include <httplib.h>

#include "api/http_server.h"
#include "api/openai_endpoints.h"
#include "api/node_endpoints.h"
#include "models/model_registry.h"
#include "core/inference_engine.h"

using namespace llm_node;

TEST(NodePrometheusTest, MetricsEndpointReturnsText) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18090, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18090);
    cli.Post("/pull", "{}", "application/json");
    auto resp = cli.Get("/metrics/prom");
    ASSERT_TRUE(resp);
    EXPECT_EQ(resp->status, 200);
    EXPECT_EQ(resp->get_header_value("Content-Type"), "text/plain");
    EXPECT_NE(resp->body.find("llm_node_uptime_seconds"), std::string::npos);
    EXPECT_NE(resp->body.find("llm_node_pull_total"), std::string::npos);

    server.stop();
}
