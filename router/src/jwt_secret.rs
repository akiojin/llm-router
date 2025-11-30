//! JWT Secret management
//!
//! Provides automatic generation and file-based persistence of JWT secrets.
//! The secret is stored in `~/.llm-router/jwt_secret` with permissions 600.

use crate::config::get_env_with_fallback;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Default JWT secret file name
const JWT_SECRET_FILE: &str = "jwt_secret";
/// Default data directory name
const DATA_DIR: &str = ".llm-router";

/// Get or create the JWT secret
///
/// Priority:
/// 1. Environment variable `LLM_ROUTER_JWT_SECRET` (or deprecated `JWT_SECRET`)
/// 2. Read from file `~/.llm-router/jwt_secret`
/// 3. Generate new UUIDv4 and save to file
///
/// # Returns
/// * `Ok(secret)` - The JWT secret string
/// * `Err(io::Error)` - Failed to read/write secret file
///
/// # Example
/// ```no_run
/// use llm_router::jwt_secret::get_or_create_jwt_secret;
///
/// let secret = get_or_create_jwt_secret().expect("Failed to get JWT secret");
/// ```
pub fn get_or_create_jwt_secret() -> io::Result<String> {
    // 1. Check environment variable first
    if let Some(secret) = get_env_with_fallback("LLM_ROUTER_JWT_SECRET", "JWT_SECRET") {
        if !secret.is_empty() {
            tracing::info!("Using JWT secret from environment variable");
            return Ok(secret);
        }
    }

    // 2. Try to read from file
    let secret_path = get_jwt_secret_path()?;
    if secret_path.exists() {
        let secret = read_secret_file(&secret_path)?;
        if !secret.is_empty() {
            tracing::info!("Using JWT secret from file: {}", secret_path.display());
            return Ok(secret);
        }
    }

    // 3. Generate new secret and save to file
    let secret = generate_secret();
    write_secret_file(&secret_path, &secret)?;
    tracing::info!(
        "Generated new JWT secret and saved to: {}",
        secret_path.display()
    );

    Ok(secret)
}

/// Get the path to the JWT secret file
fn get_jwt_secret_path() -> io::Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "Failed to resolve home directory"))?;

    Ok(PathBuf::from(home).join(DATA_DIR).join(JWT_SECRET_FILE))
}

/// Generate a new random secret using UUIDv4
fn generate_secret() -> String {
    Uuid::new_v4().to_string()
}

/// Read the secret from file
fn read_secret_file(path: &PathBuf) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut secret = String::new();
    file.read_to_string(&mut secret)?;
    Ok(secret.trim().to_string())
}

/// Write the secret to file with secure permissions (600)
fn write_secret_file(path: &PathBuf, secret: &str) -> io::Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write the secret
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    file.write_all(secret.as_bytes())?;

    // Set permissions to 600 (owner read/write only) on Unix
    #[cfg(unix)]
    {
        let metadata = file.metadata()?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(path, permissions)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    fn test_generate_secret_is_uuid_format() {
        let secret = generate_secret();
        // UUIDv4 format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
        assert_eq!(secret.len(), 36);
        assert!(Uuid::parse_str(&secret).is_ok());
    }

    #[test]
    fn test_write_and_read_secret_file() {
        let temp_dir = tempdir().unwrap();
        let secret_path = temp_dir.path().join("jwt_secret");
        let test_secret = "test-secret-12345";

        write_secret_file(&secret_path, test_secret).unwrap();
        let read_secret = read_secret_file(&secret_path).unwrap();

        assert_eq!(read_secret, test_secret);
    }

    #[cfg(unix)]
    #[test]
    fn test_secret_file_permissions() {
        let temp_dir = tempdir().unwrap();
        let secret_path = temp_dir.path().join("jwt_secret");
        let test_secret = "test-secret-12345";

        write_secret_file(&secret_path, test_secret).unwrap();

        let metadata = fs::metadata(&secret_path).unwrap();
        let permissions = metadata.permissions();
        assert_eq!(permissions.mode() & 0o777, 0o600);
    }

    #[test]
    #[serial]
    fn test_get_or_create_uses_env_var() {
        std::env::set_var("LLM_ROUTER_JWT_SECRET", "env-secret-test");
        std::env::remove_var("JWT_SECRET");

        let secret = get_or_create_jwt_secret().unwrap();
        assert_eq!(secret, "env-secret-test");

        std::env::remove_var("LLM_ROUTER_JWT_SECRET");
    }

    #[test]
    #[serial]
    fn test_get_or_create_uses_legacy_env_var() {
        std::env::remove_var("LLM_ROUTER_JWT_SECRET");
        std::env::set_var("JWT_SECRET", "legacy-secret-test");

        let secret = get_or_create_jwt_secret().unwrap();
        assert_eq!(secret, "legacy-secret-test");

        std::env::remove_var("JWT_SECRET");
    }
}
