//! assistant サブコマンドの CLI 引数型定義。

use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

/// Arguments for the assistant subcommand
#[derive(Args, Debug, Clone)]
pub struct AssistantArgs {
    /// Assistant helper subcommand
    #[command(subcommand)]
    pub command: AssistantCommand,
}

/// Assistant subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum AssistantCommand {
    /// Execute curl command with safety checks and optional auth injection
    Curl(CurlArgs),
    /// Print OpenAPI spec (JSON)
    Openapi(OpenApiArgs),
    /// Print API guide text
    Guide(GuideArgs),
}

/// Arguments for `assistant curl`
#[derive(Args, Debug, Clone)]
pub struct CurlArgs {
    /// curl command to execute
    #[arg(long)]
    pub command: String,

    /// Disable automatic auth header injection
    #[arg(long, default_value_t = false)]
    pub no_auto_auth: bool,

    /// Request timeout in seconds (1-300)
    #[arg(long)]
    pub timeout: Option<u64>,

    /// Output as JSON (compatible with automation)
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// Arguments for `assistant openapi`
#[derive(Args, Debug, Clone)]
pub struct OpenApiArgs {
    /// Path to OpenAPI file (YAML/JSON)
    #[arg(long)]
    pub path: Option<PathBuf>,
}

/// Arguments for `assistant guide`
#[derive(Args, Debug, Clone)]
pub struct GuideArgs {
    /// Guide category
    #[arg(long, value_enum)]
    pub category: GuideCategory,
}

/// Guide categories
#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum GuideCategory {
    /// API overview and auth notes
    Overview,
    /// OpenAI-compatible /v1/* APIs
    #[value(name = "openai-compatible")]
    OpenAiCompatible,
    /// /api/endpoints APIs
    #[value(name = "endpoint-management")]
    EndpointManagement,
    /// /api/models/* APIs
    #[value(name = "model-management")]
    ModelManagement,
    /// /api/dashboard/* APIs
    Dashboard,
}
