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
    EnvGuard guard({"OLLAMA_NODE_CONFIG", "OLLAMA_ROUTER_URL", "OLLAMA_MODELS_DIR",
                    "OLLAMA_NODE_PORT", "OLLAMA_HEARTBEAT_SECS", "OLLAMA_ALLOW_NO_GPU"});

    fs::path tmp = fs::temp_directory_path() / "nodecfg.json";
    std::ofstream(tmp) << R"({
        "router_url": "http://file:9000",
        "models_dir": "/tmp/models",
        "node_port": 18080,
        "heartbeat_interval_sec": 3,
        "require_gpu": false
    })";
    setenv("OLLAMA_NODE_CONFIG", tmp.string().c_str(), 1);

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
    EnvGuard guard({"OLLAMA_ROUTER_URL", "OLLAMA_MODELS_DIR", "OLLAMA_NODE_PORT",
                    "OLLAMA_HEARTBEAT_SECS", "OLLAMA_ALLOW_NO_GPU", "OLLAMA_NODE_CONFIG"});

    unsetenv("OLLAMA_NODE_CONFIG");
    setenv("OLLAMA_ROUTER_URL", "http://env:1234", 1);
    setenv("OLLAMA_MODELS_DIR", "/env/models", 1);
    setenv("OLLAMA_NODE_PORT", "19000", 1);
    setenv("OLLAMA_HEARTBEAT_SECS", "7", 1);
    setenv("OLLAMA_ALLOW_NO_GPU", "true", 1);

    auto cfg = loadNodeConfig();
    EXPECT_EQ(cfg.router_url, "http://env:1234");
    EXPECT_EQ(cfg.models_dir, "/env/models");
    EXPECT_EQ(cfg.node_port, 19000);
    EXPECT_EQ(cfg.heartbeat_interval_sec, 7);
    EXPECT_FALSE(cfg.require_gpu);
}
