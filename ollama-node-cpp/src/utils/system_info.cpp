#include "utils/system_info.h"

#include <thread>
#include <sstream>

#if defined(_WIN32)
#include <windows.h>
#elif defined(__APPLE__)
#include <sys/types.h>
#include <sys/sysctl.h>
#include <unistd.h>
#elif defined(__linux__)
#include <unistd.h>
#endif

namespace ollama_node {

static uint64_t get_total_memory_bytes() {
#if defined(_WIN32)
    MEMORYSTATUSEX status;
    status.dwLength = sizeof(status);
    if (GlobalMemoryStatusEx(&status)) {
        return static_cast<uint64_t>(status.ullTotalPhys);
    }
    return 0;
#elif defined(__APPLE__)
    int mib[2] = {CTL_HW, HW_MEMSIZE};
    uint64_t mem = 0;
    size_t len = sizeof(mem);
    if (sysctl(mib, 2, &mem, &len, nullptr, 0) == 0) {
        return mem;
    }
    return 0;
#elif defined(__linux__)
    long pages = sysconf(_SC_PHYS_PAGES);
    long page_size = sysconf(_SC_PAGESIZE);
    if (pages > 0 && page_size > 0) {
        return static_cast<uint64_t>(pages) * static_cast<uint64_t>(page_size);
    }
    return 0;
#else
    return 0;
#endif
}

static std::string detect_os() {
#if defined(_WIN32)
    return "windows";
#elif defined(__APPLE__)
    return "macos";
#elif defined(__linux__)
    return "linux";
#else
    return "unknown";
#endif
}

static std::string detect_arch() {
#if defined(__x86_64__) || defined(_M_X64)
    return "x86_64";
#elif defined(__aarch64__)
    return "arm64";
#elif defined(__arm__) || defined(_M_ARM)
    return "arm";
#else
    return "unknown";
#endif
}

SystemInfo collect_system_info() {
    SystemInfo info;
    info.os = detect_os();
    info.arch = detect_arch();
    info.cpu_cores = std::thread::hardware_concurrency();
    info.total_memory_bytes = get_total_memory_bytes();
    return info;
}

std::string format_system_info(const SystemInfo& info) {
    std::ostringstream oss;
    oss << "os=" << info.os << " arch=" << info.arch
        << " cores=" << info.cpu_cores
        << " mem_bytes=" << info.total_memory_bytes;
    return oss.str();
}

}  // namespace ollama_node
