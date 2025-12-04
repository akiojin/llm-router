#include <gtest/gtest.h>
#include <httplib.h>
#include <nlohmann/json.hpp>

#include "api/http_server.h"
#include "api/openai_endpoints.h"
#include "api/node_endpoints.h"
#include "models/model_registry.h"
#include "core/inference_engine.h"

using namespace llm_node;

TEST(HttpServerTest, ServesHealthAndMetrics) {
    ModelRegistry registry;
    registry.setModels({"gpt-oss:7b"});
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18086, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18086);
    auto res = cli.Get("/health");
    ASSERT_TRUE(res);
    EXPECT_EQ(res->status, 200);

    auto metrics = cli.Get("/metrics");
    ASSERT_TRUE(metrics);
    EXPECT_EQ(metrics->status, 200);

    server.stop();
}

TEST(HttpServerTest, HandlesCorsPreflightAndHeaders) {
    ModelRegistry registry;
    registry.setModels({"gpt-oss:7b"});
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18089, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18089);
    auto preflight = cli.Options("/v1/models");
    ASSERT_TRUE(preflight);
    EXPECT_EQ(preflight->status, 204);
    EXPECT_EQ(preflight->get_header_value("Access-Control-Allow-Origin"), "*");
    EXPECT_NE(preflight->get_header_value("Access-Control-Allow-Methods").find("GET"), std::string::npos);
    EXPECT_NE(preflight->get_header_value("Access-Control-Allow-Headers").find("Content-Type"), std::string::npos);

    auto models = cli.Get("/v1/models");
    ASSERT_TRUE(models);
    EXPECT_EQ(models->get_header_value("Access-Control-Allow-Origin"), "*");

    server.stop();
}

TEST(HttpServerTest, ReturnsJsonErrorsWithHandlers) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18090, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18090);

    auto notfound = cli.Get("/does-not-exist");
    ASSERT_TRUE(notfound);
    EXPECT_EQ(notfound->status, 404);
    auto body = nlohmann::json::parse(notfound->body);
    EXPECT_EQ(body.value("error", ""), "not_found");

    auto internal = cli.Get("/internal-error");
    ASSERT_TRUE(internal);
    EXPECT_EQ(internal->status, 500);
    auto ebody = nlohmann::json::parse(internal->body);
    EXPECT_EQ(ebody.value("error", ""), "internal_error");

    server.stop();
}

TEST(HttpServerTest, MiddlewareCanShortCircuitRequests) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18091, openai, node);
    server.addMiddleware([](const httplib::Request& req, httplib::Response& res) {
        if (req.has_header("X-Block")) {
            res.status = 403;
            res.set_content("blocked", "text/plain");
            return false;
        }
        return true;
    });
    server.start();

    httplib::Client cli("127.0.0.1", 18091);
    httplib::Headers h{{"X-Block", "1"}};
    auto blocked = cli.Get("/health", h);
    ASSERT_TRUE(blocked);
    EXPECT_EQ(blocked->status, 403);
    EXPECT_EQ(blocked->body, "blocked");

    auto allowed = cli.Get("/health");
    ASSERT_TRUE(allowed);
    EXPECT_EQ(allowed->status, 200);

    server.stop();
}

TEST(HttpServerTest, LoggerReceivesRequests) {
    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18092, openai, node);
    std::atomic<bool> logged{false};
    server.setLogger([&logged](const httplib::Request&, const httplib::Response& res) {
        if (res.status == 200) logged = true;
    });
    server.start();

    httplib::Client cli("127.0.0.1", 18092);
    auto res = cli.Get("/health");
    ASSERT_TRUE(res);
    EXPECT_TRUE(logged.load());

    server.stop();
}
