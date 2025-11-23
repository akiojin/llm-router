#include <gtest/gtest.h>
#include <filesystem>
#include <fstream>

#include "core/model_pool.h"

using namespace ollama_node;
namespace fs = std::filesystem;

class TempModelPoolDir {
public:
    TempModelPoolDir() {
        base = fs::temp_directory_path() / fs::path("pool-XXXXXX");
        std::string tmpl = base.string();
        std::vector<char> buf(tmpl.begin(), tmpl.end());
        buf.push_back('\0');
        char* created = mkdtemp(buf.data());
        base = created ? fs::path(created) : fs::temp_directory_path();
    }
    ~TempModelPoolDir() {
        std::error_code ec;
        fs::remove_all(base, ec);
    }
    fs::path base;
};

TEST(ModelPoolTest, LoadsAndCreatesContext) {
    TempModelPoolDir tmp;
    fs::path model = tmp.base / "m.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";

    auto manager = std::make_shared<LlamaManager>(tmp.base.string());
    ModelPool pool(manager);

    auto ctx = pool.acquire("m.gguf");
    ASSERT_NE(ctx, nullptr);
    EXPECT_EQ(pool.loadedCount(), 1u);
}

TEST(ModelPoolTest, ReturnsNullWhenMissing) {
    TempModelPoolDir tmp;
    auto manager = std::make_shared<LlamaManager>(tmp.base.string());
    ModelPool pool(manager);

    auto ctx = pool.acquire("missing.gguf");
    EXPECT_EQ(ctx, nullptr);
    EXPECT_EQ(pool.loadedCount(), 0u);
}

TEST(ModelPoolTest, RespectsMemoryLimit) {
    TempModelPoolDir tmp;
    fs::path model = tmp.base / "m.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";

    auto manager = std::make_shared<LlamaManager>(tmp.base.string());
    ModelPool pool(manager);
    pool.setMemoryLimit(256ull * 1024ull * 1024ull);  // lower than 512MB placeholder
    auto ctx = pool.acquire("m.gguf");
    EXPECT_EQ(ctx, nullptr);  // should reject due to limit
    EXPECT_EQ(pool.loadedCount(), 0u);
}

TEST(ModelPoolTest, ThreadSafeAcquire) {
    TempModelPoolDir tmp;
    fs::path model = tmp.base / "m.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";

    auto manager = std::make_shared<LlamaManager>(tmp.base.string());
    ModelPool pool(manager);

    std::vector<std::thread> threads;
    std::atomic<int> success{0};
    for (int i = 0; i < 8; ++i) {
        threads.emplace_back([&]() {
            auto ctx = pool.acquire("m.gguf");
            if (ctx) success++;
        });
    }
    for (auto& t : threads) t.join();
    EXPECT_GE(success.load(), 1);
}

TEST(ModelPoolTest, ThreadLocalCacheReturnsSameContext) {
    TempModelPoolDir tmp;
    fs::path model = tmp.base / "m.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";
    auto manager = std::make_shared<LlamaManager>(tmp.base.string());
    ModelPool pool(manager);

    auto ctx1 = pool.acquireForThread("m.gguf", std::this_thread::get_id());
    auto ctx2 = pool.acquireForThread("m.gguf", std::this_thread::get_id());
    ASSERT_NE(ctx1, nullptr);
    EXPECT_EQ(ctx1.get(), ctx2.get());
}

TEST(ModelPoolTest, GcClearsThreadCache) {
    TempModelPoolDir tmp;
    fs::path model = tmp.base / "m.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";
    auto manager = std::make_shared<LlamaManager>(tmp.base.string());
    ModelPool pool(manager);

    auto ctx1 = pool.acquireForThread("m.gguf", std::this_thread::get_id());
    ASSERT_NE(ctx1, nullptr);
    pool.gc();
    auto ctx2 = pool.acquireForThread("m.gguf", std::this_thread::get_id());
    EXPECT_NE(ctx1.get(), ctx2.get());
}

TEST(ModelPoolTest, GcUnloadsAll) {
    TempModelPoolDir tmp;
    fs::path model = tmp.base / "m.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";
    auto manager = std::make_shared<LlamaManager>(tmp.base.string());
    ModelPool pool(manager);
    auto ctx = pool.acquire("m.gguf");
    ASSERT_NE(ctx, nullptr);
    EXPECT_EQ(pool.loadedCount(), 1u);
    pool.gc();
    EXPECT_EQ(pool.loadedCount(), 0u);
    EXPECT_EQ(manager->memoryUsageBytes(), 0u);
}

TEST(ModelPoolTest, UnloadRemovesModel) {
    TempModelPoolDir tmp;
    fs::path model = tmp.base / "m.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "gguf";
    auto manager = std::make_shared<LlamaManager>(tmp.base.string());
    ModelPool pool(manager);
    auto ctx = pool.acquire("m.gguf");
    ASSERT_NE(ctx, nullptr);
    EXPECT_EQ(pool.loadedCount(), 1u);
    EXPECT_TRUE(pool.unload("m.gguf"));
    EXPECT_EQ(pool.loadedCount(), 0u);
    EXPECT_EQ(manager->memoryUsageBytes(), 0u);
}
