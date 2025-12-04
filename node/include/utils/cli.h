#pragma once

#include <string>

namespace llm_node {

/// Result of CLI argument parsing
struct CliResult {
    /// Whether the program should exit immediately (e.g., after --help or --version)
    bool should_exit{false};

    /// Exit code to use if should_exit is true
    int exit_code{0};

    /// Output message to display (help text, version info, or error message)
    std::string output;
};

/// Parse command line arguments
///
/// @param argc Number of arguments
/// @param argv Argument values
/// @return CliResult indicating whether to continue or exit
CliResult parseCliArgs(int argc, char* argv[]);

/// Get the help message for the CLI
///
/// @return Help message string
std::string getHelpMessage();

/// Get the version message for the CLI
///
/// @return Version message string
std::string getVersionMessage();

}  // namespace llm_node
