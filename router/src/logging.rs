//! ロギング初期化ユーティリティ
//!
//! `tracing` による構造化ロギングを標準出力とJSONライン形式ファイルへ出力する。

use chrono::Local;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Error, ErrorKind},
    path::PathBuf,
    sync::OnceLock,
};
use tracing_appender::{non_blocking, non_blocking::WorkerGuard};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// ログファイルベース名（JSON Lines）
pub const LOG_FILE_BASE: &str = "llm-router.jsonl";

const LOG_SUBDIR: &str = "logs";
const DEFAULT_DATA_DIR: &str = ".llm-router";
const DEFAULT_RETENTION_DAYS: u32 = 7;

// 環境変数名（新しい名前）
const LLM_ROUTER_LOG_DIR_ENV: &str = "LLM_ROUTER_LOG_DIR";
const LLM_ROUTER_LOG_LEVEL_ENV: &str = "LLM_ROUTER_LOG_LEVEL";
const LLM_ROUTER_LOG_RETENTION_DAYS_ENV: &str = "LLM_ROUTER_LOG_RETENTION_DAYS";
// レガシー環境変数（非推奨）
const LEGACY_LOG_DIR_ENV: &str = "LLM_LOG_DIR";
const LEGACY_LOG_LEVEL_ENV: &str = "LLM_LOG_LEVEL";
const LEGACY_DATA_DIR_ENV: &str = "LLM_ROUTER_DATA_DIR";
const LEGACY_LOG_RETENTION_DAYS_ENV: &str = "LLM_LOG_RETENTION_DAYS";
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

/// ログディレクトリのパスを返す。
pub fn log_dir() -> io::Result<PathBuf> {
    // 新しい環境変数名を優先
    if let Ok(dir) = env::var(LLM_ROUTER_LOG_DIR_ENV) {
        return Ok(PathBuf::from(dir));
    }

    // レガシー環境変数（非推奨）をチェック
    if let Ok(dir) = env::var(LEGACY_LOG_DIR_ENV) {
        tracing::warn!(
            "Environment variable '{}' is deprecated, use '{}' instead",
            LEGACY_LOG_DIR_ENV,
            LLM_ROUTER_LOG_DIR_ENV
        );
        return Ok(PathBuf::from(dir));
    }

    // レガシーデータディレクトリをチェック
    if let Ok(dir) = env::var(LEGACY_DATA_DIR_ENV) {
        return Ok(PathBuf::from(dir).join(LOG_SUBDIR));
    }

    // デフォルト: ~/.llm-router/logs
    let home = env::var("HOME")
        .or_else(|_| env::var("USERPROFILE"))
        .map_err(|_| Error::new(ErrorKind::NotFound, "Failed to resolve home directory"))?;

    Ok(PathBuf::from(home).join(DEFAULT_DATA_DIR).join(LOG_SUBDIR))
}

/// 今日のログファイルのパスを返す。
pub fn log_file_path() -> io::Result<PathBuf> {
    let today = Local::now().format("%Y-%m-%d").to_string();
    let filename = format!("{}.{}", LOG_FILE_BASE, today);
    Ok(log_dir()?.join(filename))
}

/// 保持日数を取得する。
fn get_retention_days() -> u32 {
    // 新しい環境変数名を優先
    if let Ok(val) = env::var(LLM_ROUTER_LOG_RETENTION_DAYS_ENV) {
        if let Ok(days) = val.parse() {
            return days;
        }
    }

    // レガシー環境変数をチェック
    if let Ok(val) = env::var(LEGACY_LOG_RETENTION_DAYS_ENV) {
        if let Ok(days) = val.parse() {
            // Note: ログ初期化前なのでwarningはスキップ
            return days;
        }
    }

    DEFAULT_RETENTION_DAYS
}

/// 古いログファイルを削除する。
fn cleanup_old_logs(log_dir: &PathBuf, retention_days: u32) -> io::Result<()> {
    if !log_dir.exists() {
        return Ok(());
    }

    let cutoff = Local::now() - chrono::Duration::days(retention_days as i64);
    let cutoff_str = cutoff.format("%Y-%m-%d").to_string();

    for entry in fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
            // llm-router.jsonl.YYYY-MM-DD 形式をチェック
            if filename.starts_with(LOG_FILE_BASE) {
                if let Some(date_part) = filename.strip_prefix(&format!("{}.", LOG_FILE_BASE)) {
                    if date_part < cutoff_str.as_str() {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
    }
    Ok(())
}

fn configure_logger() -> io::Result<LoggerGuard> {
    let log_directory = log_dir()?;
    fs::create_dir_all(&log_directory)?;

    // 古いログを削除
    let retention_days = get_retention_days();
    cleanup_old_logs(&log_directory, retention_days)?;

    let log_path = log_file_path()?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let (file_writer, file_guard) = non_blocking(file);

    // 環境変数からログレベルを取得（優先順位: LLM_ROUTER_LOG_LEVEL > LLM_LOG_LEVEL > RUST_LOG）
    let env_filter = EnvFilter::try_from_env(LLM_ROUTER_LOG_LEVEL_ENV)
        .or_else(|_| EnvFilter::try_from_env(LEGACY_LOG_LEVEL_ENV))
        .or_else(|_| EnvFilter::try_from_env(ALT_LEVEL_ENV))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // ファイル出力レイヤー（JSON形式）
    let file_layer = fmt::layer()
        .json()
        .with_writer(file_writer)
        .with_current_span(false)
        .with_span_list(false)
        .with_target(true)
        .with_file(false)
        .with_line_number(false);

    // 標準出力レイヤー（人間が読みやすい形式）
    let stdout_layer = fmt::layer()
        .with_target(true)
        .with_file(false)
        .with_line_number(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stdout_layer)
        .try_init()
        .map_err(Error::other)?;

    tracing::info!(
        category = "system",
        "Router logs initialized: {}",
        log_path.display()
    );

    Ok(LoggerGuard {
        _file_guard: file_guard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_log_dir_uses_new_env() {
        let temp_dir = tempfile::tempdir().unwrap();
        env::remove_var(LEGACY_DATA_DIR_ENV);
        env::remove_var(LEGACY_LOG_DIR_ENV);
        env::set_var(LLM_ROUTER_LOG_DIR_ENV, temp_dir.path());
        let dir = log_dir().unwrap();
        assert_eq!(dir, temp_dir.path());
        env::remove_var(LLM_ROUTER_LOG_DIR_ENV);
    }

    #[test]
    #[serial]
    fn test_log_dir_uses_legacy_env() {
        let temp_dir = tempfile::tempdir().unwrap();
        env::remove_var(LLM_ROUTER_LOG_DIR_ENV);
        env::remove_var(LEGACY_LOG_DIR_ENV);
        env::set_var(LEGACY_DATA_DIR_ENV, temp_dir.path());
        let dir = log_dir().unwrap();
        assert_eq!(dir, temp_dir.path().join(LOG_SUBDIR));
        env::remove_var(LEGACY_DATA_DIR_ENV);
    }

    #[test]
    #[serial]
    fn test_log_file_path_contains_date() {
        let temp_dir = tempfile::tempdir().unwrap();
        env::remove_var(LEGACY_DATA_DIR_ENV);
        env::remove_var(LEGACY_LOG_DIR_ENV);
        env::set_var(LLM_ROUTER_LOG_DIR_ENV, temp_dir.path());
        let path = log_file_path().unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(
            filename.starts_with(LOG_FILE_BASE),
            "filename should start with {}: got {}",
            LOG_FILE_BASE,
            filename
        );
        // 日付形式をチェック (llm-router.jsonl.YYYY-MM-DD)
        let today = Local::now().format("%Y-%m-%d").to_string();
        assert!(
            filename.ends_with(&today),
            "filename should end with today's date: got {}",
            filename
        );
        env::remove_var(LLM_ROUTER_LOG_DIR_ENV);
    }

    #[test]
    fn test_cleanup_old_logs() {
        let temp_dir = tempfile::tempdir().unwrap();
        let log_path = temp_dir.path();

        // 古いログファイルを作成
        let old_file = log_path.join(format!("{}.2020-01-01", LOG_FILE_BASE));
        fs::write(&old_file, "old log").unwrap();

        // 新しいログファイルを作成
        let today = Local::now().format("%Y-%m-%d").to_string();
        let new_file = log_path.join(format!("{}.{}", LOG_FILE_BASE, today));
        fs::write(&new_file, "new log").unwrap();

        // クリーンアップ実行
        cleanup_old_logs(&log_path.to_path_buf(), 7).unwrap();

        // 古いファイルは削除され、新しいファイルは残る
        assert!(!old_file.exists(), "old log file should be deleted");
        assert!(new_file.exists(), "new log file should remain");
    }

    #[test]
    #[serial]
    fn test_get_retention_days_default() {
        env::remove_var(LLM_ROUTER_LOG_RETENTION_DAYS_ENV);
        env::remove_var(LEGACY_LOG_RETENTION_DAYS_ENV);
        assert_eq!(get_retention_days(), DEFAULT_RETENTION_DAYS);
    }

    #[test]
    #[serial]
    fn test_get_retention_days_from_env() {
        env::set_var(LLM_ROUTER_LOG_RETENTION_DAYS_ENV, "14");
        env::remove_var(LEGACY_LOG_RETENTION_DAYS_ENV);
        assert_eq!(get_retention_days(), 14);
        env::remove_var(LLM_ROUTER_LOG_RETENTION_DAYS_ENV);
    }
}
