#pragma once

#include <httplib.h>
#include <thread>
#include <atomic>
#include <vector>
#include <functional>

namespace llm_node {

class OpenAIEndpoints;
class NodeEndpoints;

using Middleware = std::function<bool(const httplib::Request&, httplib::Response&)>;
using Logger = std::function<void(const httplib::Request&, const httplib::Response&)>;

class HttpServer {
public:
    HttpServer(int port, OpenAIEndpoints& openai, NodeEndpoints& node, std::string bind_address = "0.0.0.0");
    ~HttpServer();

    void start();
    void stop();

    void addMiddleware(Middleware mw);
    void enableCors(bool enable) { enable_cors_ = enable; }
    void setCorsOrigin(std::string origin) { cors_allow_origin_ = std::move(origin); }
    void setCorsMethods(std::string methods) { cors_allow_methods_ = std::move(methods); }
    void setCorsHeaders(std::string headers) { cors_allow_headers_ = std::move(headers); }
    void setLogger(Logger logger) { logger_ = std::move(logger); }

    int port() const { return port_; }

private:
    void applyCors(httplib::Response& res);

    int port_;
    std::string bind_address_;
    OpenAIEndpoints& openai_;
    NodeEndpoints& node_;
    httplib::Server server_;
    std::thread thread_;
    std::atomic<bool> running_{false};
    std::vector<Middleware> middlewares_;
    bool enable_cors_{true};
    std::string cors_allow_origin_{"*"};
    std::string cors_allow_methods_{"GET, POST, OPTIONS"};
    std::string cors_allow_headers_{"Content-Type, Authorization"};
    Logger logger_{};
};

}  // namespace llm_node
