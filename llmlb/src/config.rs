//! Configuration management via environment variables
//!
//! Provides helper functions for reading environment variables with fallback
//! to deprecated variable names with warning logs.

use std::time::Duration;

/// Get an environment variable with fallback to a deprecated name
///
/// If the new variable name is set, returns its value.
/// If only the old (deprecated) variable name is set, returns its value
/// and logs a deprecation warning.
///
/// # Arguments
/// * `new_name` - The new environment variable name (preferred)
/// * `old_name` - The deprecated environment variable name (fallback)
///
/// # Returns
/// * `Some(value)` - The environment variable value
/// * `None` - Neither variable is set
///
/// # Example
/// ```
/// use llmlb::config::get_env_with_fallback;
///
/// let port = get_env_with_fallback("LLMLB_PORT", "LLMLB_PORT");
/// ```
pub fn get_env_with_fallback(new_name: &str, old_name: &str) -> Option<String> {
    if let Ok(val) = std::env::var(new_name) {
        return Some(val);
    }
    if let Ok(val) = std::env::var(old_name) {
        tracing::warn!(
            "Environment variable '{}' is deprecated, use '{}' instead",
            old_name,
            new_name
        );
        return Some(val);
    }
    None
}

/// Get an environment variable with fallback and default value
///
/// Similar to `get_env_with_fallback`, but returns a default value
/// if neither variable is set.
///
/// # Arguments
/// * `new_name` - The new environment variable name (preferred)
/// * `old_name` - The deprecated environment variable name (fallback)
/// * `default` - The default value to return if neither is set
///
/// # Returns
/// The environment variable value or the default
pub fn get_env_with_fallback_or(new_name: &str, old_name: &str, default: &str) -> String {
    get_env_with_fallback(new_name, old_name).unwrap_or_else(|| default.to_string())
}

/// Get an environment variable with fallback, parsing to a specific type
///
/// # Arguments
/// * `new_name` - The new environment variable name (preferred)
/// * `old_name` - The deprecated environment variable name (fallback)
/// * `default` - The default value to return if neither is set or parsing fails
///
/// # Returns
/// The parsed environment variable value or the default
pub fn get_env_with_fallback_parse<T: std::str::FromStr>(
    new_name: &str,
    old_name: &str,
    default: T,
) -> T {
    get_env_with_fallback(new_name, old_name)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// Queueing configuration (request wait queue)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueConfig {
    /// Maximum number of requests allowed to wait in the queue.
    pub max_waiters: usize,
    /// Maximum time a request may wait in the queue before timing out.
    pub timeout: Duration,
}

impl QueueConfig {
    /// Load queue configuration from environment variables.
    pub fn from_env() -> Self {
        let max_waiters = get_env_with_fallback_parse("LLMLB_QUEUE_MAX", "QUEUE_MAX", 100usize);
        let timeout_secs =
            get_env_with_fallback_parse("LLMLB_QUEUE_TIMEOUT_SECS", "QUEUE_TIMEOUT_SECS", 60u64);

        Self {
            max_waiters,
            timeout: Duration::from_secs(timeout_secs),
        }
    }
}

/// デフォルトembeddingモデルを取得
///
/// 環境変数 `LLMLB_DEFAULT_EMBEDDING_MODEL`（旧: `LLM_DEFAULT_EMBEDDING_MODEL`）から取得し、
/// 未設定の場合は `nomic-embed-text-v1.5` を返す。
pub fn get_default_embedding_model() -> String {
    get_env_with_fallback(
        "LLMLB_DEFAULT_EMBEDDING_MODEL",
        "LLM_DEFAULT_EMBEDDING_MODEL",
    )
    .unwrap_or_else(|| "nomic-embed-text-v1.5".to_string())
}

/// エンドポイントのモデル自動同期の最短間隔を取得
///
/// ヘルスチェック成功時にモデル同期（`GET /v1/models` + DB反映）を実行する際、
/// 同一エンドポイントに対して過剰に同期しないためのスロットリングに使用する。
///
/// 環境変数 `LLMLB_AUTO_SYNC_MODELS_INTERVAL_SECS` から取得し、
/// 未設定の場合は 15 分（900 秒）を使用する。
pub fn get_auto_sync_models_interval() -> Duration {
    let secs = get_env_with_fallback_parse(
        "LLMLB_AUTO_SYNC_MODELS_INTERVAL_SECS",
        "AUTO_SYNC_MODELS_INTERVAL_SECS",
        900u64,
    );
    Duration::from_secs(secs)
}

/// サーバーのホスト・ポート設定
#[derive(Clone)]
pub struct ServerConfig {
    /// バインドするホストアドレス
    pub host: String,
    /// バインドするポート番号
    pub port: u16,
}

impl ServerConfig {
    /// 環境変数からサーバー設定を読み込む
    pub fn from_env() -> Self {
        let host = get_env_with_fallback_or("LLMLB_HOST", "LLMLB_HOST", "0.0.0.0");
        let port = get_env_with_fallback_parse("LLMLB_PORT", "LLMLB_PORT", 32768);
        Self { host, port }
    }

    /// コマンドライン引数からサーバー設定を作成する
    pub fn from_args(host: String, port: u16) -> Self {
        Self { host, port }
    }

    /// バインドアドレス文字列を返す
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
impl ServerConfig {
    /// ローカル接続用のホストアドレスを返す
    pub fn local_host(&self) -> String {
        match self.host.as_str() {
            "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
            other => other.to_string(),
        }
    }

    /// ベースURLを返す
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.local_host(), self.port)
    }

    /// ダッシュボードURLを返す
    pub fn dashboard_url(&self) -> String {
        format!("{}/dashboard", self.base_url())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_get_env_with_fallback_new_name() {
        std::env::set_var("TEST_NEW_VAR", "new_value");
        std::env::remove_var("TEST_OLD_VAR");

        let result = get_env_with_fallback("TEST_NEW_VAR", "TEST_OLD_VAR");
        assert_eq!(result, Some("new_value".to_string()));

        std::env::remove_var("TEST_NEW_VAR");
    }

    #[test]
    #[serial]
    fn test_get_env_with_fallback_old_name() {
        std::env::remove_var("TEST_NEW_VAR2");
        std::env::set_var("TEST_OLD_VAR2", "old_value");

        let result = get_env_with_fallback("TEST_NEW_VAR2", "TEST_OLD_VAR2");
        assert_eq!(result, Some("old_value".to_string()));

        std::env::remove_var("TEST_OLD_VAR2");
    }

    #[test]
    #[serial]
    fn test_get_env_with_fallback_neither() {
        std::env::remove_var("TEST_NEW_VAR3");
        std::env::remove_var("TEST_OLD_VAR3");

        let result = get_env_with_fallback("TEST_NEW_VAR3", "TEST_OLD_VAR3");
        assert_eq!(result, None);
    }

    #[test]
    #[serial]
    fn test_get_env_with_fallback_new_takes_precedence() {
        std::env::set_var("TEST_NEW_VAR4", "new_value");
        std::env::set_var("TEST_OLD_VAR4", "old_value");

        let result = get_env_with_fallback("TEST_NEW_VAR4", "TEST_OLD_VAR4");
        assert_eq!(result, Some("new_value".to_string()));

        std::env::remove_var("TEST_NEW_VAR4");
        std::env::remove_var("TEST_OLD_VAR4");
    }

    #[test]
    #[serial]
    fn test_get_env_with_fallback_or_default() {
        std::env::remove_var("TEST_NEW_VAR5");
        std::env::remove_var("TEST_OLD_VAR5");

        let result = get_env_with_fallback_or("TEST_NEW_VAR5", "TEST_OLD_VAR5", "default_value");
        assert_eq!(result, "default_value");
    }

    #[test]
    #[serial]
    fn test_get_env_with_fallback_parse() {
        std::env::set_var("TEST_NEW_VAR6", "32768");
        std::env::remove_var("TEST_OLD_VAR6");

        let result: u16 = get_env_with_fallback_parse("TEST_NEW_VAR6", "TEST_OLD_VAR6", 3000);
        assert_eq!(result, 32768);

        std::env::remove_var("TEST_NEW_VAR6");
    }

    #[test]
    #[serial]
    fn test_get_default_embedding_model_default() {
        std::env::remove_var("LLMLB_DEFAULT_EMBEDDING_MODEL");
        std::env::remove_var("LLM_DEFAULT_EMBEDDING_MODEL");
        let result = get_default_embedding_model();
        assert_eq!(result, "nomic-embed-text-v1.5");
    }

    #[test]
    #[serial]
    fn test_get_default_embedding_model_custom_new_name() {
        std::env::set_var("LLMLB_DEFAULT_EMBEDDING_MODEL", "bge-m3");
        std::env::remove_var("LLM_DEFAULT_EMBEDDING_MODEL");
        let result = get_default_embedding_model();
        assert_eq!(result, "bge-m3");
        std::env::remove_var("LLMLB_DEFAULT_EMBEDDING_MODEL");
    }

    #[test]
    #[serial]
    fn test_get_default_embedding_model_custom_old_name() {
        std::env::set_var("LLM_DEFAULT_EMBEDDING_MODEL", "bge-m3");
        let result = get_default_embedding_model();
        assert_eq!(result, "bge-m3");
        std::env::remove_var("LLM_DEFAULT_EMBEDDING_MODEL");
    }

    #[test]
    #[serial]
    fn test_get_default_embedding_model_new_takes_precedence() {
        std::env::set_var("LLMLB_DEFAULT_EMBEDDING_MODEL", "new-model");
        std::env::set_var("LLM_DEFAULT_EMBEDDING_MODEL", "old-model");
        let result = get_default_embedding_model();
        assert_eq!(result, "new-model");
        std::env::remove_var("LLMLB_DEFAULT_EMBEDDING_MODEL");
        std::env::remove_var("LLM_DEFAULT_EMBEDDING_MODEL");
    }

    #[test]
    #[serial]
    fn test_get_auto_sync_models_interval_default() {
        std::env::remove_var("LLMLB_AUTO_SYNC_MODELS_INTERVAL_SECS");
        std::env::remove_var("AUTO_SYNC_MODELS_INTERVAL_SECS");
        assert_eq!(get_auto_sync_models_interval(), Duration::from_secs(900));
    }

    #[test]
    #[serial]
    fn test_get_auto_sync_models_interval_from_env() {
        std::env::set_var("LLMLB_AUTO_SYNC_MODELS_INTERVAL_SECS", "60");
        std::env::remove_var("AUTO_SYNC_MODELS_INTERVAL_SECS");
        assert_eq!(get_auto_sync_models_interval(), Duration::from_secs(60));
        std::env::remove_var("LLMLB_AUTO_SYNC_MODELS_INTERVAL_SECS");
    }
}
