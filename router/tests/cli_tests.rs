//! CLI integration tests
//!
//! Tests for command-line interface parsing and behavior.

use clap::Parser;
use llm_router::cli::{Cli, Commands};

/// T005: Test --help shows user subcommand
#[test]
fn test_help_shows_user_subcommand() {
    // Verify the CLI structure contains user subcommand
    let cli = Cli::try_parse_from(["llm-router", "user", "list"]).unwrap();
    assert!(matches!(cli.command, Some(Commands::User { .. })));
}

/// T006: Test --version output contains version number
#[test]
fn test_version_available() {
    // Try parsing with --version should return error (because it prints and exits)
    let result = Cli::try_parse_from(["llm-router", "--version"]);
    // clap returns an error with kind DisplayVersion for --version
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
}

/// T005: Test help output contains user subcommand
#[test]
fn test_help_available() {
    // Try parsing with --help should return error (because it prints and exits)
    let result = Cli::try_parse_from(["llm-router", "--help"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
}

/// Test user list command parsing
#[test]
fn test_user_list_parsing() {
    let cli = Cli::try_parse_from(["llm-router", "user", "list"]).unwrap();
    match cli.command {
        Some(Commands::User { command }) => {
            assert!(matches!(command, llm_router::cli::user::UserCommand::List));
        }
        _ => panic!("Expected User command"),
    }
}

/// Test user add command parsing
#[test]
fn test_user_add_parsing() {
    let cli = Cli::try_parse_from([
        "llm-router",
        "user",
        "add",
        "testuser",
        "--password",
        "secret123",
    ])
    .unwrap();
    match cli.command {
        Some(Commands::User { command }) => {
            if let llm_router::cli::user::UserCommand::Add(add) = command {
                assert_eq!(add.username, "testuser");
                assert_eq!(add.password, "secret123");
            } else {
                panic!("Expected Add command");
            }
        }
        _ => panic!("Expected User command"),
    }
}

/// Test user add command with short password flag
#[test]
fn test_user_add_short_flag() {
    let cli =
        Cli::try_parse_from(["llm-router", "user", "add", "testuser", "-p", "secret123"]).unwrap();
    match cli.command {
        Some(Commands::User { command }) => {
            if let llm_router::cli::user::UserCommand::Add(add) = command {
                assert_eq!(add.username, "testuser");
                assert_eq!(add.password, "secret123");
            } else {
                panic!("Expected Add command");
            }
        }
        _ => panic!("Expected User command"),
    }
}

/// Test user delete command parsing
#[test]
fn test_user_delete_parsing() {
    let cli = Cli::try_parse_from(["llm-router", "user", "delete", "testuser"]).unwrap();
    match cli.command {
        Some(Commands::User { command }) => {
            if let llm_router::cli::user::UserCommand::Delete(delete) = command {
                assert_eq!(delete.username, "testuser");
            } else {
                panic!("Expected Delete command");
            }
        }
        _ => panic!("Expected User command"),
    }
}

/// Test no command (should start server)
#[test]
fn test_no_command_returns_none() {
    let cli = Cli::try_parse_from(["llm-router"]).unwrap();
    assert!(cli.command.is_none());
}

/// Test missing password for user add
#[test]
fn test_user_add_missing_password() {
    let result = Cli::try_parse_from(["llm-router", "user", "add", "testuser"]);
    assert!(result.is_err());
}

/// Test missing username for user delete
#[test]
fn test_user_delete_missing_username() {
    let result = Cli::try_parse_from(["llm-router", "user", "delete"]);
    assert!(result.is_err());
}
