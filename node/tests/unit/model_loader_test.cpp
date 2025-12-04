#include <gtest/gtest.h>
#include <filesystem>
#include <fstream>
#include <chrono>
#include <thread>

#include "core/llama_manager.h"

using namespace llm_node;
namespace fs = std::filesystem;

class TempModelDir {
public:
    TempModelDir() {
        base = fs::temp_directory_path() / fs::path("llm-loader-XXXXXX");
        std::string tmpl = base.string();
        std::vector<char> buf(tmpl.begin(), tmpl.end());
        buf.push_back('\0');
        char* created = mkdtemp(buf.data());
        base = created ? fs::path(created) : fs::temp_directory_path();
    }
    ~TempModelDir() {
        std::error_code ec;
        fs::remove_all(base, ec);
    }
    fs::path base;
};

// T049: test_on_demand_load
// オンデマンドロード機能のテスト
// モデルがロードされていない状態でリクエストが来た場合、自動的にロードする
TEST(ModelLoaderTest, OnDemandLoadTriggersWhenModelNotLoaded) {
    TempModelDir tmp;
    fs::path model = tmp.base / "test_model.gguf";
    fs::create_directories(model.parent_path());
    // ダミーGGUFファイル（llama.cppは拒否するが、ロード試行は確認できる）
    std::ofstream(model) << "GGUF";

    LlamaManager mgr(tmp.base.string());

    // 初期状態ではモデルはロードされていない
    EXPECT_FALSE(mgr.isLoaded("test_model.gguf"));
    EXPECT_EQ(mgr.loadedCount(), 0u);

    // loadModelIfNeeded()を呼び出すとロードを試行する
    // （実際のGGUFではないのでロードは失敗するが、メソッドの動作確認）
    bool loaded = mgr.loadModelIfNeeded("test_model.gguf");

    // 無効なGGUFなのでロード失敗
    EXPECT_FALSE(loaded);
}

// T050: test_idle_unload
// アイドル状態のモデルが自動的にアンロードされることをテスト
TEST(ModelLoaderTest, IdleModelsAreUnloadedAfterTimeout) {
    TempModelDir tmp;

    LlamaManager mgr(tmp.base.string());

    // 短いタイムアウトを設定（テスト用: 100ms）
    mgr.setIdleTimeout(std::chrono::milliseconds(100));

    // アイドルタイムアウトの確認
    EXPECT_EQ(mgr.getIdleTimeout(), std::chrono::milliseconds(100));

    // unloadIdleModels()を呼び出してもロード済みモデルがなければ何も起きない
    size_t unloaded = mgr.unloadIdleModels();
    EXPECT_EQ(unloaded, 0u);
}

// T051: test_memory_limit
// 同時ロード可能なモデル数の制限テスト
TEST(ModelLoaderTest, MemoryLimitRestrictsLoadedModels) {
    TempModelDir tmp;

    LlamaManager mgr(tmp.base.string());

    // 最大ロード数を設定
    mgr.setMaxLoadedModels(2);
    EXPECT_EQ(mgr.getMaxLoadedModels(), 2u);

    // 現在のロード数が制限内かチェック
    EXPECT_TRUE(mgr.canLoadMore());

    // メモリ制限を設定
    mgr.setMaxMemoryBytes(1024 * 1024 * 1024);  // 1GB
    EXPECT_EQ(mgr.getMaxMemoryBytes(), 1024 * 1024 * 1024u);
}

// アクセス時刻追跡のテスト
TEST(ModelLoaderTest, TracksLastAccessTime) {
    TempModelDir tmp;
    fs::path model = tmp.base / "track_model.gguf";
    fs::create_directories(model.parent_path());
    std::ofstream(model) << "GGUF";

    LlamaManager mgr(tmp.base.string());

    // モデルがロードされていない場合、アクセス時刻は無効
    auto access_time = mgr.getLastAccessTime("track_model.gguf");
    EXPECT_FALSE(access_time.has_value());
}

// LRUアンロードのテスト
TEST(ModelLoaderTest, LRUUnloadSelectsOldestAccessedModel) {
    TempModelDir tmp;

    LlamaManager mgr(tmp.base.string());
    mgr.setMaxLoadedModels(2);

    // ロード済みモデルがない場合、LRUアンロード対象もなし
    auto oldest = mgr.getLeastRecentlyUsedModel();
    EXPECT_FALSE(oldest.has_value());
}
