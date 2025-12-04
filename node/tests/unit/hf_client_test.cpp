#include <gtest/gtest.h>
#include <filesystem>

#include "models/hf_client.h"

using namespace llm_node;
namespace fs = std::filesystem;

class TempHfDir {
public:
    TempHfDir() {
        auto base = fs::temp_directory_path() / fs::path("hf-cache");
        std::error_code ec;
        fs::create_directories(base, ec);
        path = base;
    }
    ~TempHfDir() {
        std::error_code ec;
        fs::remove_all(path, ec);
    }
    fs::path path;
};

TEST(HfClientTest, ListsDummyFiles) {
    HfClient client("/tmp/hf");
    auto files = client.listFiles("user/model");
    ASSERT_EQ(files.size(), 2u);
    EXPECT_TRUE(client.isGguf(files[0].name));
    EXPECT_TRUE(client.needsConversion(files[1].name));
}

TEST(HfClientTest, DownloadsToCache) {
    TempHfDir tmp;
    HfClient client(tmp.path.string());
    auto out = client.downloadFile("user/model", "model.gguf");
    EXPECT_FALSE(out.empty());
    EXPECT_TRUE(fs::exists(out));
}

TEST(HfClientTest, DetectsLoraAndDiffusers) {
    HfClient client("/tmp/hf");
    EXPECT_TRUE(client.isLora("adapter.safetensors"));
    EXPECT_TRUE(client.isLora("mylora.bin"));
    EXPECT_FALSE(client.isLora("model.gguf"));

    EXPECT_TRUE(client.isDiffusersRepo("user/diffusers-unet"));
    EXPECT_TRUE(client.isDiffusersRepo("user/unet-large"));
    EXPECT_FALSE(client.isDiffusersRepo("user/text2vec"));
}
