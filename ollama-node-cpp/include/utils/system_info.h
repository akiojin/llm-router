// system_info.h - simple system information helpers
#pragma once

#include <string>
#include <cstdint>

namespace ollama_node {

struct SystemInfo {
    std::string os;
    std::string arch;
    unsigned int cpu_cores;
    uint64_t total_memory_bytes;
};

SystemInfo collect_system_info();
std::string format_system_info(const SystemInfo& info);

}  // namespace ollama_node
