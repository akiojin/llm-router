#include <gtest/gtest.h>
#include <spdlog/sinks/ostream_sink.h>
#include <spdlog/spdlog.h>
#include <sstream>

#include "utils/json_utils.h"
#include "utils/logger.h"
#include "utils/system_info.h"

using namespace ollama_node;

TEST(LoggerTest, InitSetsLevelAndWritesToSink) {
    std::stringstream ss;
    auto sink = std::make_shared<spdlog::sinks::ostream_sink_mt>(ss);
    ollama_node::logger::init("debug", "%v", "", {sink});

    spdlog::info("hello");
    EXPECT_EQ(spdlog::default_logger()->level(), spdlog::level::debug);
    auto output = ss.str();
    EXPECT_NE(output.find("hello"), std::string::npos);
}

TEST(JsonUtilsTest, ParseJsonHandlesInvalid) {
    std::string error;
    auto ok = parse_json(R"({"a":1})", &error);
    ASSERT_TRUE(ok.has_value());
    EXPECT_EQ(ok->at("a").get<int>(), 1);

    auto bad = parse_json("{invalid", &error);
    EXPECT_FALSE(bad.has_value());
    EXPECT_FALSE(error.empty());
}

TEST(JsonUtilsTest, HasRequiredKeysAndFallbacks) {
    nlohmann::json j = {{"name", "node"}, {"port", 8080}};
    std::string missing;
    EXPECT_TRUE(has_required_keys(j, {"name", "port"}, &missing));
    EXPECT_TRUE(missing.empty());

    EXPECT_FALSE(has_required_keys(j, {"name", "port", "host"}, &missing));
    EXPECT_EQ(missing, "host");

    EXPECT_EQ(get_or<int>(j, "port", 0), 8080);
    EXPECT_EQ(get_or<std::string>(j, "host", "localhost"), "localhost");
}

TEST(SystemInfoTest, CollectProvidesBasicInfo) {
    auto info = collect_system_info();
    EXPECT_FALSE(info.os.empty());
    EXPECT_FALSE(info.arch.empty());
    EXPECT_GT(info.cpu_cores, 0u);
    // Some platforms may not expose total memory; allow zero but prefer positive.
    EXPECT_GE(info.total_memory_bytes, 0u);

    auto summary = format_system_info(info);
    EXPECT_NE(summary.find("os="), std::string::npos);
    EXPECT_NE(summary.find("arch="), std::string::npos);
}
