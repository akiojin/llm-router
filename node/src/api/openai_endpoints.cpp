#include "api/openai_endpoints.h"

#include <nlohmann/json.hpp>
#include "models/model_registry.h"
#include "core/inference_engine.h"

namespace ollama_node {

using json = nlohmann::json;

OpenAIEndpoints::OpenAIEndpoints(ModelRegistry& registry, InferenceEngine& engine)
    : registry_(registry), engine_(engine) {}

void OpenAIEndpoints::registerRoutes(httplib::Server& server) {
    server.Get("/v1/models", [this](const httplib::Request&, httplib::Response& res) {
        json body;
        body["object"] = "list";
        body["data"] = json::array();
        for (const auto& id : registry_.listModels()) {
            body["data"].push_back({{"id", id}, {"object", "model"}});
        }
        setJson(res, body);
    });

    server.Post("/v1/chat/completions", [this](const httplib::Request& req, httplib::Response& res) {
        try {
            auto body = json::parse(req.body);
            std::string model = body.value("model", "");
            if (!validateModel(model, res)) return;
            std::vector<ChatMessage> messages;
            if (body.contains("messages")) {
                for (const auto& m : body["messages"]) {
                    messages.push_back({m.value("role", ""), m.value("content", "")});
                }
            }
            bool stream = body.value("stream", false);
            std::string output = engine_.generateChat(messages, model);

            if (stream) {
                res.set_header("Content-Type", "text/event-stream");
                res.set_chunked_content_provider("text/event-stream",
                    [output](size_t offset, httplib::DataSink& sink) {
                        if (offset == 0) {
                            std::string chunk = "data: {\\\"content\\\":\\\"" + output + "\\\"}\\n\\n";
                            sink.write(chunk.data(), chunk.size());
                            std::string done = "data: [DONE]\\n\\n";
                            sink.write(done.data(), done.size());
                            sink.done();
                        }
                        return true;
                    });
                return;
            }

            json resp = {
                {"id", "chatcmpl-1"},
                {"object", "chat.completion"},
                {"choices", json::array({{
                    {"index", 0},
                    {"message", {{"role", "assistant"}, {"content", output}}},
                    {"finish_reason", "stop"}
                }})}
            };
            setJson(res, resp);
        } catch (const ModelRepairingException& e) {
            // モデル修復中は202 Acceptedを返す
            res.status = 202;
            setJson(res, {
                {"status", "repairing"},
                {"message", "Model is being repaired, please retry later"},
                {"model", e.modelName()}
            });
        } catch (const std::exception& e) {
            respondError(res, 400, "bad_request", std::string("error: ") + e.what());
        } catch (...) {
            respondError(res, 400, "bad_request", "invalid JSON body");
        }
    });

    server.Post("/v1/completions", [this](const httplib::Request& req, httplib::Response& res) {
        try {
            auto body = json::parse(req.body);
            std::string model = body.value("model", "");
            if (!validateModel(model, res)) return;
            std::string prompt = body.value("prompt", "");
            std::string output = engine_.generateCompletion(prompt, model);
            json resp = {
                {"id", "cmpl-1"},
                {"object", "text_completion"},
                {"choices", json::array({{{"text", output}, {"index", 0}, {"finish_reason", "stop"}}})}
            };
            setJson(res, resp);
        } catch (const ModelRepairingException& e) {
            // モデル修復中は202 Acceptedを返す
            res.status = 202;
            setJson(res, {
                {"status", "repairing"},
                {"message", "Model is being repaired, please retry later"},
                {"model", e.modelName()}
            });
        } catch (...) {
            respondError(res, 400, "bad_request", "invalid JSON body");
        }
    });

    server.Post("/v1/embeddings", [this](const httplib::Request& req, httplib::Response& res) {
        try {
            auto body = json::parse(req.body);
            std::string model = body.value("model", "");
            if (!validateModel(model, res)) return;
            std::string input = body.contains("input") ? body["input"].dump() : "";
            // ダミー埋め込み（固定長3）
            json resp = {
                {"data", json::array({{{"object", "embedding"}, {"embedding", {1.0, 0.0, -1.0}}, {"index", 0}}})},
                {"model", body.value("model", "")},
                {"usage", {{"prompt_tokens", static_cast<int>(input.size())}, {"total_tokens", static_cast<int>(input.size())}}}
            };
            setJson(res, resp);
        } catch (...) {
            respondError(res, 400, "bad_request", "invalid JSON body");
        }
    });
}

void OpenAIEndpoints::setJson(httplib::Response& res, const nlohmann::json& body) {
    res.set_content(body.dump(), "application/json");
}

void OpenAIEndpoints::respondError(httplib::Response& res, int status, const std::string& code, const std::string& message) {
    res.status = status;
    setJson(res, {{"error", code}, {"message", message}});
}

bool OpenAIEndpoints::validateModel(const std::string& model, httplib::Response& res) {
    if (model.empty()) {
        respondError(res, 400, "model_required", "model is required");
        return false;
    }
    if (!registry_.hasModel(model)) {
        respondError(res, 404, "model_not_found", "model not found");
        return false;
    }
    return true;
}

}  // namespace ollama_node
