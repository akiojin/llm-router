#include "api/http_server.h"

#include "api/openai_endpoints.h"
#include "api/node_endpoints.h"
#include <nlohmann/json.hpp>
#include "utils/request_id.h"

namespace llm_node {

HttpServer::HttpServer(int port, OpenAIEndpoints& openai, NodeEndpoints& node, std::string bind_address)
    : port_(port), bind_address_(std::move(bind_address)), openai_(openai), node_(node) {}

HttpServer::~HttpServer() { stop(); }

void HttpServer::addMiddleware(Middleware mw) {
    middlewares_.push_back(std::move(mw));
}

void HttpServer::applyCors(httplib::Response& res) {
    if (!enable_cors_) return;
    if (!res.has_header("Access-Control-Allow-Origin"))
        res.set_header("Access-Control-Allow-Origin", cors_allow_origin_.c_str());
    if (!res.has_header("Access-Control-Allow-Methods"))
        res.set_header("Access-Control-Allow-Methods", cors_allow_methods_.c_str());
    if (!res.has_header("Access-Control-Allow-Headers"))
        res.set_header("Access-Control-Allow-Headers", cors_allow_headers_.c_str());
}

void HttpServer::start() {
    if (running_) return;

    // Pre-routing middleware & CORS preflight
    server_.set_pre_routing_handler([this](const httplib::Request& req, httplib::Response& res) {
        // CORS headers are set here as well as post-routing to cover short-circuit paths
        if (enable_cors_) {
            applyCors(res);

            if (req.method == "OPTIONS") {
                res.status = 204;
                return httplib::Server::HandlerResponse::Handled;  // short-circuit preflight
            }
        }

        // Request ID
        std::string req_id = req.get_header_value("X-Request-Id");
        if (req_id.empty()) req_id = generate_request_id();
        res.set_header("X-Request-Id", req_id);

        // Traceparent (W3C)
        std::string traceparent = req.get_header_value("traceparent");
        std::string trace_id = generate_trace_id();
        std::string span_id = generate_span_id();
        if (!traceparent.empty() && traceparent.size() >= 55) {
            // format: 00-<32 hex trace id>-<16 hex span id>-<flags>
            // keep incoming trace id, new span id
            std::string incoming_trace = traceparent.substr(3, 32);
            trace_id = incoming_trace;
        }
        std::string new_traceparent = "00-" + trace_id + "-" + span_id + "-01";
        res.set_header("traceparent", new_traceparent);

        for (auto& mw : middlewares_) {
            if (!mw(req, res)) return httplib::Server::HandlerResponse::Handled;
        }
        return httplib::Server::HandlerResponse::Unhandled;
    });

    // Post-routing handler to ensure CORS headers on all responses
    server_.set_post_routing_handler([this](const httplib::Request&, httplib::Response& res) {
        applyCors(res);
    });

    // Access log
    if (logger_) {
        server_.set_logger([this](const httplib::Request& req, const httplib::Response& res) {
            logger_(req, res);
        });
    }

    // Error handler (404/others)
    server_.set_error_handler([](const httplib::Request& req, httplib::Response& res) {
        if (!res.body.empty()) {
            // respect existing body set by middleware/handlers
            if (!res.has_header("Content-Type")) {
                res.set_header("Content-Type", "text/plain");
            }
            return;
        }
        nlohmann::json body = {
            {"error", res.status == 404 ? "not_found" : "http_error"},
            {"status", res.status},
            {"path", req.path}
        };
        res.set_content(body.dump(), "application/json");
    });

    server_.set_exception_handler([](const httplib::Request& req, httplib::Response& res, std::exception_ptr ep) {
        std::string what = "unknown";
        if (ep) {
            try {
                std::rethrow_exception(ep);
            } catch (const std::exception& e) {
                what = e.what();
            }
        }
        nlohmann::json body = {
            {"error", "internal_error"},
            {"path", req.path},
            {"message", what}
        };
        res.status = 500;
        res.set_content(body.dump(), "application/json");
    });

    openai_.registerRoutes(server_);
    node_.registerRoutes(server_);

    running_ = true;
    thread_ = std::thread([this]() { server_.listen(bind_address_.c_str(), port_); });
    while (!server_.is_running()) {
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
}

void HttpServer::stop() {
    if (!running_) return;
    server_.stop();
    if (thread_.joinable()) thread_.join();
    running_ = false;
}

}  // namespace llm_node
