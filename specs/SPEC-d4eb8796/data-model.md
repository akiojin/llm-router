# Phase 1: データモデル設計

**機能ID**: SPEC-d4eb8796
**日付**: 2025-11-17

## エンティティ定義

### 1. User (ユーザー)

**説明**: システムにログインして管理機能を利用するアカウント

**フィールド**:
| フィールド | 型 | 制約 | 説明 |
|-----------|-----|------|------|
| id | UUID | PRIMARY KEY | ユーザーID（自動生成） |
| username | String | UNIQUE, NOT NULL, 3-50文字 | ユーザー名（英数字とアンダースコア） |
| password_hash | String | NOT NULL | bcryptハッシュ化されたパスワード |
| role | UserRole | NOT NULL | 管理者（admin）または閲覧専用（viewer） |
| created_at | DateTime<Utc> | NOT NULL | アカウント作成日時 |
| last_login | DateTime<Utc> | NULL | 最終ログイン日時 |

**Rust型定義**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub password_hash: String,
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "TEXT")]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Admin,
    Viewer,
}
```

**検証ルール**:
- username: `^[a-zA-Z0-9_]{3,50}$`
- password（作成時のみ）: 最低8文字
- password_hash: bcryptハッシュ形式（`$2b$12$...`）
- role: `admin` または `viewer` のみ

**状態遷移**:
```
[新規] --作成--> [アクティブ]
[アクティブ] --パスワード変更--> [アクティブ]
[アクティブ] --最終ログイン更新--> [アクティブ]
[アクティブ] --削除--> [削除済み]
```

**ビジネスルール**:
- 最低1人の管理者ユーザーが常に存在する必要がある
- 最後の管理者は削除不可（UI で警告）
- ユーザー名の大文字小文字は区別される（`Admin` ≠ `admin`）

---

### 2. ApiKey (APIキー)

**説明**: 外部アプリケーションがAI機能を利用するための認証情報

**フィールド**:
| フィールド | 型 | 制約 | 説明 |
|-----------|-----|------|------|
| id | UUID | PRIMARY KEY | APIキーID（自動生成） |
| key_hash | String | UNIQUE, NOT NULL | SHA-256ハッシュ化されたキー本体 |
| name | String | NOT NULL, 1-100文字 | キーの説明（例: "my-chatbot"） |
| created_by | UUID | FOREIGN KEY → User.id, NOT NULL | 発行したユーザーID |
| created_at | DateTime<Utc> | NOT NULL | 発行日時 |
| expires_at | DateTime<Utc> | NULL | 有効期限（nullの場合は無期限） |

**Rust型定義**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiKey {
    pub id: Uuid,
    pub key_hash: String,
    pub name: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiKeyWithPlaintext {
    pub id: Uuid,
    pub key: String,  // 平文キー（発行時のみ）
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}
```

**検証ルール**:
- key（発行時）: `sk_` + 32文字のランダム英数字
- name: 1-100文字、任意の文字列
- key_hash: SHA-256ハッシュ（64文字の16進数）
- expires_at: 未来の日時、またはnull

**状態遷移**:
```
[発行リクエスト] --生成--> [アクティブ（有効期限あり/なし）]
[アクティブ] --有効期限到達--> [期限切れ]
[アクティブ] --削除--> [削除済み]
[期限切れ] --削除--> [削除済み]
```

**ビジネスルール**:
- キー本体（平文）は発行時の1回のみ表示
- 発行後はハッシュのみDB保存（漏洩時の安全性）
- 期限切れキーは認証拒否
- ユーザー削除時、そのユーザーが発行したキーもすべて削除（CASCADE）

---

### 3. AgentToken (エージェントトークン)

**説明**: エージェントがコーディネーターと安全に通信するための認証情報

**フィールド**:
| フィールド | 型 | 制約 | 説明 |
|-----------|-----|------|------|
| agent_id | UUID | PRIMARY KEY, FOREIGN KEY → Agent.id | エージェントID |
| token_hash | String | UNIQUE, NOT NULL | SHA-256ハッシュ化されたトークン本体 |
| created_at | DateTime<Utc> | NOT NULL | 発行日時 |

**Rust型定義**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AgentToken {
    pub agent_id: Uuid,
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentTokenWithPlaintext {
    pub agent_id: Uuid,
    pub token: String,  // 平文トークン（発行時のみ）
    pub created_at: DateTime<Utc>,
}
```

**検証ルール**:
- token（発行時）: `agt_` + UUID v4 simple形式（32文字の16進数）
- token_hash: SHA-256ハッシュ（64文字の16進数）

**状態遷移**:
```
[エージェント登録] --トークン発行--> [アクティブ]
[アクティブ] --エージェント削除--> [削除済み]
```

**ビジネスルール**:
- 1エージェントにつき1トークン（1:1関係）
- エージェント削除時、トークンも自動削除（CASCADE）
- トークン再発行は不可（エージェント再登録が必要）

---

### 4. Agent (既存エンティティ、変更)

**説明**: 登録されたエージェント（既存のエンティティに認証トークンのリレーションを追加）

**追加されるリレーションシップ**:
- `Agent.id` ← `AgentToken.agent_id` (1:1)

**変更なし**: 既存のフィールド（machine_name, ip_address, gpu_devices等）は維持

---

## エンティティ関連図（ER図）

```
User (1) --< (N) ApiKey
  - 1人のユーザーは複数のAPIキーを発行可能

Agent (1) --- (1) AgentToken
  - 1エージェントにつき1トークン
```

---

## SQLiteスキーマ

**マイグレーションファイル**: `coordinator/migrations/001_auth_init.sql`

```sql
-- ユーザーテーブル
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('admin', 'viewer')),
    created_at TEXT NOT NULL,
    last_login TEXT
);

CREATE INDEX idx_users_username ON users(username);

-- APIキーテーブル
CREATE TABLE IF NOT EXISTS api_keys (
    id TEXT PRIMARY KEY,
    key_hash TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    created_by TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT,
    FOREIGN KEY(created_by) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX idx_api_keys_created_by ON api_keys(created_by);

-- エージェントトークンテーブル
CREATE TABLE IF NOT EXISTS agent_tokens (
    agent_id TEXT PRIMARY KEY,
    token_hash TEXT UNIQUE NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY(agent_id) REFERENCES agents(id) ON DELETE CASCADE
);

CREATE INDEX idx_agent_tokens_hash ON agent_tokens(token_hash);

-- エージェントテーブル（既存）
-- 既存のagentsテーブルはそのまま維持
-- マイグレーションで外部キー制約のみ追加
```

**データ型の選択**:
- UUIDは `TEXT` として保存（SQLiteにUUID型がないため）
- DateTimeは `TEXT` (ISO 8601形式: `2025-11-17T10:30:00Z`)
- Enumは `TEXT` + `CHECK` 制約

---

## データ整合性ルール

1. **外部キー制約**:
   - `api_keys.created_by` → `users.id` (CASCADE DELETE)
   - `agent_tokens.agent_id` → `agents.id` (CASCADE DELETE)

2. **ユニーク制約**:
   - `users.username`
   - `api_keys.key_hash`
   - `agent_tokens.token_hash`

3. **NOT NULL制約**:
   - すべてのPRIMARY KEY
   - `users.password_hash`, `users.role`
   - `api_keys.key_hash`, `api_keys.name`, `api_keys.created_by`
   - `agent_tokens.token_hash`

4. **CHECK制約**:
   - `users.role` は `admin` または `viewer`

---

## セキュリティ考慮事項

1. **パスワード**: bcrypt（cost=12）で不可逆ハッシュ化
2. **APIキー**: SHA-256で不可逆ハッシュ化
3. **エージェントトークン**: SHA-256で不可逆ハッシュ化
4. **平文の保存禁止**: すべての認証情報はハッシュ化してDB保存
5. **インデックス**: ハッシュ値にインデックスを作成（検索高速化）

---

## JSONからSQLiteへのマイグレーション

**既存データ**:
- `~/.ollama-coordinator/agents.json` → `agents` テーブル
- `~/.ollama-coordinator/request_history.json` → `request_history` テーブル（新規作成）

**マイグレーション手順**:
1. 既存のJSONファイルをパース
2. SQLiteトランザクション開始
3. データを各テーブルに挿入
4. トランザクションコミット
5. JSONファイルを `.migrated` にリネーム（バックアップ）

**失敗時のロールバック**: トランザクション失敗時はロールバック、JSONファイルは保持

---

次のステップ: API契約設計 (`contracts/`)
