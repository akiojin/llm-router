use super::*;

#[tokio::test]
async fn init_db_pool_creates_sqlite_file_when_missing() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("load balancer.db");
    let db_url = format!("sqlite:{}", db_path.display());

    assert!(
        !db_path.exists(),
        "database file should not exist before initialization"
    );

    let pool = init_db_pool(&db_url)
        .await
        .expect("init_db_pool should create missing sqlite file");

    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .expect("basic query should succeed after initialization");

    assert!(
        db_path.exists(),
        "database file should be created by init_db_pool"
    );
}

// =======================================================================
// init_db_pool: in-memory SQLite
// =======================================================================
#[tokio::test]
async fn init_db_pool_in_memory() {
    let pool = init_db_pool("sqlite::memory:")
        .await
        .expect("in-memory sqlite should succeed");

    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .expect("basic query should succeed on in-memory db");
}

// =======================================================================
// init_db_pool: creates nested parent directories
// =======================================================================
#[tokio::test]
async fn init_db_pool_creates_nested_directories() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let nested_path = temp_dir
        .path()
        .join("a")
        .join("b")
        .join("c")
        .join("test.db");
    let db_url = format!("sqlite:{}", nested_path.display());

    assert!(
        !nested_path.exists(),
        "database file should not exist before init"
    );

    let pool = init_db_pool(&db_url)
        .await
        .expect("init_db_pool should create nested directories");

    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .expect("basic query should succeed");

    assert!(nested_path.exists());
}

// =======================================================================
// init_db_pool: path with spaces in directory name
// =======================================================================
#[tokio::test]
async fn init_db_pool_handles_spaces_in_path() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("dir with spaces").join("test db.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let pool = init_db_pool(&db_url)
        .await
        .expect("init_db_pool should handle spaces in path");

    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .expect("basic query should succeed");

    assert!(db_path.exists());
}

// =======================================================================
// init_db_pool: sqlite:// (double slash) prefix
// =======================================================================
#[tokio::test]
async fn init_db_pool_double_slash_prefix() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("double_slash.db");
    let db_url = format!("sqlite://{}", db_path.display());

    let pool = init_db_pool(&db_url)
        .await
        .expect("init_db_pool should handle sqlite:// prefix");

    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .expect("basic query should succeed");
}

// =======================================================================
// init_db_pool: existing file is reused
// =======================================================================
#[tokio::test]
async fn init_db_pool_reuses_existing_file() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("existing.db");
    let db_url = format!("sqlite:{}", db_path.display());

    // First pool creates the file
    let pool1 = init_db_pool(&db_url)
        .await
        .expect("first init should succeed");
    sqlx::query("CREATE TABLE test_reuse (id INTEGER PRIMARY KEY)")
        .execute(&pool1)
        .await
        .expect("table creation should succeed");
    drop(pool1);

    // Second pool should reuse the existing file and see the table
    let pool2 = init_db_pool(&db_url)
        .await
        .expect("second init should succeed");
    sqlx::query("INSERT INTO test_reuse (id) VALUES (1)")
        .execute(&pool2)
        .await
        .expect("insert into existing table should succeed");
}

// =======================================================================
// init_db_pool: URL with query parameters
// =======================================================================
#[tokio::test]
async fn init_db_pool_with_query_params() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("parameterized.db");
    let db_url = format!("sqlite:{}?mode=rwc", db_path.display());

    let pool = init_db_pool(&db_url)
        .await
        .expect("init_db_pool should handle query params");

    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .expect("basic query should succeed");
}

// =======================================================================
// InitContext struct layout
// =======================================================================
#[test]
fn init_context_has_expected_fields() {
    // Compile-time check: InitContext has state and _server_lock fields
    // This test verifies the struct definition by referencing the fields.
    fn _assert_fields(ctx: &InitContext) {
        let _state = &ctx.state;
        let _lock = &ctx._server_lock;
    }
}

// =======================================================================
// maybe_raise_nofile_limit: just ensure no panic
// =======================================================================
#[test]
fn maybe_raise_nofile_limit_does_not_panic() {
    // This function is no-op on non-Unix or if limits are already high
    maybe_raise_nofile_limit();
}

// =======================================================================
// init_db_pool: multiple concurrent pools on same db
// =======================================================================
#[tokio::test]
async fn init_db_pool_concurrent_pools() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("concurrent.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let pool1 = init_db_pool(&db_url)
        .await
        .expect("first pool should succeed");

    let pool2 = init_db_pool(&db_url)
        .await
        .expect("second pool should succeed");

    // Both pools should be able to execute queries
    sqlx::query("SELECT 1")
        .execute(&pool1)
        .await
        .expect("query on pool1 should succeed");
    sqlx::query("SELECT 1")
        .execute(&pool2)
        .await
        .expect("query on pool2 should succeed");
}

// =======================================================================
// init_db_pool: special characters in filename
// =======================================================================
#[tokio::test]
async fn init_db_pool_special_characters_in_filename() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("test-db_v2.1.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let pool = init_db_pool(&db_url)
        .await
        .expect("init_db_pool should handle special characters");

    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .expect("basic query should succeed");

    assert!(db_path.exists());
}

// =======================================================================
// init_db_pool: basic SQL operations
// =======================================================================
#[tokio::test]
async fn init_db_pool_supports_basic_operations() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("operations.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let pool = init_db_pool(&db_url)
        .await
        .expect("init_db_pool should succeed");

    // CREATE
    sqlx::query("CREATE TABLE ops_test (id INTEGER PRIMARY KEY, name TEXT NOT NULL)")
        .execute(&pool)
        .await
        .expect("CREATE TABLE should succeed");

    // INSERT
    sqlx::query("INSERT INTO ops_test (id, name) VALUES (1, 'test')")
        .execute(&pool)
        .await
        .expect("INSERT should succeed");

    // SELECT
    let row: (i64, String) = sqlx::query_as("SELECT id, name FROM ops_test WHERE id = 1")
        .fetch_one(&pool)
        .await
        .expect("SELECT should succeed");
    assert_eq!(row.0, 1);
    assert_eq!(row.1, "test");

    // UPDATE
    sqlx::query("UPDATE ops_test SET name = 'updated' WHERE id = 1")
        .execute(&pool)
        .await
        .expect("UPDATE should succeed");

    // DELETE
    sqlx::query("DELETE FROM ops_test WHERE id = 1")
        .execute(&pool)
        .await
        .expect("DELETE should succeed");
}

// =======================================================================
// init_db_pool: WAL mode
// =======================================================================
#[tokio::test]
async fn init_db_pool_default_journal_mode() {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let db_path = temp_dir.path().join("journal_test.db");
    let db_url = format!("sqlite:{}", db_path.display());

    let pool = init_db_pool(&db_url)
        .await
        .expect("init_db_pool should succeed");

    // Just verify the pool is functional
    let row: (String,) = sqlx::query_as("PRAGMA journal_mode")
        .fetch_one(&pool)
        .await
        .expect("PRAGMA should succeed");
    // journal_mode could be "wal", "delete", etc. depending on SQLite default
    assert!(!row.0.is_empty(), "journal mode should be non-empty");
}
