// SPEC-dcaeaec4: ModelStorage unit tests (TDD RED phase)
#include <gtest/gtest.h>
#include <filesystem>
#include <fstream>

#include "models/model_storage.h"

using namespace llm_node;
namespace fs = std::filesystem;

class TempModelDir {
public:
    TempModelDir() {
        base = fs::temp_directory_path() / fs::path("model-storage-XXXXXX");
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

// Helper: create model directory with model.gguf
static void create_model(const fs::path& models_dir, const std::string& dir_name) {
    auto model_dir = models_dir / dir_name;
    fs::create_directories(model_dir);
    std::ofstream(model_dir / "model.gguf") << "dummy gguf content";
}

// FR-2: Model name format conversion (colon to underscore)
TEST(ModelStorageTest, ConvertModelNameToDirectoryName) {
    EXPECT_EQ(ModelStorage::modelNameToDir("gpt-oss:20b"), "gpt-oss_20b");
    EXPECT_EQ(ModelStorage::modelNameToDir("gpt-oss:7b"), "gpt-oss_7b");
    EXPECT_EQ(ModelStorage::modelNameToDir("qwen3-coder:30b"), "qwen3-coder_30b");
}

// FR-2: Default tag is "latest"
TEST(ModelStorageTest, DefaultTagIsLatest) {
    EXPECT_EQ(ModelStorage::modelNameToDir("gpt-oss"), "gpt-oss_latest");
    EXPECT_EQ(ModelStorage::modelNameToDir("llama3"), "llama3_latest");
}

// FR-3: resolveGguf returns correct path
TEST(ModelStorageTest, ResolveGgufReturnsPathWhenPresent) {
    TempModelDir tmp;
    create_model(tmp.base, "gpt-oss_20b");

    ModelStorage storage(tmp.base.string());
    auto path = storage.resolveGguf("gpt-oss:20b");

    EXPECT_FALSE(path.empty());
    EXPECT_TRUE(fs::exists(path));
    EXPECT_EQ(fs::path(path).filename(), "model.gguf");
}

// FR-3: resolveGguf returns empty when model not found
TEST(ModelStorageTest, ResolveGgufReturnsEmptyWhenMissing) {
    TempModelDir tmp;
    ModelStorage storage(tmp.base.string());
    EXPECT_EQ(storage.resolveGguf("nonexistent:model"), "");
}

// FR-4: listAvailable returns all models with model.gguf
TEST(ModelStorageTest, ListAvailableReturnsAllModels) {
    TempModelDir tmp;
    create_model(tmp.base, "gpt-oss_20b");
    create_model(tmp.base, "gpt-oss_7b");
    create_model(tmp.base, "qwen3-coder_30b");

    ModelStorage storage(tmp.base.string());
    auto list = storage.listAvailable();

    ASSERT_EQ(list.size(), 3u);

    std::vector<std::string> names;
    for (const auto& m : list) {
        names.push_back(m.name);
    }
    std::sort(names.begin(), names.end());

    EXPECT_EQ(names[0], "gpt-oss:20b");
    EXPECT_EQ(names[1], "gpt-oss:7b");
    EXPECT_EQ(names[2], "qwen3-coder:30b");
}

// FR-4: Directories without model.gguf are ignored
TEST(ModelStorageTest, IgnoresDirectoriesWithoutGguf) {
    TempModelDir tmp;
    create_model(tmp.base, "valid_model");
    // Create directory without model.gguf
    fs::create_directories(tmp.base / "invalid_model");

    ModelStorage storage(tmp.base.string());
    auto list = storage.listAvailable();

    ASSERT_EQ(list.size(), 1u);
    EXPECT_EQ(list[0].name, "valid:model");
}

// FR-5: Load optional metadata
TEST(ModelStorageTest, LoadMetadataWhenPresent) {
    TempModelDir tmp;
    create_model(tmp.base, "gpt-oss_20b");
    std::ofstream(tmp.base / "gpt-oss_20b" / "metadata.json") << R"({"size_gb": 40})";

    ModelStorage storage(tmp.base.string());
    auto meta = storage.loadMetadata("gpt-oss:20b");

    ASSERT_TRUE(meta.has_value());
    EXPECT_EQ((*meta)["size_gb"].get<int>(), 40);
}

// FR-5: Metadata is optional - returns nullopt when missing
TEST(ModelStorageTest, LoadMetadataReturnsNulloptWhenMissing) {
    TempModelDir tmp;
    create_model(tmp.base, "gpt-oss_20b");

    ModelStorage storage(tmp.base.string());
    auto meta = storage.loadMetadata("gpt-oss:20b");

    EXPECT_FALSE(meta.has_value());
}

// Edge case: Handle multiple colons in model name
TEST(ModelStorageTest, HandleMultipleColonsInName) {
    EXPECT_EQ(ModelStorage::modelNameToDir("org:model:tag"), "org_model_tag");
}

// Edge case: Empty model name
TEST(ModelStorageTest, HandleEmptyModelName) {
    EXPECT_EQ(ModelStorage::modelNameToDir(""), "_latest");
}

// Validation: Model with valid GGUF file
TEST(ModelStorageTest, ValidateModelWithGguf) {
    TempModelDir tmp;
    create_model(tmp.base, "gpt-oss_20b");

    ModelStorage storage(tmp.base.string());
    EXPECT_TRUE(storage.validateModel("gpt-oss:20b"));
    EXPECT_FALSE(storage.validateModel("nonexistent:model"));
}

// Directory conversion: underscore to colon (reverse)
TEST(ModelStorageTest, ConvertDirNameToModelName) {
    EXPECT_EQ(ModelStorage::dirNameToModel("gpt-oss_20b"), "gpt-oss:20b");
    EXPECT_EQ(ModelStorage::dirNameToModel("qwen3-coder_30b"), "qwen3-coder:30b");
}
