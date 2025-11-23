#include <gtest/gtest.h>

#include "core/inference_engine.h"

using namespace ollama_node;

TEST(InferenceEngineTest, GeneratesChatFromLastUserMessage) {
    InferenceEngine engine;
    std::vector<ChatMessage> msgs = {
        {"system", "You are a bot."},
        {"user", "Hello"},
        {"assistant", "Hi"},
        {"user", "How are you?"},
    };
    auto out = engine.generateChat(msgs, "dummy");
    EXPECT_NE(out.find("How are you?"), std::string::npos);
}

TEST(InferenceEngineTest, GeneratesCompletionFromPrompt) {
    InferenceEngine engine;
    auto out = engine.generateCompletion("Once upon a time", "dummy");
    EXPECT_NE(out.find("Once upon a time"), std::string::npos);
}

TEST(InferenceEngineTest, GeneratesTokensWithLimit) {
    InferenceEngine engine;
    auto tokens = engine.generateTokens("a b c d e f", 3);
    ASSERT_EQ(tokens.size(), 3u);
    EXPECT_EQ(tokens[0], "a");
    EXPECT_EQ(tokens[2], "c");
}

TEST(InferenceEngineTest, StreamsChatTokens) {
    InferenceEngine engine;
    std::vector<std::string> collected;
    std::vector<ChatMessage> msgs = {{"user", "hello stream test"}};
    auto tokens = engine.generateChatStream(msgs, 2, [&](const std::string& t) { collected.push_back(t); });
    ASSERT_EQ(tokens.size(), 2u);
    EXPECT_EQ(collected, tokens);
}

TEST(InferenceEngineTest, BatchGeneratesPerPrompt) {
    InferenceEngine engine;
    std::vector<std::string> prompts = {"one two", "alpha beta gamma"};
    auto outs = engine.generateBatch(prompts, 2);
    ASSERT_EQ(outs.size(), 2u);
    EXPECT_EQ(outs[0][0], "one");
    EXPECT_EQ(outs[1][1], "beta");
}

TEST(InferenceEngineTest, SampleNextTokenReturnsLast) {
    InferenceEngine engine;
    std::vector<std::string> tokens = {"x", "y", "z"};
    EXPECT_EQ(engine.sampleNextToken(tokens), "z");
}
