#include "core/inference_engine.h"
#include "core/llama_manager.h"
#include "models/model_storage.h"
#include "include/llama.h"

#include <spdlog/spdlog.h>
#include <random>
#include <sstream>
#include <chrono>

namespace llm_node {

// 前方宣言
static std::string stripControlTokens(std::string text);
static std::string extractGptOssFinalMessage(const std::string& output);
std::string extractGptOssFinalMessageForTest(const std::string& output);

// コンストラクタ
InferenceEngine::InferenceEngine(LlamaManager& manager, ModelStorage& model_storage)
    : manager_(&manager)
    , model_storage_(&model_storage) {}

// チャットメッセージからプロンプトを構築（llama_chat_apply_template使用）
std::string InferenceEngine::buildChatPrompt(const std::vector<ChatMessage>& messages) const {
    // この関数はモデルなしで呼ばれる互換性維持用のフォールバック
    // 実際の推論では generateChat/generateChatStream 内で直接テンプレートを適用
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

// ChatML形式でプロンプトを構築するフォールバック関数
static std::string buildChatMLPrompt(const std::vector<ChatMessage>& messages) {
    std::ostringstream oss;
    for (const auto& msg : messages) {
        oss << "<|im_start|>" << msg.role << "\n" << msg.content << "<|im_end|>\n";
    }
    // アシスタント応答の開始
    oss << "<|im_start|>assistant\n";
    return oss.str();
}

// 制御トークンを除去してトリム
static std::string stripControlTokens(std::string text) {
    const std::vector<std::string> tokens = {
        "<|start|>", "<|end|>", "<|message|>", "<|channel|>",
        "<|im_start|>", "<|im_end|>", "<s>", "</s>", "<|endoftext|>", "<|eot_id|>"
    };
    for (const auto& t : tokens) {
        size_t pos = 0;
        while ((pos = text.find(t, pos)) != std::string::npos) {
            text.erase(pos, t.size());
        }
    }
    auto l = text.find_first_not_of(" \t\n\r");
    if (l == std::string::npos) return "";
    auto r = text.find_last_not_of(" \t\n\r");
    return text.substr(l, r - l + 1);
}

// gpt-ossテンプレート（モデル側にテンプレが無い場合のフォールバック）。ユーザー入力は改変しない。
static const char * GPT_OSS_TEMPLATE = R"tmpl({% for message in messages %}
{% if message['role'] == 'system' %}
<|start|>system<|message|>{{ message['content'] }}<|end|>
{% elif message['role'] == 'user' %}
<|start|>user<|message|>{{ message['content'] }}<|end|>
{% elif message['role'] == 'assistant' %}
<|start|>assistant<|channel|>final<|message|>{{ message['content'] }}<|end|>
{% endif %}
{% endfor %}
<|start|>assistant<|channel|>final<|message|>
)tmpl";

// gpt-oss: finalチャンネルだけを抽出して制御トークンを除去
static std::string extractGptOssFinalMessage(const std::string& output) {
    const std::string marker = "<|channel|>final<|message|>";
    const std::string end = "<|end|>";

    size_t mpos = output.rfind(marker);
    if (mpos == std::string::npos) return output;
    size_t start = mpos + marker.size();
    size_t endpos = output.find(end, start);
    std::string seg = endpos == std::string::npos ? output.substr(start) : output.substr(start, endpos - start);
    return stripControlTokens(seg);
}

// テスト用に公開する薄いラッパー（本番コードには影響なし）
std::string extractGptOssFinalMessageForTest(const std::string& output) {
    return extractGptOssFinalMessage(output);
}

// gpt-oss形式でプロンプトを構築する関数
// gpt-oss固有トークン: <|start|>, <|message|>, <|end|>, <|channel|>
// 応答形式: <|start|>assistant<|channel|>final<|message|>content<|end|>
// Reasoning: none を設定して推論チャンネルを無効化
static std::string buildGptOssPrompt(const std::vector<ChatMessage>& messages) {
    std::ostringstream oss;

    // システムメッセージの有無をチェック
    bool hasSystemMessage = false;
    for (const auto& msg : messages) {
        if (msg.role == "system") {
            hasSystemMessage = true;
            break;
        }
    }

    // システムメッセージがない場合、推論無効のシステムプロンプトを追加
    if (!hasSystemMessage) {
        oss << "<|start|>system<|message|>You are a helpful assistant.\n\nReasoning: none<|end|>";
    }

    for (const auto& msg : messages) {
        if (msg.role == "system") {
            // システムメッセージに推論設定を追加
            oss << "<|start|>system<|message|>" << msg.content << "\n\nReasoning: none<|end|>";
        } else {
            oss << "<|start|>" << msg.role << "<|message|>" << msg.content << "<|end|>";
        }
    }

    // アシスタント応答の開始（final チャンネルでコンテンツを直接生成）
    oss << "<|start|>assistant<|channel|>final<|message|>";
    return oss.str();
}

// gpt-ossモデルの出力から特殊トークンを除去する後処理関数
static std::string cleanGptOssOutput(const std::string& output) {
    std::string result = output;

    // gpt-ossおよびChatMLの特殊トークンリスト
    const std::vector<std::string> tokens_to_remove = {
        // gpt-oss tokens
        "<|start|>", "<|end|>", "<|message|>", "<|channel|>",
        "<|startoftext|>", "<|endoftext|>", "<|return|>", "<|call|>",
        "<|constrain|>", "<|endofprompt|>",
        // ChatML tokens
        "<|im_start|>", "<|im_end|>", "<|assistant>", "<|user>", "<|system>",
        // Common control tokens
        "<|eot_id|>", "</s>", "<s>", "<|begin_of_text|>", "<|end_of_text|>"
    };

    // 特殊トークンを除去
    for (const auto& token : tokens_to_remove) {
        size_t pos = 0;
        while ((pos = result.find(token, pos)) != std::string::npos) {
            result.erase(pos, token.length());
        }
    }

    // "to=" パターンを全て除去（例: "to=assistant", "to=You", "to=user"）
    // 正規表現的に "to=" + 英数字列 を除去
    {
        size_t pos = 0;
        while ((pos = result.find("to=", pos)) != std::string::npos) {
            size_t end_pos = pos + 3;  // "to=" の後ろ
            // 英数字とアンダースコアが続く間は除去対象
            while (end_pos < result.size() &&
                   (std::isalnum(static_cast<unsigned char>(result[end_pos])) ||
                    result[end_pos] == '_')) {
                end_pos++;
            }
            result.erase(pos, end_pos - pos);
        }
    }

    // チャンネル名やロール名を含むパターンを除去
    // 例: "assistantanalysis:", "analysis:", "final:", "assistantfinal:", etc.
    const std::vector<std::string> channel_patterns = {
        // 連結パターン（優先度高）
        "assistantanalysis:", "assistantfinal:", "assistantcommentary:",
        "useranalysis:", "userfinal:", "usercommentary:",
        "systemanalysis:", "systemfinal:", "systemcommentary:",
        // 単独パターン
        "analysis:", "final:", "commentary:",
        "assistant:", "user:", "system:", "developer:",
        // "=name" パターン
        "=assistant", "=analysis", "=final", "=commentary",
        "=user", "=system", "=developer"
    };
    for (const auto& pattern : channel_patterns) {
        size_t pos = 0;
        while ((pos = result.find(pattern, pos)) != std::string::npos) {
            result.erase(pos, pattern.length());
        }
    }

    // 行頭のチャンネル名（コロンなし）を除去
    const std::vector<std::string> channel_names = {
        "assistant", "analysis", "final", "commentary", "user", "system", "developer"
    };
    for (const auto& name : channel_names) {
        // 行頭の "name\n" パターン
        std::string line_pattern = "\n" + name + "\n";
        size_t pos = 0;
        while ((pos = result.find(line_pattern, pos)) != std::string::npos) {
            result.erase(pos + 1, name.length() + 1);  // 最初の\nは残す
        }
        // 文字列先頭の場合
        if (result.find(name + "\n") == 0) {
            result.erase(0, name.length() + 1);
        }
    }

    // 先頭と末尾の空白を除去
    size_t start = result.find_first_not_of(" \t\n\r");
    if (start == std::string::npos) {
        return "";
    }
    size_t end = result.find_last_not_of(" \t\n\r");
    return result.substr(start, end - start + 1);
}

// モデルがgpt-oss形式かどうかを判定
// モデルのテンプレートやアーキテクチャから判定する
static bool isGptOssModel(llama_model* model) {
    // 1. アーキテクチャ名で判定（最も確実）
    char arch_buf[64] = {0};
    int arch_len = llama_model_meta_val_str(model, "general.architecture", arch_buf, sizeof(arch_buf));
    spdlog::info("isGptOssModel: arch_len={}, arch_buf='{}'", arch_len, arch_buf);
    if (arch_len > 0) {
        std::string arch(arch_buf);
        spdlog::info("isGptOssModel: checking architecture '{}'", arch);
        if (arch == "gptoss") {
            spdlog::info("Detected gpt-oss model by architecture: {}", arch);
            return true;
        }
    }

    // 2. チャットテンプレートにgpt-oss固有トークンが含まれているかチェック
    const char* tmpl = llama_model_chat_template(model, nullptr);
    spdlog::info("isGptOssModel: chat_template={}", tmpl != nullptr ? tmpl : "(null)");
    if (tmpl != nullptr && tmpl[0] != '\0') {
        std::string template_str(tmpl);
        if (template_str.find("<|start|>") != std::string::npos ||
            template_str.find("<|message|>") != std::string::npos) {
            spdlog::info("Detected gpt-oss model by chat template tokens");
            return true;
        }
    }

    spdlog::info("isGptOssModel: not detected as gpt-oss");
    return false;
}

// モデル固有のチャットテンプレートを適用してプロンプトを構築
static std::string applyModelChatTemplate(
    llama_model* model,
    const std::vector<ChatMessage>& messages) {

    // gpt-ossモデルの場合はgpt-oss専用形式を使用
    if (isGptOssModel(model)) {
        spdlog::info("Detected gpt-oss model, using gpt-oss chat format");
        return buildGptOssPrompt(messages);
    }

    // llama_chat_message 配列を構築
    std::vector<llama_chat_message> llama_messages;
    llama_messages.reserve(messages.size());
    for (const auto& msg : messages) {
        llama_messages.push_back({msg.role.c_str(), msg.content.c_str()});
    }

    // モデルからチャットテンプレートを取得
    const char* tmpl = llama_model_chat_template(model, nullptr);

    // テンプレートがない場合はgpt-oss用テンプレかChatMLにフォールバック
    if (tmpl == nullptr || tmpl[0] == '\0') {
        if (isGptOssModel(model)) {
            spdlog::info("Model has no chat template, using built-in gpt-oss template");
            tmpl = GPT_OSS_TEMPLATE;
        } else {
            spdlog::info("Model has no chat template, using ChatML format");
            return buildChatMLPrompt(messages);
        }
    }

    spdlog::debug("Model chat template found: {}", tmpl);

    // 初回呼び出しで必要なバッファサイズを取得
    int32_t required_size = llama_chat_apply_template(
        tmpl,
        llama_messages.data(),
        llama_messages.size(),
        true,  // add_ass: アシスタント応答の開始を追加
        nullptr,
        0);

    if (required_size < 0) {
        // テンプレート適用に失敗した場合、ChatML形式にフォールバック
        spdlog::warn("llama_chat_apply_template failed (size={}), using ChatML fallback", required_size);
        return buildChatMLPrompt(messages);
    }

    // バッファを確保してテンプレートを適用
    std::vector<char> buf(static_cast<size_t>(required_size) + 1);
    int32_t actual_size = llama_chat_apply_template(
        tmpl,
        llama_messages.data(),
        llama_messages.size(),
        true,
        buf.data(),
        static_cast<int32_t>(buf.size()));

    if (actual_size < 0 || actual_size > static_cast<int32_t>(buf.size())) {
        spdlog::error("llama_chat_apply_template failed on second call");
        // ChatML形式にフォールバック
        return buildChatMLPrompt(messages);
    }

    std::string prompt(buf.data(), static_cast<size_t>(actual_size));
    spdlog::debug("Applied chat template: {} chars", prompt.size());
    return prompt;
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

    // 1. モデルパス解決（固定ディレクトリのみを許容）
    std::string gguf_path = model_storage_->resolveGguf(model_name);
    if (gguf_path.empty()) {
        spdlog::error("Model not found in ~/.llm-router/models: {}", model_name);
        throw std::runtime_error("Model not found in ~/.llm-router/models: " + model_name);
    }

    // 2. モデルロード（オンデマンドロードのみ。Ollama blob 等へのフォールバックはしない）
    if (!manager_->loadModelIfNeeded(gguf_path)) {
        throw std::runtime_error("Failed to load model: " + gguf_path);
    }

    // 3. コンテキストとモデル取得
    llama_context* ctx = manager_->getContext(gguf_path);
    llama_model* model = manager_->getModel(gguf_path);

    if (!ctx || !model) {
        throw std::runtime_error("Failed to get context/model for: " + gguf_path);
    }

    // 4. プロンプト構築（モデル固有のチャットテンプレートを使用）
    std::string prompt = applyModelChatTemplate(model, messages);
    spdlog::debug("Prompt: {}", prompt);

    // 5. vocab取得
    const llama_vocab* vocab = llama_model_get_vocab(model);
    if (!vocab) {
        throw std::runtime_error("Failed to get vocab from model");
    }

    // 6. トークン化
    // gpt-ossモデルはadd_bos_token=falseを指定しているため、
    // add_special=falseに設定。parse_special=trueで特殊トークンを認識させる。
    bool is_gptoss = isGptOssModel(model);
    bool add_special = !is_gptoss;  // gpt-oss以外はBOS追加
    bool parse_special = is_gptoss; // gpt-ossは特殊トークンをパース

    std::vector<llama_token> tokens(prompt.size() + 128);
    int32_t n_tokens = llama_tokenize(
        vocab,
        prompt.c_str(),
        static_cast<int32_t>(prompt.size()),
        tokens.data(),
        static_cast<int32_t>(tokens.size()),
        add_special,
        parse_special
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
            add_special,
            parse_special
        );
    }

    if (n_tokens < 0) {
        throw std::runtime_error("Failed to tokenize prompt");
    }

    tokens.resize(static_cast<size_t>(n_tokens));
    spdlog::debug("Tokenized prompt: {} tokens", n_tokens);

    // 7. バッチ分割処理でプロンプトをデコード
    const int32_t batch_size = llama_n_batch(ctx);
    spdlog::debug("Decoding prompt with {} tokens in batches of {}", n_tokens, batch_size);

    for (int32_t i = 0; i < n_tokens; i += batch_size) {
        int32_t current_batch_size = std::min(batch_size, n_tokens - i);
        llama_batch batch = llama_batch_get_one(tokens.data() + i, current_batch_size);

        int32_t decode_result = llama_decode(ctx, batch);
        if (decode_result != 0) {
            spdlog::error("llama_decode failed at batch {}/{}: n_tokens={}, batch_size={}, error={}",
                i / batch_size + 1, (n_tokens + batch_size - 1) / batch_size,
                n_tokens, batch_size, decode_result);
            throw std::runtime_error("llama_decode failed");
        }
    }

    // 8. サンプラーチェーン初期化
    llama_sampler_chain_params sparams = llama_sampler_chain_default_params();
    llama_sampler* sampler = llama_sampler_chain_init(sparams);

    // サンプリング戦略を追加
    llama_sampler_chain_add(sampler, llama_sampler_init_top_k(params.top_k));
    llama_sampler_chain_add(sampler, llama_sampler_init_top_p(params.top_p, 1));
    llama_sampler_chain_add(sampler, llama_sampler_init_temp(params.temperature));

    // 繰り返し抑制ペナルティを追加（重要：反復出力を防ぐ）
    llama_sampler_chain_add(sampler, llama_sampler_init_penalties(
        64,                      // last_n: 直近64トークンを考慮
        params.repeat_penalty,   // repeat_penalty: 1.1
        0.0f,                    // frequency_penalty
        0.0f                     // presence_penalty
    ));

    // シード設定
    uint32_t seed = params.seed;
    if (seed == 0) {
        seed = static_cast<uint32_t>(
            std::chrono::steady_clock::now().time_since_epoch().count() & 0xFFFFFFFF);
    }
    llama_sampler_chain_add(sampler, llama_sampler_init_dist(seed));

    // 9. トークン生成ループ
    std::string output;
    // int32_t n_cur = n_tokens; // unused

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
            // Debug: log token ID and raw bytes
            std::string hex_bytes;
            for (int32_t j = 0; j < len; j++) {
                char hex[8];
                snprintf(hex, sizeof(hex), "%02X ", static_cast<unsigned char>(buf[j]));
                hex_bytes += hex;
            }
            spdlog::debug("Token {}: id={}, len={}, bytes=[{}]", i, new_token, len, hex_bytes);
            output.append(buf, static_cast<size_t>(len));
        }

        // サンプラーにトークンを通知
        llama_sampler_accept(sampler, new_token);

        // 次のトークン用にバッチを準備
        llama_batch next_batch = llama_batch_get_one(&new_token, 1);
        int32_t gen_decode_result = llama_decode(ctx, next_batch);
        if (gen_decode_result != 0) {
            spdlog::warn("llama_decode failed during generation: {}", gen_decode_result);
            break;
        }

        // n_cur++; // unused
    }

    // 10. クリーンアップ
    llama_sampler_free(sampler);

    // 11. 出力の後処理: chatMLテンプレートのストップトークンで切り詰め
    // Qwen3などのモデルは<|im_end|>で応答を終了するが、EOGとして認識されない場合がある
    static const std::vector<std::string> stop_sequences = {
        "<|im_end|>",       // ChatML (Qwen3, etc.)
        "<|end|>",          // gpt-oss, Some models
        "<|start|>",        // gpt-oss (新しいメッセージの開始を検出)
        "<|eot_id|>",       // Llama 3
        "</s>",             // Llama 2, Mistral
        "<|endoftext|>",    // GPT-style
    };

    for (const auto& stop : stop_sequences) {
        size_t pos = output.find(stop);
        if (pos != std::string::npos) {
            spdlog::debug("Truncating output at stop sequence '{}' at position {}", stop, pos);
            output = output.substr(0, pos);
            break;
        }
    }

    // 12. gpt-ossモデルの場合は特殊トークンを除去する後処理を適用
    if (isGptOssModel(model)) {
        spdlog::info("Applying gpt-oss output cleanup, before: {} chars", output.size());
        output = cleanGptOssOutput(output);
        spdlog::info("After cleanup: {} chars", output.size());
    }

    // Debug: log final output hex dump (first 100 bytes)
    std::string hex_output;
    for (size_t j = 0; j < std::min(output.size(), size_t(100)); j++) {
        char hex[8];
        snprintf(hex, sizeof(hex), "%02X ", static_cast<unsigned char>(output[j]));
        hex_output += hex;
    }
    spdlog::info("Generated {} bytes for model {}, first 100 bytes: [{}]", output.size(), model_name, hex_output);
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

    // 1. モデルパス解決（固定ディレクトリのみ）
    std::string gguf_path = model_storage_->resolveGguf(model_name);
    if (gguf_path.empty()) {
        throw std::runtime_error("Model not found in ~/.llm-router/models: " + model_name);
    }

    // 2. モデルロード（フォールバックなし）
    if (!manager_->loadModelIfNeeded(gguf_path)) {
        throw std::runtime_error("Failed to load model: " + gguf_path);
    }

    llama_context* ctx = manager_->getContext(gguf_path);
    llama_model* model = manager_->getModel(gguf_path);

    if (!ctx || !model) {
        throw std::runtime_error("Failed to get context/model");
    }

    // 3. vocab取得とプロンプト処理（モデル固有のチャットテンプレートを使用）
    const llama_vocab* vocab = llama_model_get_vocab(model);
    std::string prompt = applyModelChatTemplate(model, messages);

    // gpt-ossモデルはadd_bos_token=falseを指定しているため、
    // add_special=falseに設定。parse_special=trueで特殊トークンを認識させる。
    bool is_gptoss = isGptOssModel(model);
    bool add_special = !is_gptoss;  // gpt-oss以外はBOS追加
    bool parse_special = is_gptoss; // gpt-ossは特殊トークンをパース

    std::vector<llama_token> tokens(prompt.size() + 128);
    int32_t n_tokens = llama_tokenize(
        vocab, prompt.c_str(), static_cast<int32_t>(prompt.size()),
        tokens.data(), static_cast<int32_t>(tokens.size()), add_special, parse_special);

    if (n_tokens < 0) {
        tokens.resize(static_cast<size_t>(-n_tokens));
        n_tokens = llama_tokenize(
            vocab, prompt.c_str(), static_cast<int32_t>(prompt.size()),
            tokens.data(), static_cast<int32_t>(tokens.size()), add_special, parse_special);
    }

    tokens.resize(static_cast<size_t>(n_tokens));

    // 4. バッチ分割処理でプロンプトをデコード
    const int32_t batch_size = llama_n_batch(ctx);
    spdlog::debug("Streaming: Decoding prompt with {} tokens in batches of {}", n_tokens, batch_size);

    for (int32_t i = 0; i < n_tokens; i += batch_size) {
        int32_t current_batch_size = std::min(batch_size, n_tokens - i);
        llama_batch batch = llama_batch_get_one(tokens.data() + i, current_batch_size);

        if (llama_decode(ctx, batch) != 0) {
            spdlog::error("llama_decode failed at batch {}/{}: n_tokens={}, batch_size={}",
                i / batch_size + 1, (n_tokens + batch_size - 1) / batch_size,
                n_tokens, batch_size);
            throw std::runtime_error("llama_decode failed for prompt");
        }
    }

    // 5. サンプラー初期化
    llama_sampler_chain_params sparams = llama_sampler_chain_default_params();
    llama_sampler* sampler = llama_sampler_chain_init(sparams);

    llama_sampler_chain_add(sampler, llama_sampler_init_top_k(params.top_k));
    llama_sampler_chain_add(sampler, llama_sampler_init_top_p(params.top_p, 1));
    llama_sampler_chain_add(sampler, llama_sampler_init_temp(params.temperature));

    // 繰り返し抑制ペナルティを追加（重要：反復出力を防ぐ）
    llama_sampler_chain_add(sampler, llama_sampler_init_penalties(
        64,                      // last_n: 直近64トークンを考慮
        params.repeat_penalty,   // repeat_penalty: 1.1
        0.0f,                    // frequency_penalty
        0.0f                     // presence_penalty
    ));

    uint32_t seed = params.seed;
    if (seed == 0) {
        seed = static_cast<uint32_t>(
            std::chrono::steady_clock::now().time_since_epoch().count() & 0xFFFFFFFF);
    }
    llama_sampler_chain_add(sampler, llama_sampler_init_dist(seed));

    // 6. ストリーミング生成ループ
    // ストップシーケンスの定義（chatMLテンプレート用）
    static const std::vector<std::string> stop_sequences = {
        "<|im_end|>",       // ChatML (Qwen3, etc.)
        "<|end|>",          // gpt-oss, Some models
        "<|start|>",        // gpt-oss (新しいメッセージの開始を検出)
        "<|eot_id|>",       // Llama 3
        "</s>",             // Llama 2, Mistral
        "<|endoftext|>",    // GPT-style
    };

    std::string accumulated_output;  // ストップシーケンス検出用の累積出力
    bool should_stop = false;

    for (size_t i = 0; i < params.max_tokens && !should_stop; i++) {
        llama_token new_token = llama_sampler_sample(sampler, ctx, -1);

        if (llama_vocab_is_eog(vocab, new_token)) {
            break;
        }

        char buf[256];
        int32_t len = llama_token_to_piece(vocab, new_token, buf, sizeof(buf), 0, false);
        if (len > 0) {
            std::string piece(buf, static_cast<size_t>(len));
            accumulated_output += piece;

            // ストップシーケンスのチェック
            for (const auto& stop : stop_sequences) {
                size_t pos = accumulated_output.find(stop);
                if (pos != std::string::npos) {
                    spdlog::debug("Streaming: found stop sequence '{}' at position {}", stop, pos);
                    // ストップシーケンス前の部分のみを送信
                    if (pos > 0 && pos > accumulated_output.size() - piece.size()) {
                        // 現在のピースがストップシーケンスを含む場合、その前の部分のみ送信
                        std::string partial = piece.substr(0, pos - (accumulated_output.size() - piece.size()));
                        if (!partial.empty() && on_token) {
                            on_token(partial);
                            all_tokens.push_back(partial);
                        }
                    } else if (pos == 0 || accumulated_output.find(stop) >= accumulated_output.size() - piece.size()) {
                        // ストップシーケンスがこのピースで始まる場合、送信しない
                    } else {
                        all_tokens.push_back(piece);
                        if (on_token) {
                            on_token(piece);
                        }
                    }
                    should_stop = true;
                    break;
                }
            }

            if (!should_stop) {
                all_tokens.push_back(piece);
                // コールバックで即座に送信
                if (on_token) {
                    on_token(piece);
                }
            }
        }

        if (!should_stop) {
            llama_sampler_accept(sampler, new_token);

            llama_batch next_batch = llama_batch_get_one(&new_token, 1);
            if (llama_decode(ctx, next_batch) != 0) {
                break;
            }
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

// モデルをロード（ローカルまたは共有パスから解決）
ModelLoadResult InferenceEngine::loadModel(const std::string& model_name) {
    ModelLoadResult result;

    if (!isInitialized()) {
        result.error_message = "InferenceEngine not initialized";
        return result;
    }

    // 1. モデルパス解決（ローカル or 共有パス）
    std::string gguf_path = model_storage_->resolveGguf(model_name);
    if (gguf_path.empty()) {
        result.error_message = "Model not found: " + model_name;
        return result;
    }

    // 2. 既にロード済みならそのまま成功
    if (manager_->isLoaded(gguf_path)) {
        result.success = true;
        return result;
    }

    // 3. モデルをロード
    if (!manager_->loadModelIfNeeded(gguf_path)) {
        result.error_message = "Failed to load model: " + gguf_path;
        return result;
    }

    result.success = true;
    return result;
}

}  // namespace llm_node
