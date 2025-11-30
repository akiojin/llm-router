#include <gtest/gtest.h>
#include <cstdlib>
#include <fstream>
#include <filesystem>
#include <unordered_map>

#include "utils/config.h"

using namespace ollama_node;
namespace fs = std::filesystem;

class EnvGuard {
public:
    EnvGuard(const std::vector<std::string>& keys) : keys_(keys) {
        for (const auto& k : keys_) {
            const char* v = std::getenv(k.c_str());
            if (v) saved_[k] = v;
        }
    }
    ~EnvGuard() {
        for (const auto& k : keys_) {
            if (auto it = saved_.find(k); it != saved_.end()) {
                setenv(k.c_str(), it->second.c_str(), 1);
            } else {
                unsetenv(k.c_str());
            }
        }
    }
private:
    std::vector<std::string> keys_;
    std::unordered_map<std::string, std::string> saved_;
};

TEST(UtilsConfigTest, LoadsNodeConfigFromFileWithLock) {
    EnvGuard guard({"LLM_NODE_CONFIG", "LLM_ROUTER_URL", "LLM_MODELS_DIR",
                    "LLM_NODE_PORT", "LLM_HEARTBEAT_SECS", "LLM_ALLOW_NO_GPU"});

    fs::path tmp = fs::temp_directory_path() / "nodecfg.json";
    std::ofstream(tmp) << R"({
        "router_url": "http://file:9000",
        "models_dir": "/tmp/models",
        "node_port": 18080,
        "heartbeat_interval_sec": 3,
        "require_gpu": false
    })";
    setenv("LLM_NODE_CONFIG", tmp.string().c_str(), 1);

    auto info = loadNodeConfigWithLog();
    auto cfg = info.first;

    EXPECT_EQ(cfg.router_url, "http://file:9000");
    EXPECT_EQ(cfg.models_dir, "/tmp/models");
    EXPECT_EQ(cfg.node_port, 18080);
    EXPECT_EQ(cfg.heartbeat_interval_sec, 3);
    EXPECT_FALSE(cfg.require_gpu);
    EXPECT_NE(info.second.find("file="), std::string::npos);

    fs::remove(tmp);
}

TEST(UtilsConfigTest, EnvOverridesNodeConfig) {
    EnvGuard guard({"LLM_ROUTER_URL", "LLM_MODELS_DIR", "LLM_NODE_PORT",
                    "LLM_HEARTBEAT_SECS", "LLM_ALLOW_NO_GPU", "LLM_NODE_CONFIG",
                    "LLM_NODE_MODELS_DIR", "LLM_NODE_HEARTBEAT_SECS", "LLM_NODE_ALLOW_NO_GPU"});

    unsetenv("LLM_NODE_CONFIG");
    // Test with deprecated env var names (fallback)
    setenv("LLM_ROUTER_URL", "http://env:1234", 1);
    setenv("LLM_MODELS_DIR", "/env/models", 1);
    setenv("LLM_NODE_PORT", "19000", 1);
    setenv("LLM_HEARTBEAT_SECS", "7", 1);
    setenv("LLM_ALLOW_NO_GPU", "true", 1);

    auto cfg = loadNodeConfig();
    EXPECT_EQ(cfg.router_url, "http://env:1234");
    EXPECT_EQ(cfg.models_dir, "/env/models");
    EXPECT_EQ(cfg.node_port, 19000);
    EXPECT_EQ(cfg.heartbeat_interval_sec, 7);
    EXPECT_FALSE(cfg.require_gpu);
}

TEST(UtilsConfigTest, NewEnvVarsTakePriorityOverDeprecated) {
    EnvGuard guard({"LLM_ROUTER_URL", "LLM_NODE_MODELS_DIR", "LLM_MODELS_DIR",
                    "LLM_NODE_PORT", "LLM_NODE_HEARTBEAT_SECS", "LLM_HEARTBEAT_SECS",
                    "LLM_NODE_ALLOW_NO_GPU", "LLM_ALLOW_NO_GPU", "LLM_NODE_CONFIG"});

    unsetenv("LLM_NODE_CONFIG");

    // Set both new and deprecated env vars
    setenv("LLM_NODE_MODELS_DIR", "/new/models", 1);
    setenv("LLM_MODELS_DIR", "/old/models", 1);  // Should be ignored
    setenv("LLM_NODE_HEARTBEAT_SECS", "15", 1);
    setenv("LLM_HEARTBEAT_SECS", "5", 1);  // Should be ignored
    setenv("LLM_NODE_ALLOW_NO_GPU", "true", 1);
    setenv("LLM_ALLOW_NO_GPU", "false", 1);  // Should be ignored

    auto cfg = loadNodeConfig();
    EXPECT_EQ(cfg.models_dir, "/new/models");
    EXPECT_EQ(cfg.heartbeat_interval_sec, 15);
    EXPECT_FALSE(cfg.require_gpu);  // LLM_NODE_ALLOW_NO_GPU=true
}
