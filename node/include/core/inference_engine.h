#pragma once

#include <string>
#include <vector>
#include <functional>
#include <memory>
#include <stdexcept>

namespace llm_node {

// 前方宣言
class LlamaManager;
class ModelStorage;

struct ChatMessage {
    std::string role;
    std::string content;
};

/// 推論パラメータ
struct InferenceParams {
    size_t max_tokens{256};
    float temperature{0.8f};
    float top_p{0.9f};
    int top_k{40};
    float repeat_penalty{1.1f};
    uint32_t seed{0};  // 0 = ランダム
};

/// モデルロード結果
struct ModelLoadResult {
    bool success{false};
    std::string error_message;
};

class InferenceEngine {
public:
    /// コンストラクタ: LlamaManager と ModelStorage への参照を注入
    InferenceEngine(LlamaManager& manager, ModelStorage& model_storage);

    /// デフォルトコンストラクタ（互換性維持、スタブモード）
    InferenceEngine() = default;

    /// チャット生成（llama.cpp API使用）
    std::string generateChat(const std::vector<ChatMessage>& messages,
                            const std::string& model,
                            const InferenceParams& params = {}) const;

    /// テキスト補完
    std::string generateCompletion(const std::string& prompt,
                                   const std::string& model,
                                   const InferenceParams& params = {}) const;

    /// ストリーミングチャット生成
    /// on_token コールバックは各トークン生成時に呼ばれる
    /// 完了時は "[DONE]" を送信
    std::vector<std::string> generateChatStream(
        const std::vector<ChatMessage>& messages,
        const std::string& model,
        const InferenceParams& params,
        const std::function<void(const std::string&)>& on_token) const;

    /// 旧API互換（max_tokens のみ指定）
    std::vector<std::string> generateChatStream(
        const std::vector<ChatMessage>& messages,
        size_t max_tokens,
        const std::function<void(const std::string&)>& on_token) const;

    /// バッチ推論（複数プロンプトを処理）
    std::vector<std::vector<std::string>> generateBatch(
        const std::vector<std::string>& prompts,
        size_t max_tokens) const;

    /// 簡易トークン生成（スペース区切り、互換性維持）
    std::vector<std::string> generateTokens(const std::string& prompt,
                                            size_t max_tokens = 5) const;

    /// サンプリング（互換性維持）
    std::string sampleNextToken(const std::vector<std::string>& tokens) const;

    /// 依存関係が注入されているか確認
    bool isInitialized() const { return manager_ != nullptr && model_storage_ != nullptr; }

    /// モデルをロード（ローカルまたは共有パスから解決）
    /// @return ロード結果（成功/失敗）
    ModelLoadResult loadModel(const std::string& model_name);

private:
    LlamaManager* manager_{nullptr};
    ModelStorage* model_storage_{nullptr};

    /// チャットメッセージからプロンプト文字列を構築
    std::string buildChatPrompt(const std::vector<ChatMessage>& messages) const;
};

}  // namespace llm_node
