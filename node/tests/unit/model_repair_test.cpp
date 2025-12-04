#include <gtest/gtest.h>
#include <filesystem>
#include <fstream>

#include "models/model_repair.h"
#include "models/model_sync.h"
#include "models/model_downloader.h"
#include "models/model_storage.h"

namespace fs = std::filesystem;
using namespace llm_node;

class TempDirGuard {
public:
    TempDirGuard() {
        path = fs::temp_directory_path() / fs::path("model-repair-test-XXXXXX");
        std::string tmpl = path.string();
        std::vector<char> buf(tmpl.begin(), tmpl.end());
        buf.push_back('\0');
        char* created = mkdtemp(buf.data());
        path = created ? fs::path(created) : fs::temp_directory_path();
    }
    ~TempDirGuard() {
        std::error_code ec;
        fs::remove_all(path, ec);
    }
    fs::path path;
};

// テスト用の有効なGGUFヘッダーを作成
void createValidGgufFile(const fs::path& path, size_t size = 4096) {
    std::ofstream file(path, std::ios::binary);
    // GGUFマジックナンバー
    file.write("GGUF", 4);
    // バージョン（リトルエンディアン、version 3）
    uint32_t version = 3;
    file.write(reinterpret_cast<const char*>(&version), sizeof(version));
    // 残りをゼロで埋める
    std::vector<char> padding(size - 8, 0);
    file.write(padding.data(), padding.size());
}

// テスト用の破損ファイルを作成
void createCorruptedFile(const fs::path& path, size_t size = 100) {
    std::ofstream file(path, std::ios::binary);
    // 無効なヘッダー
    file.write("XXXX", 4);
    std::vector<char> garbage(size - 4, 'X');
    file.write(garbage.data(), garbage.size());
}

// テスト用の空ファイルを作成
void createEmptyFile(const fs::path& path) {
    std::ofstream file(path, std::ios::binary);
    // 何も書き込まない
}

// テスト用の小さすぎるファイルを作成
void createTinyFile(const fs::path& path) {
    std::ofstream file(path, std::ios::binary);
    file.write("tiny", 4);
}

// =========================================================
// needsRepair テスト
// =========================================================

TEST(ModelRepairTest, NeedsRepairReturnsTrueForNonExistentFile) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    fs::path non_existent = guard.path / "non_existent.gguf";
    EXPECT_TRUE(repair.needsRepair(non_existent.string()));
}

TEST(ModelRepairTest, NeedsRepairReturnsTrueForEmptyFile) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    fs::path empty_file = guard.path / "empty.gguf";
    createEmptyFile(empty_file);

    EXPECT_TRUE(repair.needsRepair(empty_file.string()));
}

TEST(ModelRepairTest, NeedsRepairReturnsTrueForTinyFile) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    fs::path tiny_file = guard.path / "tiny.gguf";
    createTinyFile(tiny_file);

    // ファイルサイズが1KB未満なので修復が必要
    EXPECT_TRUE(repair.needsRepair(tiny_file.string()));
}

TEST(ModelRepairTest, NeedsRepairReturnsTrueForInvalidGgufHeader) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    fs::path corrupted = guard.path / "corrupted.gguf";
    createCorruptedFile(corrupted, 2048);  // 1KB以上だがヘッダーが無効

    EXPECT_TRUE(repair.needsRepair(corrupted.string()));
}

TEST(ModelRepairTest, NeedsRepairReturnsFalseForValidGgufFile) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    fs::path valid = guard.path / "valid.gguf";
    createValidGgufFile(valid);

    EXPECT_FALSE(repair.needsRepair(valid.string()));
}

// =========================================================
// 修復ステータステスト
// =========================================================

TEST(ModelRepairTest, IsRepairingReturnsFalseInitially) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    EXPECT_FALSE(repair.isRepairing("gpt-oss:7b"));
}

TEST(ModelRepairTest, DefaultTimeoutIs300Seconds) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    EXPECT_EQ(repair.getDefaultTimeout(), std::chrono::seconds(300));
}

TEST(ModelRepairTest, SetDefaultTimeoutUpdatesValue) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    repair.setDefaultTimeout(std::chrono::seconds(600));
    EXPECT_EQ(repair.getDefaultTimeout(), std::chrono::seconds(600));
}

// =========================================================
// RepairResult構造体テスト
// =========================================================

TEST(ModelRepairTest, RepairResultDefaultsToIdle) {
    RepairResult result;
    EXPECT_EQ(result.status, RepairStatus::Idle);
    EXPECT_TRUE(result.error_message.empty());
    EXPECT_TRUE(result.model_path.empty());
    EXPECT_EQ(result.elapsed.count(), 0);
}

// =========================================================
// ModelLoadError列挙型テスト
// =========================================================

TEST(ModelLoadErrorTest, EnumValuesAreDefined) {
    EXPECT_EQ(static_cast<int>(ModelLoadError::None), 0);
    EXPECT_EQ(static_cast<int>(ModelLoadError::FileNotFound), 1);
    EXPECT_EQ(static_cast<int>(ModelLoadError::InvalidFormat), 2);
    EXPECT_EQ(static_cast<int>(ModelLoadError::Corrupted), 3);
    EXPECT_EQ(static_cast<int>(ModelLoadError::ContextFailed), 4);
    EXPECT_EQ(static_cast<int>(ModelLoadError::Unknown), 5);
}

TEST(RepairStatusTest, EnumValuesAreDefined) {
    EXPECT_EQ(static_cast<int>(RepairStatus::Idle), 0);
    EXPECT_EQ(static_cast<int>(RepairStatus::InProgress), 1);
    EXPECT_EQ(static_cast<int>(RepairStatus::Success), 2);
    EXPECT_EQ(static_cast<int>(RepairStatus::Failed), 3);
}

// =========================================================
// エッジケーステスト
// =========================================================

TEST(ModelRepairTest, NeedsRepairReturnsTrueForPartialGgufHeader) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    // GGUFヘッダーの最初の2バイトのみ
    fs::path partial = guard.path / "partial.gguf";
    std::ofstream file(partial, std::ios::binary);
    file.write("GG", 2);
    std::vector<char> padding(2048, 0);
    file.write(padding.data(), padding.size());
    file.close();

    EXPECT_TRUE(repair.needsRepair(partial.string()));
}

TEST(ModelRepairTest, NeedsRepairReturnsTrueForWrongMagicNumber) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    // 間違ったマジックナンバー
    fs::path wrong_magic = guard.path / "wrong_magic.gguf";
    std::ofstream file(wrong_magic, std::ios::binary);
    file.write("GGML", 4);  // GGMLフォーマット（旧形式）
    uint32_t version = 3;
    file.write(reinterpret_cast<const char*>(&version), sizeof(version));
    std::vector<char> padding(2048 - 8, 0);
    file.write(padding.data(), padding.size());
    file.close();

    EXPECT_TRUE(repair.needsRepair(wrong_magic.string()));
}

TEST(ModelRepairTest, ValidGgufWithDifferentVersions) {
    TempDirGuard guard;
    ModelSync sync("http://localhost:9999", guard.path.string());
    ModelDownloader downloader("http://localhost:9999", guard.path.string());
    ModelStorage storage(guard.path.string());

    ModelRepair repair(sync, downloader, storage);

    // バージョン2のGGUF
    fs::path v2 = guard.path / "v2.gguf";
    std::ofstream file(v2, std::ios::binary);
    file.write("GGUF", 4);
    uint32_t version = 2;
    file.write(reinterpret_cast<const char*>(&version), sizeof(version));
    std::vector<char> padding(2048 - 8, 0);
    file.write(padding.data(), padding.size());
    file.close();

    // ヘッダーが正しければバージョンに関わらず有効
    EXPECT_FALSE(repair.needsRepair(v2.string()));
}
