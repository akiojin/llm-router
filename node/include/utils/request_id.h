// request_id.h - simple request-id generator (hex)
#pragma once

#include <string>

namespace llm_node {

// Generate a random 16-hex-character request id.
std::string generate_request_id();

// Generate 32-hex trace id and 16-hex span id (W3C traceparent compatible)
std::string generate_trace_id();
std::string generate_span_id();

}  // namespace llm_node
