#include <gtest/gtest.h>
#include <httplib.h>
#include <nlohmann/json.hpp>
#include <thread>

#include "api/http_server.h"
#include "api/openai_endpoints.h"
#include "api/node_endpoints.h"
#include "models/model_registry.h"
#include "core/inference_engine.h"

using namespace llm_node;
using json = nlohmann::json;

class OpenAIContractFixture : public ::testing::Test {
protected:
    void SetUp() override {
        registry.setModels({"gpt-oss:7b"});
        server = std::make_unique<HttpServer>(18090, openai, node);
        server->start();
    }

    void TearDown() override {
        server->stop();
    }

    ModelRegistry registry;
    InferenceEngine engine;
    OpenAIEndpoints openai{registry, engine};
    NodeEndpoints node;
    std::unique_ptr<HttpServer> server;
};

TEST_F(OpenAIContractFixture, ModelsEndpointReturnsArray) {
    httplib::Client cli("127.0.0.1", 18090);
    auto res = cli.Get("/v1/models");
    ASSERT_TRUE(res);
    EXPECT_EQ(res->status, 200);
    auto body = json::parse(res->body);
    ASSERT_TRUE(body.contains("data"));
    EXPECT_FALSE(body["data"].empty());
}

TEST_F(OpenAIContractFixture, ChatCompletionsReturnsMessage) {
    httplib::Client cli("127.0.0.1", 18090);
    std::string body = R"({"model":"gpt-oss:7b","messages":[{"role":"user","content":"ping"}]})";
    auto res = cli.Post("/v1/chat/completions", body, "application/json");
    ASSERT_TRUE(res);
    EXPECT_EQ(res->status, 200);
    auto j = json::parse(res->body);
    EXPECT_EQ(j["object"], "chat.completion");
    EXPECT_EQ(j["choices"][0]["message"]["role"], "assistant");
}

TEST_F(OpenAIContractFixture, ChatCompletionsSupportsStreamingSSE) {
    httplib::Client cli("127.0.0.1", 18090);
    std::string body = R"({"model":"gpt-oss:7b","messages":[{"role":"user","content":"stream"}],"stream":true})";
    auto res = cli.Post("/v1/chat/completions", body, "application/json");
    ASSERT_TRUE(res);
    EXPECT_EQ(res->status, 200);
    // SSE should include data: prefix
    EXPECT_NE(res->body.find("data:"), std::string::npos);
    EXPECT_NE(res->body.find("[DONE]"), std::string::npos);
    EXPECT_EQ(res->get_header_value("Content-Type"), "text/event-stream");
}

