//! Configuration management via environment variables
//!
//! Provides helper functions for reading environment variables with fallback
//! to deprecated variable names with warning logs.

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
/// use llm_router::config::get_env_with_fallback;
///
/// let port = get_env_with_fallback("LLM_ROUTER_PORT", "ROUTER_PORT");
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
        std::env::set_var("TEST_NEW_VAR6", "8080");
        std::env::remove_var("TEST_OLD_VAR6");

        let result: u16 = get_env_with_fallback_parse("TEST_NEW_VAR6", "TEST_OLD_VAR6", 3000);
        assert_eq!(result, 8080);

        std::env::remove_var("TEST_NEW_VAR6");
    }
}
