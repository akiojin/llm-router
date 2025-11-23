#pragma once

#include <httplib.h>
#include <string>
#include <memory>
#include <nlohmann/json.hpp>

namespace ollama_node {

class ModelRegistry;
class InferenceEngine;

class OpenAIEndpoints {
public:
    OpenAIEndpoints(ModelRegistry& registry, InferenceEngine& engine);

    void registerRoutes(httplib::Server& server);

private:
    ModelRegistry& registry_;
    InferenceEngine& engine_;

    static void setJson(httplib::Response& res, const nlohmann::json& body);
    void respondError(httplib::Response& res, int status, const std::string& code, const std::string& message);
    bool validateModel(const std::string& model, httplib::Response& res);
};

}  // namespace ollama_node
