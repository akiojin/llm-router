# 実装計画: コーディネーター認証・アクセス制御

**機能ID**: `SPEC-d4eb8796` | **日付**: 2025-11-17 | **仕様**: [spec.md](./spec.md)
**入力**: `/specs/SPEC-d4eb8796/spec.md`の機能仕様

## 実行フロー (/speckit.plan コマンドのスコープ)

```
1. 入力パスから機能仕様を読み込み ✓
2. 技術コンテキストを記入 (進行中)
3. 憲章チェックセクションを評価 (次)
4. Phase 0 を実行 → research.md
5. Phase 1 を実行 → contracts, data-model.md, quickstart.md
6. 憲章チェックセクションを再評価
7. Phase 2 を計画 → タスク生成アプローチを記述
8. 停止 - /speckit.tasks コマンドの準備完了
```

## 概要

コーディネーターに認証・アクセス制御機能を追加し、管理APIとAI機能APIを保護します。
主要な要件：

- 初回起動時の管理者アカウント作成（対話式または環境変数）
- JWT認証による管理画面ログイン
- APIキー発行・管理（外部アプリケーション向け）
- 3種類の認証方式：JWT（管理API）、APIキー（OpenAI互換API）、エージェントトークン（エージェント通信）
- 認証無効化モード（プライベートネットワーク用）
- SQLiteへのデータ移行（既存のJSONファイルベースから）

技術アプローチ：
- Axum + tower-httpの認証ミドルウェア
- bcryptによるパスワードハッシュ化
- JWTトークン管理（jsonwebtokenクレート）
- SQLite（sqlxクレート、非同期対応）
- 既存のJSON→SQLite自動マイグレーション

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+ (edition 2021)
**主要依存関係**:
- Webフレームワーク: axum 0.7, tower 0.4, tower-http 0.5
- 認証: bcrypt 0.15, jsonwebtoken 9.2
- データベース: sqlx 0.7 (sqlite, runtime-tokio)
- 非同期: tokio 1.35 (full)
- シリアライゼーション: serde 1.0, serde_json 1.0

**ストレージ**: SQLite (既存のJSONファイルベースから移行)
- データベースファイル: `~/.ollama-coordinator/coordinator.db`
- 自動マイグレーション機能（初回起動時にJSONからインポート）

**テスト**: cargo test
- Contract tests: API契約テスト
- Integration tests: 認証フロー統合テスト
- Unit tests: パスワードハッシュ、JWT生成等

**対象プラットフォーム**: Windows, macOS, Linux（クロスプラットフォーム対応必須）

**プロジェクトタイプ**: single (Rustワークスペース: common, coordinator, agent)

**パフォーマンス目標**:
- ログインレスポンス: < 500ms
- API認証オーバーヘッド: < 10ms
- JWT検証: < 5ms

**制約**:
- 既存のエージェント登録APIの後方互換性を維持
- 認証無効化モード対応（環境変数で切り替え）
- JSONファイルからSQLiteへの自動マイグレーション（データ損失なし）

**スケール/スコープ**:
- 想定ユーザー数: 1〜10人（小規模チーム運用）
- APIキー: 10〜50個
- エージェント: 5〜20台

## 憲章チェック

*ゲート: Phase 0 research前に合格必須。Phase 1 design後に再チェック。*

**シンプルさ**:
- プロジェクト数: 3 (coordinator, agent, common) ✓
- フレームワークを直接使用? **はい** (axum/sqlxを直接使用、ラッパーなし) ✓
- 単一データモデル? **はい** (DTO不使用、serde経由でシリアライゼーション) ✓
- パターン回避? **はい** (Repository/UoWパターン不使用、直接的なDB操作) ✓

**アーキテクチャ**:
- すべての機能をライブラリとして? **はい** (coordinator/src/lib.rs経由で公開)
- ライブラリリスト:
  - `coordinator` - サーバー本体（認証、API、ルーティング）
  - `agent` - エージェント実行ファイル
  - `common` - 共有型定義・エラー定義
- ライブラリごとのCLI:
  - coordinator: `--help`, `--version`, `--port`, `--host`
  - agent: `--help`, `--version`, `--coordinator-url`
- ライブラリドキュメント: README.md + CLAUDE.md (llms.txt形式を検討) ✓

**テスト (妥協不可)**:
- RED-GREEN-Refactorサイクルを強制? **はい** (TDD必須) ✓
- Gitコミットはテストが実装より先に表示? **はい** ✓
- 順序: Contract→Integration→E2E→Unitを厳密に遵守? **はい** ✓
- 実依存関係を使用? **はい** (実SQLiteを使用、モックなし) ✓
- Integration testの対象:
  - 認証API全エンドポイント
  - 認証ミドルウェア
  - SQLiteマイグレーション
- 禁止: テスト前の実装、REDフェーズのスキップ ✓

**可観測性**:
- 構造化ロギング含む? **はい** (tracing/tracing-subscriber使用) ✓
- フロントエンドログ → バックエンド? **はい** (ブラウザコンソールエラーをログ送信API経由) ✓
- エラーコンテキスト十分? **はい** (認証エラー、DB エラー、詳細なメッセージ) ✓

**バージョニング**:
- バージョン番号割り当て済み? **はい** (semantic-release自動管理) ✓
- 変更ごとにBUILDインクリメント? **はい** (feat: = MINOR, fix: = PATCH) ✓
- 破壊的変更を処理? **はい** (エージェントトークン追加は後方互換、認証はオプトイン) ✓

## プロジェクト構造

### ドキュメント (この機能)

```
specs/SPEC-d4eb8796/
├── plan.md              # このファイル
├── research.md          # Phase 0 出力
├── data-model.md        # Phase 1 出力
├── quickstart.md        # Phase 1 出力
├── contracts/           # Phase 1 出力
│   ├── auth-api.yaml   # 認証API (OpenAPI)
│   ├── users-api.yaml  # ユーザー管理API
│   └── api-keys-api.yaml # APIキー管理API
└── tasks.md             # Phase 2 出力 (/speckit.tasks)
```

### ソースコード (リポジトリルート)

```
coordinator/
├── src/
│   ├── auth/           # 新規: 認証モジュール
│   │   ├── mod.rs
│   │   ├── password.rs  # パスワードハッシュ化
│   │   ├── jwt.rs       # JWT生成・検証
│   │   ├── middleware.rs # 認証ミドルウェア
│   │   └── bootstrap.rs  # 初回起動時の管理者作成
│   ├── db/             # 既存: データベースモジュール
│   │   ├── mod.rs      # 変更: SQLiteに移行
│   │   ├── migrations.rs # 新規: マイグレーション
│   │   ├── users.rs    # 新規: ユーザーDB操作
│   │   ├── api_keys.rs # 新規: APIキーDB操作
│   │   └── agent_tokens.rs # 新規: エージェントトークンDB操作
│   ├── api/            # 既存: APIルーティング
│   │   ├── mod.rs      # 変更: 認証ミドルウェア追加
│   │   ├── auth.rs     # 新規: 認証エンドポイント
│   │   ├── users.rs    # 新規: ユーザー管理エンドポイント
│   │   ├── api_keys.rs # 新規: APIキー管理エンドポイント
│   │   └── agent.rs    # 変更: エージェントトークン発行
│   └── web/            # 既存: ダッシュボード
│       └── static/
│           ├── login.html  # 新規: ログイン画面
│           ├── api-keys.js # 新規: APIキー管理画面
│           └── users.js    # 新規: ユーザー管理画面
└── tests/
    ├── contract/       # 新規: 契約テスト
    │   ├── auth_api_test.rs
    │   ├── users_api_test.rs
    │   └── api_keys_api_test.rs
    ├── integration/    # 新規: 統合テスト
    │   ├── auth_flow_test.rs
    │   ├── migration_test.rs
    │   └── middleware_test.rs
    └── unit/           # 新規: ユニットテスト
        ├── password_test.rs
        └── jwt_test.rs

agent/
└── src/
    └── main.rs         # 変更: トークン保存・送信機能

common/
└── src/
    ├── auth.rs         # 新規: 認証関連型定義
    └── error.rs        # 変更: 認証エラー追加
```

**構造決定**: 単一プロジェクト（Rustワークスペース）、Webアプリケーションだが
frontendは静的ファイル（バニラJS）のため分離不要

## Phase 0: アウトライン＆リサーチ

### リサーチ対象

1. **SQLiteマイグレーション戦略**:
   - sqlxのマイグレーション機能（`sqlx::migrate!`）
   - JSONファイル → SQLite自動インポート手法
   - データ損失防止（トランザクション、バックアップ）

2. **bcryptベストプラクティス**:
   - コスト設定（デフォルト12、調整可能性）
   - クロスプラットフォーム互換性
   - パフォーマンス特性

3. **JWT実装パターン**:
   - トークン有効期限（デフォルト24時間）
   - リフレッシュトークン戦略（将来的な拡張）
   - シークレット管理（環境変数または自動生成）

4. **Axum認証ミドルウェアパターン**:
   - `tower::middleware::from_fn_with_state`の使用
   - エラーハンドリング（401 Unauthorized）
   - ルーター階層でのミドルウェア適用

5. **エージェントトークン生成**:
   - セキュアなランダム生成（`uuid::Uuid::new_v4`）
   - ハッシュ化（SHA-256）
   - 衝突回避

**出力**: すべての技術選択が確定したresearch.md

## Phase 1: 設計＆契約

*前提条件: research.md完了*

### 1. データモデル設計 (`data-model.md`)

**エンティティ:**

- **User**: ユーザーアカウント
  - id: UUID (PRIMARY KEY)
  - username: String (UNIQUE, NOT NULL)
  - password_hash: String (NOT NULL)
  - role: UserRole (admin | viewer)
  - created_at: DateTime<Utc>
  - last_login: Option<DateTime<Utc>>

- **ApiKey**: 外部アプリケーション向けAPIキー
  - id: UUID (PRIMARY KEY)
  - key_hash: String (UNIQUE, NOT NULL)
  - name: String (NOT NULL)
  - created_by: UUID (FOREIGN KEY → User.id)
  - created_at: DateTime<Utc>
  - expires_at: Option<DateTime<Utc>>

- **AgentToken**: エージェント通信用トークン
  - agent_id: UUID (PRIMARY KEY, FOREIGN KEY → Agent.id)
  - token_hash: String (UNIQUE, NOT NULL)
  - created_at: DateTime<Utc>

- **Agent** (既存、変更):
  - トークンリレーションシップ追加

**検証ルール:**
- username: 3〜50文字、英数字とアンダースコアのみ
- password: 最低8文字（初回起動時のみチェック）
- api_key: `sk_` プレフィックス + 32文字ランダム文字列

**状態遷移:**
- User: 作成 → アクティブ → (パスワード変更) → アクティブ → 削除
- ApiKey: 発行 → アクティブ → 削除
- AgentToken: 発行 → アクティブ (削除はエージェント削除時)

### 2. API契約設計 (`/contracts/`)

**認証API** (`auth-api.yaml`):
- `POST /api/auth/login` - ログイン
- `POST /api/auth/logout` - ログアウト
- `GET /api/auth/me` - 現在のユーザー情報

**ユーザー管理API** (`users-api.yaml`):
- `GET /api/users` - ユーザー一覧（Admin専用）
- `POST /api/users` - ユーザー作成（Admin専用）
- `PUT /api/users/:id` - ユーザー更新（Admin専用）
- `DELETE /api/users/:id` - ユーザー削除（Admin専用）

**APIキー管理API** (`api-keys-api.yaml`):
- `GET /api/api-keys` - APIキー一覧（Admin専用）
- `POST /api/api-keys` - APIキー発行（Admin専用）
- `DELETE /api/api-keys/:id` - APIキー削除（Admin専用）

**エージェント登録API** (既存、変更):
- `POST /api/agents` - レスポンスに `agent_token` フィールド追加

### 3. 契約テスト生成

**契約テストファイル** (`tests/contract/`):
- `auth_api_test.rs` - 認証APIのスキーマ検証（失敗するテスト）
- `users_api_test.rs` - ユーザー管理APIのスキーマ検証（失敗するテスト）
- `api_keys_api_test.rs` - APIキー管理APIのスキーマ検証（失敗するテスト）

テストは実装前なので **RED** (失敗) が期待される。

### 4. テストシナリオ抽出

**ユーザーストーリー1** → `integration/auth_flow_test.rs`:
- 初回起動時の管理者作成（環境変数）
- ログイン成功
- ログイン失敗（間違ったパスワード）
- 未認証でのダッシュボードアクセス拒否

**ユーザーストーリー2** → `integration/api_key_flow_test.rs`:
- APIキー発行
- 発行されたキーで認証成功
- 無効なキーで認証失敗
- APIキー削除後の認証失敗

**ユーザーストーリー3** → `integration/middleware_test.rs`:
- 未認証での管理API拒否
- JWT認証での管理API許可
- APIキー認証での管理API拒否（JWT専用）

**ユーザーストーリー4** → `integration/auth_disabled_test.rs`:
- 認証無効化モードでの全API許可
- 認証有効化モードでの認証要求

**ユーザーストーリー5** → `integration/agent_token_test.rs`:
- エージェント登録時のトークン発行
- トークン付きヘルスチェック成功
- トークンなしヘルスチェック拒否

### 5. クイックスタートガイド (`quickstart.md`)

```markdown
# クイックスタート: 認証機能

## 初回起動

### 環境変数で管理者作成
export ADMIN_USERNAME=admin
export ADMIN_PASSWORD=secure123
cargo run --bin coordinator

### 対話式で管理者作成
cargo run --bin coordinator
> ユーザー名: admin
> パスワード: ********

## ログイン

1. ブラウザで http://localhost:8080/dashboard にアクセス
2. ログイン画面でユーザー名・パスワード入力
3. ダッシュボードが表示される

## APIキー発行

1. ダッシュボードの「APIキー」タブを開く
2. 「新規発行」ボタンをクリック
3. キー名を入力（例: "my-chatbot"）
4. 発行されたキー（`sk_xxxxx`）をコピー（一度しか表示されない）

## 外部アプリケーションからのアクセス

curl -H "Authorization: Bearer sk_xxxxx" \
     -H "Content-Type: application/json" \
     -d '{"model":"gpt-oss:7b","messages":[{"role":"user","content":"Hello"}]}' \
     http://localhost:8080/v1/chat/completions

## 認証無効化（プライベートネットワーク用）

export AUTH_DISABLED=true
cargo run --bin coordinator
```

**出力**: data-model.md, contracts/*.yaml, 失敗する契約テスト, quickstart.md

## Phase 2: タスク計画アプローチ

*このセクションは/speckit.tasksコマンドが実行することを記述*

### タスク生成戦略

**Phase 1設計ドキュメントからタスクを生成:**

1. **Setup タスク** (並列実行可能 [P]):
   - [ ] SQLiteスキーマファイル作成 `coordinator/migrations/001_init.sql` [P]
   - [ ] Cargo.toml に依存関係追加（bcrypt, jsonwebtoken, sqlx） [P]
   - [ ] 環境変数設定（AUTH_DISABLED, JWT_SECRET等） [P]

2. **Contract Test タスク** (TDD: RED):
   - [ ] 認証API契約テスト作成（`tests/contract/auth_api_test.rs`） [P]
   - [ ] ユーザー管理API契約テスト作成（`tests/contract/users_api_test.rs`） [P]
   - [ ] APIキー管理API契約テスト作成（`tests/contract/api_keys_api_test.rs`） [P]
   - [ ] 契約テスト実行 → **RED** 確認

3. **Data Model タスク** (並列実行可能 [P]):
   - [ ] User構造体実装（`common/src/auth.rs`） [P]
   - [ ] ApiKey構造体実装（`common/src/auth.rs`） [P]
   - [ ] AgentToken構造体実装（`common/src/auth.rs`） [P]
   - [ ] エラー型追加（`common/src/error.rs`）

4. **Database Migration タスク** (TDD: Integration Test):
   - [ ] マイグレーションテスト作成（`tests/integration/migration_test.rs`） → **RED**
   - [ ] SQLiteマイグレーション実装（`coordinator/src/db/migrations.rs`） → **GREEN**
   - [ ] JSONインポート機能実装（既存データ移行）→ **GREEN**

5. **Authentication Core タスク** (TDD: Unit Test):
   - [ ] パスワードハッシュ化テスト作成（`tests/unit/password_test.rs`） → **RED**
   - [ ] パスワードハッシュ化実装（`coordinator/src/auth/password.rs`） → **GREEN**
   - [ ] JWT生成・検証テスト作成（`tests/unit/jwt_test.rs`） → **RED**
   - [ ] JWT生成・検証実装（`coordinator/src/auth/jwt.rs`） → **GREEN**

6. **Middleware タスク** (TDD: Integration Test):
   - [ ] JWT認証ミドルウェアテスト作成 → **RED**
   - [ ] JWT認証ミドルウェア実装（`coordinator/src/auth/middleware.rs`） → **GREEN**
   - [ ] APIキー認証ミドルウェアテスト作成 → **RED**
   - [ ] APIキー認証ミドルウェア実装 → **GREEN**
   - [ ] エージェントトークン認証ミドルウェアテスト作成 → **RED**
   - [ ] エージェントトークン認証ミドルウェア実装 → **GREEN**

7. **API Implementation タスク** (TDD: Contract Test → GREEN):
   - [ ] 認証APIエンドポイント実装（`coordinator/src/api/auth.rs`） → 契約テスト **GREEN**
   - [ ] ユーザー管理APIエンドポイント実装（`coordinator/src/api/users.rs`） → 契約テスト **GREEN**
   - [ ] APIキー管理APIエンドポイント実装（`coordinator/src/api/api_keys.rs`） → 契約テスト **GREEN**
   - [ ] エージェント登録API修正（agent_token追加） → 既存テスト **GREEN**

8. **Database Operations タスク**:
   - [ ] ユーザーDB操作実装（`coordinator/src/db/users.rs`）
   - [ ] APIキーDB操作実装（`coordinator/src/db/api_keys.rs`）
   - [ ] エージェントトークンDB操作実装（`coordinator/src/db/agent_tokens.rs`）

9. **Bootstrap タスク** (TDD: Integration Test):
   - [ ] 初回起動時管理者作成テスト → **RED**
   - [ ] 初回起動時管理者作成実装（`coordinator/src/auth/bootstrap.rs`） → **GREEN**

10. **Router Integration タスク**:
    - [ ] 認証ミドルウェアをルーターに適用（`coordinator/src/api/mod.rs`）
    - [ ] 認証無効化モード実装（環境変数チェック）

11. **Frontend タスク** (並列実行可能 [P]):
    - [ ] ログイン画面実装（`coordinator/src/web/static/login.html`） [P]
    - [ ] APIキー管理画面実装（`coordinator/src/web/static/api-keys.js`） [P]
    - [ ] ユーザー管理画面実装（`coordinator/src/web/static/users.js`） [P]
    - [ ] 認証状態管理実装（localStorage、JWT送信）

12. **Agent Integration タスク**:
    - [ ] エージェント側トークン保存実装（`agent/src/main.rs`）
    - [ ] エージェント側トークン送信実装（ヘルスチェック、メトリクス）

13. **E2E Test タスク**:
    - [ ] 認証フロー E2E テスト（ログイン→API呼び出し→ログアウト）
    - [ ] APIキーフロー E2E テスト（発行→使用→削除）
    - [ ] エージェントフロー E2E テスト（登録→トークン使用）

14. **Documentation タスク**:
    - [ ] README.md更新（認証機能追加）
    - [ ] 環境変数一覧更新
    - [ ] APIドキュメント更新

### 順序戦略

**TDD順序**: Contract Test → Integration Test → Unit Test → Implementation
**依存関係順序**:
1. Setup (並列)
2. Contract Tests (並列、すべてRED)
3. Data Model (並列)
4. Database Migration
5. Authentication Core (Unit Tests)
6. Middleware (Integration Tests)
7. Database Operations
8. API Implementation (Contract Tests → GREEN)
9. Bootstrap
10. Router Integration
11. Frontend (並列)
12. Agent Integration
13. E2E Tests
14. Documentation

**並列実行のために[P]をマーク**: Setup, Contract Tests, Data Model, Frontend

**推定出力**: tasks.mdに約40〜50個の番号付き、順序付きタスク

**重要**: このフェーズは/speckit.tasksコマンドで実行、/speckit.planではない

## Phase 3+: 今後の実装

*これらのフェーズは/planコマンドのスコープ外*

**Phase 3**: タスク実行 (/speckit.tasksコマンドがtasks.mdを作成)
**Phase 4**: 実装 (TDD原則に従ってtasks.mdを実行)
**Phase 5**: 検証
- ローカル検証: `make quality-checks` (cargo fmt, clippy, test, commitlint, markdownlint)
- 手動検証: quickstart.mdの手順を実行
- パフォーマンス検証: ログインレスポンス < 500ms、認証オーバーヘッド < 10ms

## 複雑さトラッキング

*憲章チェックに正当化が必要な違反がある場合のみ記入*

| 違反 | 必要な理由 | より単純な代替案が却下された理由 |
|------|-----------|--------------------------------|
| なし | - | - |

## 進捗トラッキング

*このチェックリストは実行フロー中に更新される*

**フェーズステータス**:
- [x] Phase 0: Research完了 (/speckit.plan コマンド)
- [x] Phase 1: Design完了 (/speckit.plan コマンド)
- [x] Phase 2: Task planning完了 (/speckit.plan コマンド - アプローチのみ記述)
- [x] Phase 3: Tasks生成済み (/speckit.tasks コマンド) - 102タスク
- [ ] Phase 4: 実装完了
- [ ] Phase 5: 検証合格

**ゲートステータス**:
- [x] 初期憲章チェック: 合格
- [x] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱を文書化済み (なし)

---
*憲章準拠 - `memory/constitution.md` 参照*
