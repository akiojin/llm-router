//! ロギング初期化ユーティリティ
//!
//! `tracing` による構造化ロギングを標準出力とJSONライン形式ファイルへ出力する。

use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Error, ErrorKind},
    path::PathBuf,
    sync::OnceLock,
};
use tracing_appender::{non_blocking, non_blocking::WorkerGuard};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// ログファイル名（JSON Lines）
pub const LOG_FILE_NAME: &str = "coordinator.log.jsonl";

const LOG_SUBDIR: &str = "logs";
const LEVEL_ENV: &str = "COORDINATOR_LOG_LEVEL";
const ALT_LEVEL_ENV: &str = "RUST_LOG";

static LOGGER_GUARD: OnceLock<Result<LoggerGuard, io::Error>> = OnceLock::new();

struct LoggerGuard {
    _file_guard: WorkerGuard,
}

/// ログ出力を初期化する。
pub fn init() -> io::Result<()> {
    match LOGGER_GUARD.get_or_init(configure_logger) {
        Ok(_) => Ok(()),
        Err(err) => Err(io::Error::new(err.kind(), err.to_string())),
    }
}

/// ログファイルのパスを返す。
pub fn log_file_path() -> io::Result<PathBuf> {
    Ok(resolve_data_dir()?.join(LOG_SUBDIR).join(LOG_FILE_NAME))
}

fn resolve_data_dir() -> io::Result<PathBuf> {
    if let Ok(dir) = env::var("OLLAMA_COORDINATOR_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }

    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map_err(|_| Error::new(ErrorKind::NotFound, "Failed to resolve home directory"))?;

    Ok(PathBuf::from(home).join(".ollama-coordinator"))
}

fn configure_logger() -> io::Result<LoggerGuard> {
    let log_path = log_file_path()?;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let (file_writer, file_guard) = non_blocking(file);

    let env_filter = EnvFilter::try_from_env(LEVEL_ENV)
        .or_else(|_| EnvFilter::try_from_env(ALT_LEVEL_ENV))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = fmt::layer()
        .json()
        .with_writer(file_writer)
        .with_current_span(false)
        .with_span_list(false)
        .with_target(true)
        .with_file(false)
        .with_line_number(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .try_init()
        .map_err(Error::other)?;

    tracing::info!("Coordinator logs will be written to {}", log_path.display());

    Ok(LoggerGuard {
        _file_guard: file_guard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_data_dir_uses_env_override() {
        let temp_dir = tempfile::tempdir().unwrap();
        env::set_var("OLLAMA_COORDINATOR_DATA_DIR", temp_dir.path());
        let dir = resolve_data_dir().unwrap();
        assert_eq!(dir, temp_dir.path());
        env::remove_var("OLLAMA_COORDINATOR_DATA_DIR");
    }

    #[test]
    fn test_log_file_path_contains_logs_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        env::set_var("OLLAMA_COORDINATOR_DATA_DIR", temp_dir.path());
        let path = log_file_path().unwrap();
        assert!(
            path.ends_with(std::path::Path::new("logs").join(LOG_FILE_NAME)),
            "unexpected log path: {:?}",
            path
        );
        env::remove_var("OLLAMA_COORDINATOR_DATA_DIR");
    }
}
