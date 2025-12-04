#include "utils/cli.h"
#include "utils/version.h"
#include <sstream>
#include <cstring>

namespace llm_node {

std::string getHelpMessage() {
    std::ostringstream oss;
    oss << "llm-node " << LLM_NODE_VERSION << " - LLM inference node with llama.cpp\n";
    oss << "\n";
    oss << "USAGE:\n";
    oss << "    llm-node [OPTIONS]\n";
    oss << "\n";
    oss << "OPTIONS:\n";
    oss << "    -h, --help       Print help information\n";
    oss << "    -V, --version    Print version information\n";
    oss << "\n";
    oss << "ENVIRONMENT VARIABLES:\n";
    oss << "    LLM_NODE_MODELS_DIR          Model files directory (default: ~/.llm-router/models)\n";
    oss << "    LLM_NODE_PORT                HTTP server port (default: 11435)\n";
    oss << "    LLM_NODE_HEARTBEAT_SECS      Heartbeat interval in seconds (default: 10)\n";
    oss << "    LLM_NODE_ALLOW_NO_GPU        Allow running without GPU (default: false)\n";
    oss << "    LLM_NODE_BIND_ADDRESS        Bind address (default: 0.0.0.0)\n";
    oss << "    LLM_NODE_LOG_DIR             Log files directory\n";
    oss << "    LLM_NODE_LOG_LEVEL           Log level: trace, debug, info, warn, error (default: info)\n";
    oss << "    LLM_NODE_LOG_RETENTION_DAYS  Log retention days (default: 7)\n";
    oss << "\n";
    oss << "    LLM_ROUTER_URL               Router URL (default: http://127.0.0.1:11434)\n";
    oss << "    LLM_NODE_IP                  Node IP address for registration (auto-detect)\n";
    oss << "    LLM_NODE_CONFIG              Path to config JSON file\n";
    oss << "\n";
    oss << "DEPRECATED ENVIRONMENT VARIABLES (use LLM_NODE_* instead):\n";
    oss << "    LLM_MODELS_DIR               -> LLM_NODE_MODELS_DIR\n";
    oss << "    LLM_HEARTBEAT_SECS           -> LLM_NODE_HEARTBEAT_SECS\n";
    oss << "    LLM_ALLOW_NO_GPU             -> LLM_NODE_ALLOW_NO_GPU\n";
    oss << "    LLM_BIND_ADDRESS             -> LLM_NODE_BIND_ADDRESS\n";
    oss << "    LLM_LOG_DIR                  -> LLM_NODE_LOG_DIR\n";
    oss << "    LLM_LOG_LEVEL                -> LLM_NODE_LOG_LEVEL\n";
    oss << "    LLM_LOG_RETENTION_DAYS       -> LLM_NODE_LOG_RETENTION_DAYS\n";
    return oss.str();
}

std::string getVersionMessage() {
    std::ostringstream oss;
    oss << "llm-node " << LLM_NODE_VERSION << "\n";
    return oss.str();
}

CliResult parseCliArgs(int argc, char* argv[]) {
    CliResult result;

    for (int i = 1; i < argc; ++i) {
        const char* arg = argv[i];

        if (std::strcmp(arg, "-h") == 0 || std::strcmp(arg, "--help") == 0) {
            result.should_exit = true;
            result.exit_code = 0;
            result.output = getHelpMessage();
            return result;
        }

        if (std::strcmp(arg, "-V") == 0 || std::strcmp(arg, "--version") == 0) {
            result.should_exit = true;
            result.exit_code = 0;
            result.output = getVersionMessage();
            return result;
        }

        // Unknown argument
        result.should_exit = true;
        result.exit_code = 1;
        std::ostringstream oss;
        oss << "Error: Unknown argument '" << arg << "'\n";
        oss << "\n";
        oss << "For more information, try '--help'\n";
        result.output = oss.str();
        return result;
    }

    // No special arguments, continue to server mode
    result.should_exit = false;
    return result;
}

}  // namespace llm_node
