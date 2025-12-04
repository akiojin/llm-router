#include "runtime/state.h"

namespace llm_node {

std::atomic<bool> g_running_flag{true};
std::atomic<bool> g_ready_flag{false};

}  // namespace llm_node
