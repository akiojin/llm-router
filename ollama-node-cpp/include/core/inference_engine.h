#pragma once

#include <string>
#include <vector>
#include <functional>

namespace ollama_node {

struct ChatMessage {
    std::string role;
    std::string content;
};

class InferenceEngine {
public:
    std::string generateChat(const std::vector<ChatMessage>& messages, const std::string& model) const;
    std::string generateCompletion(const std::string& prompt, const std::string& model) const;

    // 簡易トークン生成（スペース区切りでmax_tokensまで返す）
    std::vector<std::string> generateTokens(const std::string& prompt, size_t max_tokens = 5) const;

    // ストリーミング: on_token に逐次渡しつつトークン列を返す
    std::vector<std::string> generateChatStream(const std::vector<ChatMessage>& messages,
                                                size_t max_tokens,
                                                const std::function<void(const std::string&)>& on_token) const;

    // バッチ推論: 各プロンプトに対し generateTokens を適用
    std::vector<std::vector<std::string>> generateBatch(const std::vector<std::string>& prompts,
                                                        size_t max_tokens) const;

    // サンプリング戦略（ダミー実装）: 最後のトークンを返す
    std::string sampleNextToken(const std::vector<std::string>& tokens) const;
};

}  // namespace ollama_node
