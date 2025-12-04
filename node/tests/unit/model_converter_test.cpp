#include <gtest/gtest.h>
#include <filesystem>
#include <fstream>

#include "models/model_converter.h"

using namespace llm_node;
namespace fs = std::filesystem;

class TempDir {
public:
    TempDir() {
        auto base = fs::temp_directory_path();
        for (int i = 0; i < 10; ++i) {
            auto candidate = base / fs::path("conv-" + std::to_string(std::rand()));
            std::error_code ec;
            if (fs::create_directories(candidate, ec)) {
                path = candidate;
                return;
            }
        }
        path = base;
    }
    ~TempDir() {
        std::error_code ec;
        fs::remove_all(path, ec);
    }
    fs::path path;
};

TEST(ModelConverterTest, ConvertsPyTorchToGguf) {
    TempDir tmp;
    fs::path src = tmp.path / "model.bin";
    std::ofstream(src) << "bin";

    ModelConverter conv(tmp.path.string());
    auto out = conv.convertPyTorchToGguf(src, "m1");
    EXPECT_FALSE(out.empty());
    EXPECT_TRUE(fs::exists(out));
    EXPECT_TRUE(conv.isConverted("m1"));
}

TEST(ModelConverterTest, ConvertsSafetensorsToGguf) {
    TempDir tmp;
    fs::path src = tmp.path / "model.safetensors";
    std::ofstream(src) << "safe";

    ModelConverter conv(tmp.path.string());
    auto out = conv.convertSafetensorsToGguf(src, "m2");
    EXPECT_FALSE(out.empty());
    EXPECT_TRUE(fs::exists(out));
}

TEST(ModelConverterTest, IsConvertedDetectsExistingGguf) {
    TempDir tmp;
    fs::create_directories(tmp.path / "m3");
    std::ofstream(tmp.path / "m3" / "m3.gguf") << "gguf";
    ModelConverter conv(tmp.path.string());
    EXPECT_TRUE(conv.isConverted("m3"));
}

TEST(ModelConverterTest, UsesCacheWhenProvided) {
    TempDir tmp;
    ModelConverter conv(tmp.path.string());
    conv.setCache({{"cached", (tmp.path / "cached" / "cached.gguf").string()}});
    std::filesystem::create_directories(tmp.path / "cached");
    std::ofstream(tmp.path / "cached" / "cached.gguf") << "gguf";
    EXPECT_TRUE(conv.isConverted("cached"));
}

TEST(ModelConverterTest, ProgressCallbackCalledOnConvert) {
    TempDir tmp;
    fs::path src = tmp.path / "model.bin";
    std::ofstream(src) << "bin";
    ModelConverter conv(tmp.path.string());
    double last = 0.0;
    conv.setProgressCallback([&](const std::string&, double p) { last = p; });
    conv.convertPyTorchToGguf(src, "m4");
    EXPECT_DOUBLE_EQ(last, 1.0);
}
