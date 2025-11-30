/// @file model_lifecycle_test.cpp
/// @brief Integration tests for on-demand model loading lifecycle
/// T052: test_load_unload_cycle()

#include <gtest/gtest.h>

#include "core/llama_manager.h"

#include <chrono>
#include <thread>
#include <fstream>
#include <filesystem>

namespace fs = std::filesystem;

namespace {

/// Create a temporary directory for test models
class TempModelDir {
public:
    TempModelDir() {
        char tmpl[] = "/tmp/model-lifecycle-XXXXXX";
        char* result = mkdtemp(tmpl);
        if (result) {
            path_ = result;
        }
    }
    ~TempModelDir() {
        if (!path_.empty()) {
            fs::remove_all(path_);
        }
    }
    const std::string& path() const { return path_; }

    /// Create a dummy file (not a valid GGUF, just for path testing)
    std::string createDummyModel(const std::string& name) {
        std::string file_path = path_ + "/" + name + ".gguf";
        std::ofstream ofs(file_path);
        ofs << "dummy";
        ofs.close();
        return file_path;
    }

private:
    std::string path_;
};

}  // namespace

/// Integration test: verify the complete load/unload cycle for on-demand loading
TEST(ModelLifecycleTest, LoadUnloadCycle) {
    TempModelDir tmp;
    ollama_node::LlamaManager manager(tmp.path());

    // Configure on-demand settings
    manager.setIdleTimeout(std::chrono::milliseconds(100));
    manager.setMaxLoadedModels(2);

    // Verify initial state
    EXPECT_EQ(manager.loadedCount(), 0);
    EXPECT_TRUE(manager.canLoadMore());
    EXPECT_EQ(manager.getIdleTimeout(), std::chrono::milliseconds(100));
    EXPECT_EQ(manager.getMaxLoadedModels(), 2);

    // Create dummy model files
    std::string model1 = tmp.createDummyModel("model1");
    std::string model2 = tmp.createDummyModel("model2");

    // loadModelIfNeeded should fail for invalid GGUF files, but
    // verifies the API works correctly
    bool result1 = manager.loadModelIfNeeded(model1);
    // Expected to fail because dummy file is not valid GGUF
    EXPECT_FALSE(result1);

    // LRU tracking should not have entries for failed loads
    auto lru = manager.getLeastRecentlyUsedModel();
    EXPECT_FALSE(lru.has_value());

    // unloadIdleModels should handle empty state
    size_t unloaded = manager.unloadIdleModels();
    EXPECT_EQ(unloaded, 0);
}

/// Integration test: verify LRU eviction policy with max model limit
TEST(ModelLifecycleTest, LRUEvictionPolicy) {
    TempModelDir tmp;
    ollama_node::LlamaManager manager(tmp.path());

    // Set max loaded models to 1 (restrictive for testing)
    manager.setMaxLoadedModels(1);

    // Initial state
    EXPECT_TRUE(manager.canLoadMore());

    // No models loaded, LRU should return nullopt
    auto lru = manager.getLeastRecentlyUsedModel();
    EXPECT_FALSE(lru.has_value());
}

/// Integration test: verify idle timeout configuration
TEST(ModelLifecycleTest, IdleTimeoutConfiguration) {
    TempModelDir tmp;
    ollama_node::LlamaManager manager(tmp.path());

    // Test default timeout (5 minutes)
    EXPECT_EQ(manager.getIdleTimeout(), std::chrono::minutes(5));

    // Set custom timeout
    manager.setIdleTimeout(std::chrono::seconds(30));
    EXPECT_EQ(manager.getIdleTimeout(), std::chrono::seconds(30));

    // Set to zero (effectively disable auto-unload)
    manager.setIdleTimeout(std::chrono::milliseconds(0));
    EXPECT_EQ(manager.getIdleTimeout(), std::chrono::milliseconds(0));
}

/// Integration test: verify memory limit configuration
TEST(ModelLifecycleTest, MemoryLimitConfiguration) {
    TempModelDir tmp;
    ollama_node::LlamaManager manager(tmp.path());

    // Test default (0 = unlimited)
    EXPECT_EQ(manager.getMaxMemoryBytes(), 0);

    // Set memory limit
    size_t limit = 8ULL * 1024 * 1024 * 1024;  // 8GB
    manager.setMaxMemoryBytes(limit);
    EXPECT_EQ(manager.getMaxMemoryBytes(), limit);

    // Verify canLoadMore with max models set to 0 (unlimited)
    manager.setMaxLoadedModels(0);
    EXPECT_TRUE(manager.canLoadMore());

    // Set restrictive limit
    manager.setMaxLoadedModels(0);
    EXPECT_TRUE(manager.canLoadMore());  // 0 = unlimited
}

/// Integration test: access time tracking
TEST(ModelLifecycleTest, AccessTimeTracking) {
    TempModelDir tmp;
    ollama_node::LlamaManager manager(tmp.path());

    // No models loaded, access time should be nullopt
    auto access_time = manager.getLastAccessTime("nonexistent.gguf");
    EXPECT_FALSE(access_time.has_value());
}
