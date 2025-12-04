#include <gtest/gtest.h>
#include <cstdlib>
#include <array>
#include <memory>
#include <string>
#include <stdexcept>

#include "utils/cli.h"
#include "utils/version.h"

using namespace llm_node;

// Test --help flag
TEST(CliTest, HelpFlagShowsHelpMessage) {
    std::vector<std::string> args = {"llm-node", "--help"};
    std::vector<char*> argv;
    for (auto& s : args) argv.push_back(s.data());
    argv.push_back(nullptr);

    CliResult result = parseCliArgs(static_cast<int>(args.size()), argv.data());

    EXPECT_TRUE(result.should_exit);
    EXPECT_EQ(result.exit_code, 0);
    EXPECT_TRUE(result.output.find("llm-node") != std::string::npos);
    EXPECT_TRUE(result.output.find("ENVIRONMENT VARIABLES") != std::string::npos);
}

TEST(CliTest, ShortHelpFlagShowsHelpMessage) {
    std::vector<std::string> args = {"llm-node", "-h"};
    std::vector<char*> argv;
    for (auto& s : args) argv.push_back(s.data());
    argv.push_back(nullptr);

    CliResult result = parseCliArgs(static_cast<int>(args.size()), argv.data());

    EXPECT_TRUE(result.should_exit);
    EXPECT_EQ(result.exit_code, 0);
    EXPECT_TRUE(result.output.find("llm-node") != std::string::npos);
}

// Test --version flag
TEST(CliTest, VersionFlagShowsVersion) {
    std::vector<std::string> args = {"llm-node", "--version"};
    std::vector<char*> argv;
    for (auto& s : args) argv.push_back(s.data());
    argv.push_back(nullptr);

    CliResult result = parseCliArgs(static_cast<int>(args.size()), argv.data());

    EXPECT_TRUE(result.should_exit);
    EXPECT_EQ(result.exit_code, 0);
    EXPECT_TRUE(result.output.find(LLM_NODE_VERSION) != std::string::npos);
}

TEST(CliTest, ShortVersionFlagShowsVersion) {
    std::vector<std::string> args = {"llm-node", "-V"};
    std::vector<char*> argv;
    for (auto& s : args) argv.push_back(s.data());
    argv.push_back(nullptr);

    CliResult result = parseCliArgs(static_cast<int>(args.size()), argv.data());

    EXPECT_TRUE(result.should_exit);
    EXPECT_EQ(result.exit_code, 0);
    EXPECT_TRUE(result.output.find(LLM_NODE_VERSION) != std::string::npos);
}

// Test no arguments (should continue to server mode)
TEST(CliTest, NoArgumentsContinuesToServerMode) {
    std::vector<std::string> args = {"llm-node"};
    std::vector<char*> argv;
    for (auto& s : args) argv.push_back(s.data());
    argv.push_back(nullptr);

    CliResult result = parseCliArgs(static_cast<int>(args.size()), argv.data());

    EXPECT_FALSE(result.should_exit);
}

// Test unknown argument
TEST(CliTest, UnknownArgumentShowsError) {
    std::vector<std::string> args = {"llm-node", "--unknown-flag"};
    std::vector<char*> argv;
    for (auto& s : args) argv.push_back(s.data());
    argv.push_back(nullptr);

    CliResult result = parseCliArgs(static_cast<int>(args.size()), argv.data());

    EXPECT_TRUE(result.should_exit);
    EXPECT_NE(result.exit_code, 0);
    EXPECT_TRUE(result.output.find("unknown") != std::string::npos ||
                result.output.find("Unknown") != std::string::npos ||
                result.output.find("error") != std::string::npos ||
                result.output.find("Error") != std::string::npos);
}

// Test help message contains environment variables
TEST(CliTest, HelpMessageContainsEnvironmentVariables) {
    std::vector<std::string> args = {"llm-node", "--help"};
    std::vector<char*> argv;
    for (auto& s : args) argv.push_back(s.data());
    argv.push_back(nullptr);

    CliResult result = parseCliArgs(static_cast<int>(args.size()), argv.data());

    EXPECT_TRUE(result.output.find("LLM_NODE_MODELS_DIR") != std::string::npos);
    EXPECT_TRUE(result.output.find("LLM_NODE_PORT") != std::string::npos);
    EXPECT_TRUE(result.output.find("LLM_NODE_LOG_LEVEL") != std::string::npos);
}
