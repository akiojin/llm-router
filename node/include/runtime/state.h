#pragma once

#include <atomic>

namespace llm_node {

extern std::atomic<bool> g_running_flag;
extern std::atomic<bool> g_ready_flag;

inline bool is_running() { return g_running_flag.load(); }
inline void request_shutdown() { g_running_flag.store(false); }
inline bool is_ready() { return g_ready_flag.load(); }
inline void set_ready(bool v) { g_ready_flag.store(v); }

}  // namespace llm_node
