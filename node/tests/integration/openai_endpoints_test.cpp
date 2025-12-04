#include <gtest/gtest.h>
#include <httplib.h>

#include "api/http_server.h"
#include "api/openai_endpoints.h"
#include "api/node_endpoints.h"
#include "models/model_registry.h"
#include "core/inference_engine.h"

using namespace llm_node;

TEST(OpenAIEndpointsTest, ListsModelsAndRespondsToChat) {
    ModelRegistry registry;
    registry.setModels({"gpt-oss:7b"});
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18087, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18087);
    auto models = cli.Get("/v1/models");
    ASSERT_TRUE(models);
    EXPECT_EQ(models->status, 200);
    EXPECT_NE(models->body.find("gpt-oss:7b"), std::string::npos);

    std::string body = R"({"model":"gpt-oss:7b","messages":[{"role":"user","content":"hello"}]})";
    auto chat = cli.Post("/v1/chat/completions", body, "application/json");
    ASSERT_TRUE(chat);
    EXPECT_EQ(chat->status, 200);
    EXPECT_NE(chat->body.find("Response to"), std::string::npos);

    server.stop();
}

TEST(OpenAIEndpointsTest, Returns404WhenModelMissing) {
    ModelRegistry registry;
    registry.setModels({"gpt-oss:7b"});
    InferenceEngine engine;
    OpenAIEndpoints openai(registry, engine);
    NodeEndpoints node;
    HttpServer server(18092, openai, node);
    server.start();

    httplib::Client cli("127.0.0.1", 18092);
    std::string body = R"({"model":"missing","prompt":"hello"})";
    auto res = cli.Post("/v1/completions", body, "application/json");
    ASSERT_TRUE(res);
    EXPECT_EQ(res->status, 404);
    EXPECT_NE(res->body.find("model_not_found"), std::string::npos);

    server.stop();
}
