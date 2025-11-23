#pragma once

#include <atomic>

namespace ollama_node {

extern std::atomic<bool> g_running_flag;

inline bool is_running() { return g_running_flag.load(); }
inline void request_shutdown() { g_running_flag.store(false); }

}  // namespace ollama_node
