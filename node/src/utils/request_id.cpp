#include "utils/request_id.h"

#include <random>
#include <sstream>
#include <iomanip>

namespace llm_node {

std::string generate_request_id() {
    static thread_local std::mt19937_64 rng{std::random_device{}()};
    uint64_t v = rng();
    std::ostringstream oss;
    oss << std::hex << std::setfill('0') << std::setw(16) << v;
    return oss.str();
}

std::string generate_trace_id() {
    static thread_local std::mt19937_64 rng{std::random_device{}()};
    uint64_t hi = rng();
    uint64_t lo = rng();
    std::ostringstream oss;
    oss << std::hex << std::setfill('0') << std::setw(16) << hi
        << std::setw(16) << lo;
    return oss.str();
}

std::string generate_span_id() {
    static thread_local std::mt19937_64 rng{std::random_device{}()};
    uint64_t v = rng();
    std::ostringstream oss;
    oss << std::hex << std::setfill('0') << std::setw(16) << v;
    return oss.str();
}

}  // namespace llm_node
