#include <gtest/gtest.h>
#include <filesystem>
#include <fstream>

#include "models/ollama_compat.h"

using namespace ollama_node;
namespace fs = std::filesystem;

class TempModelDir {
public:
    TempModelDir() {
        base = fs::temp_directory_path() / fs::path("ollama-compat-XXXXXX");
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

static void write_manifest(const fs::path& dir, const std::string& digest, const std::string& gguf_rel) {
    fs::create_directories(dir);
    std::ofstream ofs(dir / "manifest.json");
    ofs << R"({"files":[{"type":"gguf","digest":")" << digest << R"(","path":")" << gguf_rel << R"(","name":")" << fs::path(gguf_rel).filename().string() << R"("}]})";
    fs::create_directories((dir / gguf_rel).parent_path());
    std::ofstream(dir / gguf_rel) << "dummy";
}

TEST(OllamaCompatTest, ListsAvailableManifestsWithExistingGguf) {
    TempModelDir tmp;
    write_manifest(tmp.base / "modelA", "sha123", "gguf/model.gguf");
    write_manifest(tmp.base / "modelB", "sha456", "model.gguf");

    OllamaCompat compat(tmp.base.string());
    auto list = compat.listAvailable();
    ASSERT_EQ(list.size(), 2u);
    EXPECT_TRUE((list[0].name == "modelA" && list[1].name == "modelB") ||
                (list[0].name == "modelB" && list[1].name == "modelA"));
}

TEST(OllamaCompatTest, ResolveReturnsEmptyWhenManifestMissing) {
    TempModelDir tmp;
    fs::create_directories(tmp.base / "nomani");
    OllamaCompat compat(tmp.base.string());
    EXPECT_EQ(compat.resolveGguf("nomani"), "");
}

TEST(OllamaCompatTest, ResolveReturnsPathWhenPresent) {
    TempModelDir tmp;
    write_manifest(tmp.base / "modelC", "sha789", "files/m.gguf");
    OllamaCompat compat(tmp.base.string());
    auto path = compat.resolveGguf("modelC");
    EXPECT_FALSE(path.empty());
    EXPECT_TRUE(fs::exists(path));
}

TEST(OllamaCompatTest, LoadMetadataReturnsJson) {
    TempModelDir tmp;
    write_manifest(tmp.base / "modelE", "sha999", "files/e.gguf");
    OllamaCompat compat(tmp.base.string());
    auto meta = compat.loadMetadata("modelE");
    ASSERT_TRUE(meta.has_value());
    EXPECT_TRUE(meta->contains("files"));
}

TEST(OllamaCompatTest, IgnoresGgufMissingOnDisk) {
    TempModelDir tmp;
    fs::create_directories(tmp.base / "modelD");
    std::ofstream(tmp.base / "modelD" / "manifest.json") << R"({"files":[{"type":"gguf","digest":"x","path":"missing.gguf"}]})";
    OllamaCompat compat(tmp.base.string());
    auto list = compat.listAvailable();
    EXPECT_TRUE(list.empty());
}

TEST(OllamaCompatTest, ValidateModelChecksDigest) {
    TempModelDir tmp;
    auto model_dir = tmp.base / "modelF";
    fs::create_directories(model_dir / "gguf");
    std::ofstream(model_dir / "gguf" / "file.gguf") << "abc";

    // sha256 of "abc"
    std::string digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    std::ofstream(model_dir / "manifest.json") << R"({"files":[{"type":"gguf","digest":")" << digest << R"(","path":"gguf/file.gguf"}]})";

    OllamaCompat compat(tmp.base.string());
    EXPECT_TRUE(compat.validateModel("modelF"));

    // wrong digest
    std::ofstream(model_dir / "manifest.json") << R"({"files":[{"type":"gguf","digest":"dead","path":"gguf/file.gguf"}]})";
    EXPECT_FALSE(compat.validateModel("modelF"));
}
