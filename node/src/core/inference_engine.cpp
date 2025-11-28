#include "core/inference_engine.h"
#include "core/llama_manager.h"
#include "models/ollama_compat.h"
#include "models/model_repair.h"
#include "include/llama.h"

#include <spdlog/spdlog.h>
#include <random>
#include <sstream>
#include <chrono>

namespace ollama_node {

// コンストラクタ
InferenceEngine::InferenceEngine(LlamaManager& manager, OllamaCompat& ollama_compat)
    : manager_(&manager)
    , ollama_compat_(&ollama_compat)
    , repair_(nullptr) {}

// コンストラクタ: ModelRepair を含む完全な依存関係注入
InferenceEngine::InferenceEngine(LlamaManager& manager, OllamaCompat& ollama_compat, ModelRepair& repair)
    : manager_(&manager)
    , ollama_compat_(&ollama_compat)
    , repair_(&repair) {}

// チャットメッセージからプロンプトを構築
std::string InferenceEngine::buildChatPrompt(const std::vector<ChatMessage>& messages) const {
    std::ostringstream oss;

    for (const auto& msg : messages) {
        if (msg.role == "system") {
            oss << "System: " << msg.content << "\n\n";
        } else if (msg.role == "user") {
            oss << "User: " << msg.content << "\n\n";
        } else if (msg.role == "assistant") {
            oss << "Assistant: " << msg.content << "\n\n";
        }
    }

    // アシスタント応答の開始を示す
    oss << "Assistant: ";
    return oss.str();
}

// チャット生成（llama.cpp API使用）
std::string InferenceEngine::generateChat(
    const std::vector<ChatMessage>& messages,
    const std::string& model_name,
    const InferenceParams& params) const {

    // 依存関係が注入されていない場合はスタブモード
    if (!isInitialized()) {
        spdlog::warn("InferenceEngine not initialized, using stub mode");
        if (messages.empty()) return "";
        return "Response to: " + messages.back().content;
    }

    // 1. モデルパス解決
    std::string gguf_path = ollama_compat_->resolveGguf(model_name);
    if (gguf_path.empty()) {
        spdlog::error("Model not found: {}", model_name);
        throw std::runtime_error("Model not found: " + model_name);
    }

    // 2. モデルロード（未ロードの場合）
    if (!manager_->isLoaded(gguf_path)) {
        spdlog::info("Loading model on demand: {}", gguf_path);
        if (!manager_->loadModel(gguf_path)) {
            throw std::runtime_error("Failed to load model: " + gguf_path);
        }
    }

    // 3. コンテキストとモデル取得
    llama_context* ctx = manager_->getContext(gguf_path);
    llama_model* model = manager_->getModel(gguf_path);

    if (!ctx || !model) {
        throw std::runtime_error("Failed to get context/model for: " + gguf_path);
    }

    // 4. プロンプト構築
    std::string prompt = buildChatPrompt(messages);
    spdlog::debug("Prompt: {}", prompt);

    // 5. vocab取得
    const llama_vocab* vocab = llama_model_get_vocab(model);
    if (!vocab) {
        throw std::runtime_error("Failed to get vocab from model");
    }

    // 6. トークン化
    std::vector<llama_token> tokens(prompt.size() + 128);
    int32_t n_tokens = llama_tokenize(
        vocab,
        prompt.c_str(),
        static_cast<int32_t>(prompt.size()),
        tokens.data(),
        static_cast<int32_t>(tokens.size()),
        true,   // add_special (BOS)
        false   // parse_special
    );

    if (n_tokens < 0) {
        // バッファが小さすぎる場合、再割り当て
        tokens.resize(static_cast<size_t>(-n_tokens));
        n_tokens = llama_tokenize(
            vocab,
            prompt.c_str(),
            static_cast<int32_t>(prompt.size()),
            tokens.data(),
            static_cast<int32_t>(tokens.size()),
            true,
            false
        );
    }

    if (n_tokens < 0) {
        throw std::runtime_error("Failed to tokenize prompt");
    }

    tokens.resize(static_cast<size_t>(n_tokens));
    spdlog::debug("Tokenized prompt: {} tokens", n_tokens);

    // 7. バッチ作成とプロンプト処理
    llama_batch batch = llama_batch_get_one(tokens.data(), n_tokens);

    int32_t decode_result = llama_decode(ctx, batch);
    if (decode_result != 0) {
        spdlog::error("llama_decode failed for prompt: {}", decode_result);
        throw std::runtime_error("llama_decode failed");
    }

    // 8. サンプラーチェーン初期化
    llama_sampler_chain_params sparams = llama_sampler_chain_default_params();
    llama_sampler* sampler = llama_sampler_chain_init(sparams);

    // サンプリング戦略を追加
    llama_sampler_chain_add(sampler, llama_sampler_init_top_k(params.top_k));
    llama_sampler_chain_add(sampler, llama_sampler_init_top_p(params.top_p, 1));
    llama_sampler_chain_add(sampler, llama_sampler_init_temp(params.temperature));

    // シード設定
    uint32_t seed = params.seed;
    if (seed == 0) {
        seed = static_cast<uint32_t>(
            std::chrono::steady_clock::now().time_since_epoch().count() & 0xFFFFFFFF);
    }
    llama_sampler_chain_add(sampler, llama_sampler_init_dist(seed));

    // 9. トークン生成ループ
    std::string output;
    int32_t n_cur = n_tokens;

    for (size_t i = 0; i < params.max_tokens; i++) {
        // トークンサンプリング
        llama_token new_token = llama_sampler_sample(sampler, ctx, -1);

        // EOG（End of Generation）チェック
        if (llama_vocab_is_eog(vocab, new_token)) {
            spdlog::debug("EOG token received at position {}", i);
            break;
        }

        // トークンをテキストに変換
        char buf[256];
        int32_t len = llama_token_to_piece(vocab, new_token, buf, sizeof(buf), 0, false);
        if (len > 0) {
            output.append(buf, static_cast<size_t>(len));
        }

        // サンプラーにトークンを通知
        llama_sampler_accept(sampler, new_token);

        // 次のトークン用にバッチを準備
        llama_batch next_batch = llama_batch_get_one(&new_token, 1);
        decode_result = llama_decode(ctx, next_batch);
        if (decode_result != 0) {
            spdlog::warn("llama_decode failed during generation: {}", decode_result);
            break;
        }

        n_cur++;
    }

    // 10. クリーンアップ
    llama_sampler_free(sampler);

    spdlog::info("Generated {} tokens for model {}", output.size(), model_name);
    return output;
}

// テキスト補完
std::string InferenceEngine::generateCompletion(
    const std::string& prompt,
    const std::string& model,
    const InferenceParams& params) const {

    // チャットメッセージとして処理
    std::vector<ChatMessage> messages = {{"user", prompt}};
    return generateChat(messages, model, params);
}

// ストリーミングチャット生成
std::vector<std::string> InferenceEngine::generateChatStream(
    const std::vector<ChatMessage>& messages,
    const std::string& model_name,
    const InferenceParams& params,
    const std::function<void(const std::string&)>& on_token) const {

    std::vector<std::string> all_tokens;

    // 依存関係が注入されていない場合はスタブモード
    if (!isInitialized()) {
        spdlog::warn("InferenceEngine not initialized, using stub mode for streaming");
        std::string text = messages.empty() ? "" : "Response to: " + messages.back().content;
        auto tokens = generateTokens(text, params.max_tokens);
        for (const auto& t : tokens) {
            if (on_token) on_token(t);
        }
        if (on_token) on_token("[DONE]");
        return tokens;
    }

    // 1. モデルパス解決とロード
    std::string gguf_path = ollama_compat_->resolveGguf(model_name);
    if (gguf_path.empty()) {
        throw std::runtime_error("Model not found: " + model_name);
    }

    if (!manager_->isLoaded(gguf_path)) {
        if (!manager_->loadModel(gguf_path)) {
            throw std::runtime_error("Failed to load model: " + gguf_path);
        }
    }

    llama_context* ctx = manager_->getContext(gguf_path);
    llama_model* model = manager_->getModel(gguf_path);

    if (!ctx || !model) {
        throw std::runtime_error("Failed to get context/model");
    }

    // 2. vocab取得とプロンプト処理
    const llama_vocab* vocab = llama_model_get_vocab(model);
    std::string prompt = buildChatPrompt(messages);

    std::vector<llama_token> tokens(prompt.size() + 128);
    int32_t n_tokens = llama_tokenize(
        vocab, prompt.c_str(), static_cast<int32_t>(prompt.size()),
        tokens.data(), static_cast<int32_t>(tokens.size()), true, false);

    if (n_tokens < 0) {
        tokens.resize(static_cast<size_t>(-n_tokens));
        n_tokens = llama_tokenize(
            vocab, prompt.c_str(), static_cast<int32_t>(prompt.size()),
            tokens.data(), static_cast<int32_t>(tokens.size()), true, false);
    }

    tokens.resize(static_cast<size_t>(n_tokens));

    // 3. プロンプトをデコード
    llama_batch batch = llama_batch_get_one(tokens.data(), n_tokens);
    if (llama_decode(ctx, batch) != 0) {
        throw std::runtime_error("llama_decode failed for prompt");
    }

    // 4. サンプラー初期化
    llama_sampler_chain_params sparams = llama_sampler_chain_default_params();
    llama_sampler* sampler = llama_sampler_chain_init(sparams);

    llama_sampler_chain_add(sampler, llama_sampler_init_top_k(params.top_k));
    llama_sampler_chain_add(sampler, llama_sampler_init_top_p(params.top_p, 1));
    llama_sampler_chain_add(sampler, llama_sampler_init_temp(params.temperature));

    uint32_t seed = params.seed;
    if (seed == 0) {
        seed = static_cast<uint32_t>(
            std::chrono::steady_clock::now().time_since_epoch().count() & 0xFFFFFFFF);
    }
    llama_sampler_chain_add(sampler, llama_sampler_init_dist(seed));

    // 5. ストリーミング生成ループ
    for (size_t i = 0; i < params.max_tokens; i++) {
        llama_token new_token = llama_sampler_sample(sampler, ctx, -1);

        if (llama_vocab_is_eog(vocab, new_token)) {
            break;
        }

        char buf[256];
        int32_t len = llama_token_to_piece(vocab, new_token, buf, sizeof(buf), 0, false);
        if (len > 0) {
            std::string piece(buf, static_cast<size_t>(len));
            all_tokens.push_back(piece);

            // コールバックで即座に送信
            if (on_token) {
                on_token(piece);
            }
        }

        llama_sampler_accept(sampler, new_token);

        llama_batch next_batch = llama_batch_get_one(&new_token, 1);
        if (llama_decode(ctx, next_batch) != 0) {
            break;
        }
    }

    // 完了を通知
    if (on_token) {
        on_token("[DONE]");
    }

    llama_sampler_free(sampler);
    return all_tokens;
}

// 旧API互換のストリーミング（[DONE]を送信しない）
std::vector<std::string> InferenceEngine::generateChatStream(
    const std::vector<ChatMessage>& messages,
    size_t max_tokens,
    const std::function<void(const std::string&)>& on_token) const {

    // スタブモード: 旧実装と同じ動作を維持
    std::string text = generateChat(messages, "");
    auto tokens = generateTokens(text, max_tokens);
    for (const auto& t : tokens) {
        if (on_token) on_token(t);
    }
    // 注: 旧APIでは[DONE]を送信しない
    return tokens;
}

// バッチ推論
std::vector<std::vector<std::string>> InferenceEngine::generateBatch(
    const std::vector<std::string>& prompts,
    size_t max_tokens) const {

    std::vector<std::vector<std::string>> outputs;
    outputs.reserve(prompts.size());

    for (const auto& p : prompts) {
        outputs.push_back(generateTokens(p, max_tokens));
    }
    return outputs;
}

// 簡易トークン生成（スペース区切り、互換性維持）
std::vector<std::string> InferenceEngine::generateTokens(
    const std::string& prompt,
    size_t max_tokens) const {

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

    if (!current.empty() && tokens.size() < max_tokens) {
        tokens.push_back(current);
    }

    return tokens;
}

// サンプリング（互換性維持）
std::string InferenceEngine::sampleNextToken(const std::vector<std::string>& tokens) const {
    if (tokens.empty()) return "";
    return tokens.back();
}

// モデルをロードし、必要に応じて自動修復を試行
ModelLoadResult InferenceEngine::loadModelWithRepair(const std::string& model_name) {
    ModelLoadResult result;

    if (!isInitialized()) {
        result.error_message = "InferenceEngine not initialized";
        return result;
    }

    // 1. モデルパス解決
    std::string gguf_path = ollama_compat_->resolveGguf(model_name);
    if (gguf_path.empty()) {
        result.error_message = "Model not found: " + model_name;
        return result;
    }

    // 2. 既にロード済みならそのまま成功
    if (manager_->isLoaded(gguf_path)) {
        result.success = true;
        return result;
    }

    // 3. 自動修復が有効で、ファイルが破損している場合は修復を試みる
    if (repair_ && repair_->needsRepair(gguf_path)) {
        spdlog::info("Model file needs repair, triggering auto-repair: {}", model_name);

        // 既に修復中の場合は待機
        if (repair_->isRepairing(model_name)) {
            spdlog::info("Model {} is already being repaired, waiting...", model_name);
            result.repair_triggered = true;
            bool completed = repair_->waitForRepair(model_name, repair_->getDefaultTimeout());
            if (!completed) {
                result.error_message = "Repair timeout for model: " + model_name;
                return result;
            }
        } else {
            // 新規修復を開始
            result.repair_triggered = true;
            auto repair_result = repair_->repair(model_name, repair_->getDefaultTimeout());
            if (repair_result.status != RepairStatus::Success) {
                result.error_message = "Repair failed: " + repair_result.error_message;
                return result;
            }
        }

        // 修復後にパスを再解決
        gguf_path = ollama_compat_->resolveGguf(model_name);
    }

    // 4. モデルをロード
    if (!manager_->loadModel(gguf_path)) {
        // ロード失敗時、まだ自動修復を試みていなければ試行
        if (repair_ && !result.repair_triggered) {
            spdlog::warn("Model load failed, attempting auto-repair: {}", model_name);
            result.repair_triggered = true;
            auto repair_result = repair_->repair(model_name, repair_->getDefaultTimeout());
            if (repair_result.status == RepairStatus::Success) {
                // 修復成功後に再ロード
                gguf_path = ollama_compat_->resolveGguf(model_name);
                if (manager_->loadModel(gguf_path)) {
                    result.success = true;
                    return result;
                }
            }
        }
        result.error_message = "Failed to load model: " + gguf_path;
        return result;
    }

    result.success = true;
    return result;
}

}  // namespace ollama_node
