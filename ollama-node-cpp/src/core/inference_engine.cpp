#include "core/inference_engine.h"

namespace ollama_node {

std::string InferenceEngine::generateChat(const std::vector<ChatMessage>& messages, const std::string& model) const {
    // 仮実装: 最新ユーザーメッセージに固定レスポンスを返す
    (void)model;
    if (messages.empty()) return "";
    return "Response to: " + messages.back().content;
}

std::string InferenceEngine::generateCompletion(const std::string& prompt, const std::string& model) const {
    (void)model;
    return "Completion for: " + prompt;
}

std::vector<std::string> InferenceEngine::generateTokens(const std::string& prompt, size_t max_tokens) const {
    std::vector<std::string> tokens;
    std::string current;
    for (char c : prompt) {
        if (std::isspace(static_cast<unsigned char>(c))) {
            if (!current.empty()) {
                tokens.push_back(current);
                if (tokens.size() >= max_tokens) break;
                current.clear();
            }
        } else {
            current.push_back(c);
        }
    }
    if (!current.empty() && tokens.size() < max_tokens) tokens.push_back(current);
    return tokens;
}

std::vector<std::string> InferenceEngine::generateChatStream(const std::vector<ChatMessage>& messages,
                                                             size_t max_tokens,
                                                             const std::function<void(const std::string&)>& on_token) const {
    std::string text = generateChat(messages, "");
    auto tokens = generateTokens(text, max_tokens);
    for (const auto& t : tokens) {
        if (on_token) on_token(t);
    }
    return tokens;
}

std::vector<std::vector<std::string>> InferenceEngine::generateBatch(const std::vector<std::string>& prompts,
                                                                     size_t max_tokens) const {
    std::vector<std::vector<std::string>> outputs;
    outputs.reserve(prompts.size());
    for (const auto& p : prompts) {
        outputs.push_back(generateTokens(p, max_tokens));
    }
    return outputs;
}

std::string InferenceEngine::sampleNextToken(const std::vector<std::string>& tokens) const {
    if (tokens.empty()) return "";
    return tokens.back();
}

}  // namespace ollama_node
