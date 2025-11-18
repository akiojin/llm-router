-- T039: 認証機能の初期スキーマ
-- SQLite用マイグレーション

-- ユーザーテーブル
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY NOT NULL,  -- UUID
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,  -- bcryptハッシュ
    role TEXT NOT NULL CHECK(role IN ('admin', 'viewer')),
    created_at TEXT NOT NULL,  -- ISO8601形式
    last_login TEXT  -- ISO8601形式、NULL可
);

-- ユーザー名インデックス（UNIQUE制約で自動作成されるが明示）
CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);

-- APIキーテーブル
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY NOT NULL,  -- UUID
    key_hash TEXT UNIQUE NOT NULL,  -- SHA-256ハッシュ
    name TEXT NOT NULL,
    created_by TEXT NOT NULL,  -- 発行したユーザーのUUID
    created_at TEXT NOT NULL,  -- ISO8601形式
    expires_at TEXT,  -- ISO8601形式、NULL可
    FOREIGN KEY (created_by) REFERENCES users(id) ON DELETE CASCADE
);

-- APIキーハッシュインデックス（高速検索用）
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);

-- APIキー発行者インデックス（ユーザー削除時のCASCADE用）
CREATE INDEX IF NOT EXISTS idx_api_keys_created_by ON api_keys(created_by);

-- エージェントトークンテーブル
CREATE TABLE IF NOT EXISTS agent_tokens (
    agent_id TEXT PRIMARY KEY NOT NULL,  -- エージェントUUID
    token_hash TEXT UNIQUE NOT NULL,  -- SHA-256ハッシュ
    created_at TEXT NOT NULL  -- ISO8601形式
);

-- エージェントトークンハッシュインデックス（認証時の高速検索用）
CREATE INDEX IF NOT EXISTS idx_agent_tokens_token_hash ON agent_tokens(token_hash);

-- 外部キー制約を有効化
PRAGMA foreign_keys = ON;
