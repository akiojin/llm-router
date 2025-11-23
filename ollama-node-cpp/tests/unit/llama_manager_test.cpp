#include <gtest/gtest.h>
#include <filesystem>
#include <fstream>

#include "core/llama_manager.h"

using namespace ollama_node;
namespace fs = std::filesystem;

class TempModelFile {
public:
    TempModelFile() {
        base = fs::temp_directory_path() / fs::path("llm-XXXXXX");
        std::string tmpl = base.string();
        std::vector<char> buf(tmpl.begin(), tmpl.end());
        buf.push_back('\0');
        char* created = mkdtemp(buf.data());
        base = created ? fs::path(created) : fs::temp_directory_path();
    }
    ~TempModelFile() {
        std::error_code ec;
        fs::remove_all(base, ec);
    }
    fs::path base;
};

TEST(LlamaManagerTest, LoadsExistingModel) {
    TempModelFile tmp;
    fs::path model = tmp.base / "model.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";

    LlamaManager mgr(tmp.base.string());
    mgr.setGpuLayerSplit(5);
    EXPECT_TRUE(mgr.loadModel("model.gguf"));
    EXPECT_EQ(mgr.loadedCount(), 1u);

    auto ctx = mgr.createContext("model.gguf");
    ASSERT_NE(ctx, nullptr);
    EXPECT_EQ(ctx->model, model.string());
    EXPECT_EQ(ctx->gpu_layers, 5u);
}

TEST(LlamaManagerTest, FailsOnMissingModel) {
    TempModelFile tmp;
    LlamaManager mgr(tmp.base.string());
    EXPECT_FALSE(mgr.loadModel("missing.gguf"));
    EXPECT_EQ(mgr.loadedCount(), 0u);
    EXPECT_EQ(mgr.createContext("missing.gguf"), nullptr);
}

TEST(LlamaManagerTest, RejectsUnsupportedExtension) {
    TempModelFile tmp;
    fs::path model = tmp.base / "bad.txt";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "bad";
    LlamaManager mgr(tmp.base.string());
    EXPECT_FALSE(mgr.loadModel("bad.txt"));
    EXPECT_EQ(mgr.loadedCount(), 0u);
}

TEST(LlamaManagerTest, TracksMemoryUsageOnLoad) {
    TempModelFile tmp;
    fs::path model1 = tmp.base / "m1.gguf";
    fs::path model2 = tmp.base / "m2.gguf";
    fs::create_directories(model1.parent_path());
    std::ofstream(model1) << "gguf";
    std::ofstream(model2) << "gguf";

    LlamaManager mgr(tmp.base.string());
    EXPECT_EQ(mgr.memoryUsageBytes(), 0u);
    mgr.loadModel("m1.gguf");
    mgr.loadModel("m2.gguf");
    EXPECT_EQ(mgr.memoryUsageBytes(), 1024ull * 1024ull * 1024ull);  // 2 * 512MB
}

TEST(LlamaManagerTest, UnloadReducesMemory) {
    TempModelFile tmp;
    fs::path model = tmp.base / "m.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";
    LlamaManager mgr(tmp.base.string());
    mgr.loadModel("m.gguf");
    EXPECT_GT(mgr.memoryUsageBytes(), 0u);
    EXPECT_TRUE(mgr.unloadModel("m.gguf"));
    EXPECT_EQ(mgr.memoryUsageBytes(), 0u);
}
