//! CLI module for llm-router
//!
//! Provides command-line interface for router management.

pub mod user;

use clap::{Parser, Subcommand};

/// LLM Router - Centralized management system for LLM inference nodes
#[derive(Parser, Debug)]
#[command(name = "llm-router")]
#[command(version, about, long_about = None)]
#[command(after_help = r#"ENVIRONMENT VARIABLES:
    LLM_ROUTER_HOST              Bind address (default: 0.0.0.0)
    LLM_ROUTER_PORT              Listen port (default: 8080)
    LLM_ROUTER_LOG_LEVEL         Log level (default: info)
    LLM_ROUTER_DATABASE_URL      Database URL
    LLM_ROUTER_JWT_SECRET        JWT signing key (auto-generated if not set)
    LLM_ROUTER_ADMIN_USERNAME    Initial admin username (default: admin)
    LLM_ROUTER_ADMIN_PASSWORD    Initial admin password (required on first run)
"#)]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available CLI commands
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage users
    User {
        /// User management subcommand
        #[command(subcommand)]
        command: user::UserCommand,
    },
}
