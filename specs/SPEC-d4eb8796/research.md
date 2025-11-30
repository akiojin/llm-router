# Phase 0: 技術リサーチ

**機能ID**: SPEC-d4eb8796
**日付**: 2025-11-17

## 1. SQLiteマイグレーション戦略

### 決定: sqlx + 埋め込みマイグレーション

**選択理由**:
- sqlxは型安全な非同期SQLiteサポート
- `sqlx::migrate!`マクロでコンパイル時にマイグレーション埋め込み
- トランザクションサポートによる安全なマイグレーション
- クロスプラットフォーム（Windows/macOS/Linux）完全対応

**検討した代替案**:
- **rusqlite**: 同期のみ、tokio統合が煩雑
- **diesel**: 重量級、非同期サポートが限定的
- **手動SQL実行**: エラー処理が複雑、型安全性なし

**実装方針**:
```rust
// coordinator/migrations/001_init.sql
CREATE TABLE IF NOT EXISTS users (...);
CREATE TABLE IF NOT EXISTS api_keys (...);
CREATE TABLE IF NOT EXISTS agent_tokens (...);

// coordinator/src/db/mod.rs
pub async fn init_database() -> Result<SqlitePool> {
    let pool = SqlitePool::connect(&db_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
```

**JSONインポート戦略**:
1. 起動時に `~/.llm-router/agents.json` の存在確認
2. 存在する場合、SQLiteにデータが未移行かチェック
3. トランザクション内でJSONをパース→SQLiteに挿入
4. 成功後、`agents.json.migrated` にリネーム（バックアップ）

## 2. bcryptベストプラクティス

### 決定: bcrypt 0.15 with cost=12

**選択理由**:
- 業界標準のパスワードハッシュアルゴリズム
- 計算コストを調整可能（将来的な攻撃に対応）
- クロスプラットフォーム対応
- パフォーマンス: cost=12で約200-300ms（許容範囲）

**検討した代替案**:
- **argon2**: より新しいが、llm-routerの規模ではオーバースペック
- **scrypt**: 標準化が不十分
- **SHA-256**: ハッシュアルゴリズムであり、パスワードハッシュ専用ではない（不適切）

**実装方針**:
```rust
use bcrypt::{hash, verify, DEFAULT_COST};

pub fn hash_password(password: &str) -> Result<String> {
    hash(password, DEFAULT_COST) // cost=12
        .map_err(|e| CoordinatorError::PasswordHash(e.to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    verify(password, hash)
        .map_err(|e| CoordinatorError::PasswordVerify(e.to_string()))
}
```

**セキュリティ考慮事項**:
- パスワードは平文で保存しない（bcryptハッシュのみ）
- ハッシュ化はバックグラウンドスレッドで実行（非同期ブロック回避）
- ユーザー名は大文字小文字を区別（セキュリティ向上）

## 3. JWT実装パターン

### 決定: jsonwebtoken 9.2 with HS256

**選択理由**:
- Rustの標準的なJWTライブラリ
- HMAC-SHA256（HS256）でシンプルかつセキュア
- トークン有効期限設定が容易
- エンコード・デコード時の検証が自動

**検討した代替案**:
- **frank_jwt**: 開発が停滞
- **手動実装**: セキュリティリスクが高い
- **RS256（公開鍵暗号）**: 単一サーバーではオーバースペック

**実装方針**:
```rust
use jsonwebtoken::{encode, decode, Header, Validation, EncodingKey, DecodingKey};

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,      // ユーザーID
    role: UserRole,   // 管理者/閲覧専用
    exp: usize,       // 有効期限（Unix timestamp）
}

pub fn create_jwt(user_id: &str, role: UserRole) -> Result<String> {
    let expiration = Utc::now()
        .checked_add_signed(chrono::Duration::hours(24))
        .unwrap()
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        role,
        exp: expiration,
    };

    encode(&Header::default(), &claims, &EncodingKey::from_secret(SECRET))
        .map_err(|e| CoordinatorError::JwtCreation(e.to_string()))
}

pub fn verify_jwt(token: &str) -> Result<Claims> {
    decode::<Claims>(token, &DecodingKey::from_secret(SECRET), &Validation::default())
        .map(|data| data.claims)
        .map_err(|e| CoordinatorError::JwtValidation(e.to_string()))
}
```

**トークン設定**:
- **有効期限**: 24時間（デフォルト）
- **シークレット**: 環境変数 `JWT_SECRET`、未設定の場合は初回起動時にランダム生成
- **リフレッシュトークン**: スコープ外（将来的な拡張）

**セキュリティ考慮事項**:
- シークレットは最低32文字のランダム文字列
- トークンはHTTPSで送信（推奨、リバースプロキシで実現）
- HTTPOnlyクッキーは不使用（SPAのため、Authorizationヘッダー使用）

## 4. Axum認証ミドルウェアパターン

### 決定: tower::middleware::from_fn_with_state

**選択理由**:
- Axumの標準的なミドルウェア実装パターン
- AppState共有が容易（JWTシークレット等）
- 型安全なエラーハンドリング
- ルーター階層で柔軟に適用可能

**検討した代替案**:
- **tower::layer**: より低レベル、複雑
- **axum::extract::Extension**: 非推奨パターン
- **グローバルミドルウェア**: 柔軟性がない

**実装方針**:
```rust
use axum::{
    middleware::{self, Next},
    http::{Request, StatusCode},
    response::Response,
    extract::State,
};

async fn jwt_auth_middleware<B>(
    State(state): State<AppState>,
    mut req: Request<B>,
    next: Next<B>,
) -> Result<Response, StatusCode> {
    // Authorization ヘッダーからトークン抽出
    let token = req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // JWT検証
    let claims = verify_jwt(token, &state.jwt_secret)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // リクエストにユーザー情報を追加
    req.extensions_mut().insert(claims);

    Ok(next.run(req).await)
}

// ルーターへの適用
Router::new()
    .route("/api/agents", get(list_agents))
    .layer(middleware::from_fn_with_state(state.clone(), jwt_auth_middleware))
```

**ミドルウェア階層**:
1. **JWT認証**: `/api/agents`, `/api/models`, `/api/dashboard`, `/api/users`, `/api/api-keys`
2. **APIキー認証**: `/v1/chat/completions`, `/v1/completions`, `/v1/embeddings`, `/v1/models`
3. **エージェントトークン認証**: `/api/health`, `/api/agents/:id/metrics`, `/api/tasks/:id/progress`

**認証無効化モード**:
```rust
if env::var("AUTH_DISABLED").unwrap_or_default() == "true" {
    router // ミドルウェアなし
} else {
    router.layer(middleware::from_fn_with_state(state.clone(), jwt_auth_middleware))
}
```

## 5. エージェントトークン生成

### 決定: UUID v4 + SHA-256ハッシュ

**選択理由**:
- UUIDv4はセキュアなランダム生成（衝突確率極小）
- SHA-256で不可逆ハッシュ化（漏洩時の安全性）
- パフォーマンス: 生成・検証が高速
- 標準ライブラリ使用（追加依存なし）

**検討した代替案**:
- **JWT**: エージェント通信には過剰（有効期限不要）
- **HMAC**: 対称鍵管理が煩雑
- **bcrypt**: 遅すぎる（毎リクエストで検証）

**実装方針**:
```rust
use uuid::Uuid;
use sha2::{Sha256, Digest};

pub fn generate_agent_token() -> (String, String) {
    let token = format!("agt_{}", Uuid::new_v4().simple());
    let token_hash = hash_token(&token);
    (token, token_hash)
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn verify_agent_token(token: &str, stored_hash: &str) -> bool {
    hash_token(token) == stored_hash
}
```

**トークン形式**: `agt_` + UUID v4 simple形式（32文字の16進数）
- 例: `agt_a1b2c3d4e5f67890a1b2c3d4e5f67890`
- プレフィックスでAPIキーと区別

**エージェント側の実装**:
- トークンを `~/.llm-node/token` に保存
- 全HTTPリクエストに `X-Agent-Token: agt_...` ヘッダーを追加

## まとめ

すべての技術選択が確定しました：

| 項目 | 選択技術 | 理由 |
|------|---------|------|
| データベース | sqlx + SQLite | 型安全、非同期、マイグレーション埋め込み |
| パスワードハッシュ | bcrypt (cost=12) | 業界標準、調整可能、クロスプラットフォーム |
| JWT | jsonwebtoken (HS256) | シンプル、セキュア、標準的 |
| ミドルウェア | tower::middleware | Axum標準パターン、柔軟 |
| エージェントトークン | UUID v4 + SHA-256 | セキュア、高速、シンプル |

次のPhase 1でデータモデルとAPI契約を設計します。
