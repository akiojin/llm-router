# タスク: コーディネーター認証・アクセス制御

**入力**: `/specs/SPEC-d4eb8796/`の設計ドキュメント
**前提条件**: plan.md, research.md, data-model.md, contracts/, quickstart.md

## 実行フロー

```
✓ 1. plan.mdから技術スタック抽出完了
✓ 2. 設計ドキュメント読み込み完了
✓ 3. カテゴリ別タスク生成完了
✓ 4. TDD順序適用完了
✓ 5. 並列実行マーク完了
✓ 6. タスク検証完了
→ 7. 実装開始準備完了
```

## フォーマット: `[ID] [P?] 説明`

- **[P]**: 並列実行可能（異なるファイル、依存関係なし）
- 説明には正確なファイルパスを含める

## Phase 3.1: セットアップ

- [x] **T001** [P] Cargo.tomlに認証関連依存関係を追加
（bcrypt 0.15, jsonwebtoken 9.2, sqlx 0.7 with sqlite/runtime-tokio）
- [x] **T002** [P] SQLiteマイグレーションディレクトリ作成
`coordinator/migrations/` ディレクトリ構造を準備
- [x] **T003** [P] 環境変数設定ドキュメント作成 `.env.example` ファイルで
AUTH_DISABLED, JWT_SECRET, ADMIN_USERNAME, ADMIN_PASSWORD を定義

## Phase 3.2: テストファースト (TDD) ⚠️ 3.3の前に完了必須

**重要: これらのテストは記述され、実装前に失敗する必要がある（RED）**

### Contract Tests (並列実行可能)

- [x] **T004** [P] `coordinator/tests/contract/auth_api_test.rs` に
POST /api/auth/login の契約テスト（スキーマ検証、REDを確認）
- [x] **T005** [P] `coordinator/tests/contract/auth_api_test.rs` に
POST /api/auth/logout の契約テスト（スキーマ検証、REDを確認）
- [x] **T006** [P] `coordinator/tests/contract/auth_api_test.rs` に
GET /api/auth/me の契約テスト（スキーマ検証、REDを確認）
- [x] **T007** [P] `coordinator/tests/contract/users_api_test.rs` に
GET /api/users の契約テスト（スキーマ検証、REDを確認）
- [x] **T008** [P] `coordinator/tests/contract/users_api_test.rs` に
POST /api/users の契約テスト（スキーマ検証、REDを確認）
- [x] **T009** [P] `coordinator/tests/contract/users_api_test.rs` に
PUT /api/users/:id の契約テスト（スキーマ検証、REDを確認）
- [x] **T010** [P] `coordinator/tests/contract/users_api_test.rs` に
DELETE /api/users/:id の契約テスト（スキーマ検証、REDを確認）
- [x] **T011** [P] `coordinator/tests/contract/api_keys_api_test.rs` に
GET /api/api-keys の契約テスト（スキーマ検証、REDを確認）
- [x] **T012** [P] `coordinator/tests/contract/api_keys_api_test.rs` に
POST /api/api-keys の契約テスト（スキーマ検証、REDを確認）
- [x] **T013** [P] `coordinator/tests/contract/api_keys_api_test.rs` に
DELETE /api/api-keys/:id の契約テスト（スキーマ検証、REDを確認）

### Integration Tests (並列実行可能)

- [x] **T014** [P] `coordinator/tests/integration/migration_test.rs` に
JSONからSQLiteへのマイグレーションテスト（REDを確認）
- [x] **T015** [P] `coordinator/tests/integration/auth_flow_test.rs` に
ログイン成功フローのテスト（REDを確認）
- [x] **T016** [P] `coordinator/tests/integration/auth_flow_test.rs` に
ログイン失敗フロー（間違ったパスワード）のテスト（REDを確認）
- [x] **T017** [P] `coordinator/tests/integration/auth_flow_test.rs` に
未認証でのダッシュボードアクセス拒否テスト（REDを確認）
- [x] **T018** [P] `coordinator/tests/integration/api_key_flow_test.rs` に
APIキー発行フローのテスト（REDを確認）
- [x] **T019** [P] `coordinator/tests/integration/api_key_flow_test.rs` に
APIキー認証成功フローのテスト（REDを確認）
- [x] **T020** [P] `coordinator/tests/integration/api_key_flow_test.rs` に
無効なAPIキーでの認証失敗テスト（REDを確認）
- [x] **T021** [P] `coordinator/tests/integration/middleware_test.rs` に
未認証での管理API拒否テスト（REDを確認）
- [x] **T022** [P] `coordinator/tests/integration/middleware_test.rs` に
JWT認証での管理API許可テスト（REDを確認）
- [x] **T023** [P] `coordinator/tests/integration/auth_disabled_test.rs` に
認証無効化モードでのアクセス許可テスト（REDを確認）
- [x] **T024** [P] `coordinator/tests/integration/agent_token_test.rs` に
エージェント登録時のトークン発行テスト（REDを確認）
- [x] **T025** [P] `coordinator/tests/integration/agent_token_test.rs` に
トークン付きヘルスチェック成功テスト（REDを確認）
- [x] **T026** [P] `coordinator/tests/integration/agent_token_test.rs` に
トークンなしヘルスチェック拒否テスト（REDを確認）

### Unit Tests (並列実行可能)

- [x] **T027** [P] `coordinator/tests/unit/password_test.rs` に
パスワードハッシュ化のユニットテスト（REDを確認）
- [x] **T028** [P] `coordinator/tests/unit/password_test.rs` に
パスワード検証のユニットテスト（REDを確認）
- [x] **T029** [P] `coordinator/tests/unit/jwt_test.rs` に
JWT生成のユニットテスト（REDを確認）
- [x] **T030** [P] `coordinator/tests/unit/jwt_test.rs` に
JWT検証のユニットテスト（REDを確認）
- [x] **T031** [P] `coordinator/tests/unit/jwt_test.rs` に
JWT有効期限チェックのユニットテスト（REDを確認）

**テスト実行: すべてのテストがREDであることを確認**
```bash
cargo test
# すべてのテストが失敗することを確認（実装がないため）
```

## Phase 3.3: データモデル実装 (テストが失敗した後のみ)

- [x] **T032** [P] `common/src/auth.rs` に User 構造体を実装
（id, username, password_hash, role, created_at, last_login）
- [x] **T033** [P] `common/src/auth.rs` に UserRole enum を実装
（Admin, Viewer）
- [x] **T034** [P] `common/src/auth.rs` に ApiKey 構造体を実装
（id, key_hash, name, created_by, created_at, expires_at）
- [x] **T035** [P] `common/src/auth.rs` に ApiKeyWithPlaintext 構造体を実装
（発行時のレスポンス用）
- [x] **T036** [P] `common/src/auth.rs` に AgentToken 構造体を実装
（agent_id, token_hash, created_at）
- [x] **T037** [P] `common/src/auth.rs` に AgentTokenWithPlaintext 構造体を実装
（発行時のレスポンス用）
- [x] **T038** `common/src/error.rs` に認証関連エラーを追加
（AuthError, PasswordHashError, JwtError, ApiKeyError, AgentTokenError）

## Phase 3.4: データベースマイグレーション

- [x] **T039** `coordinator/migrations/001_auth_init.sql` に SQLiteスキーマを作成
（users, api_keys, agent_tokens テーブル、インデックス、外部キー制約）
- [x] **T040** `coordinator/src/db/migrations.rs` に
マイグレーション実行関数を実装（sqlx::migrate!使用） → T014 GREEN
- [x] **T041** `coordinator/src/db/migrations.rs` に
JSONインポート機能を実装（agents.json → SQLite） → T014 GREEN

## Phase 3.5: 認証コア実装

- [x] **T042** `coordinator/src/auth/password.rs` に
パスワードハッシュ化関数を実装（bcrypt, cost=12） → T027 GREEN
- [x] **T043** `coordinator/src/auth/password.rs` に
パスワード検証関数を実装（bcrypt verify） → T028 GREEN
- [x] **T044** `coordinator/src/auth/jwt.rs` に
JWT生成関数を実装（jsonwebtoken, HS256, 24時間有効期限） → T029 GREEN
- [x] **T045** `coordinator/src/auth/jwt.rs` に
JWT検証関数を実装（jsonwebtoken decode） → T030, T031 GREEN
- [x] **T046** `coordinator/src/auth/jwt.rs` に
JWTシークレット管理を実装（環境変数または自動生成）

## Phase 3.6: ミドルウェア実装

- [x] **T047** `coordinator/src/auth/middleware.rs` に
JWT認証ミドルウェアを実装（tower::middleware::from_fn_with_state使用）
→ T021, T022 GREEN
- [x] **T048** `coordinator/src/auth/middleware.rs` に
APIキー認証ミドルウェアを実装（SHA-256検証） → T019, T020 GREEN
- [x] **T049** `coordinator/src/auth/middleware.rs` に
エージェントトークン認証ミドルウェアを実装（SHA-256検証） → T025, T026 GREEN

## Phase 3.7: データベース操作実装

- [x] **T050** `coordinator/src/db/users.rs` に
ユーザーCRUD操作を実装（create, find_by_username, update, delete）
- [x] **T051** `coordinator/src/db/users.rs` に
初回起動チェック関数を実装（ユーザーが0人かどうか）
- [x] **T052** `coordinator/src/db/users.rs` に
最後の管理者チェック関数を実装（削除前の検証用）
- [x] **T053** `coordinator/src/db/api_keys.rs` に
APIキーCRUD操作を実装（create, list, find_by_hash, delete）
- [x] **T054** `coordinator/src/db/api_keys.rs` に
APIキー生成関数を実装（`sk_` + 32文字ランダム、SHA-256ハッシュ）
- [x] **T055** `coordinator/src/db/agent_tokens.rs` に
エージェントトークンCRUD操作を実装（create, find_by_hash, delete）
- [x] **T056** `coordinator/src/db/agent_tokens.rs` に
エージェントトークン生成関数を実装（`agt_` + UUID, SHA-256ハッシュ）

## Phase 3.8: API実装

- [x] **T057** `coordinator/src/api/auth.rs` に
POST /api/auth/login エンドポイントを実装 → T004 GREEN
- [x] **T058** `coordinator/src/api/auth.rs` に
POST /api/auth/logout エンドポイントを実装 → T005 GREEN
- [x] **T059** `coordinator/src/api/auth.rs` に
GET /api/auth/me エンドポイントを実装 → T006 GREEN
- [x] **T060** `coordinator/src/api/users.rs` に
GET /api/users エンドポイントを実装（Admin専用） → T007 GREEN
- [x] **T061** `coordinator/src/api/users.rs` に
POST /api/users エンドポイントを実装（Admin専用） → T008 GREEN
- [x] **T062** `coordinator/src/api/users.rs` に
PUT /api/users/:id エンドポイントを実装（Admin専用） → T009 GREEN
- [x] **T063** `coordinator/src/api/users.rs` に
DELETE /api/users/:id エンドポイントを実装（Admin専用、最後の管理者チェック）
→ T010 GREEN
- [x] **T064** `coordinator/src/api/api_keys.rs` に
GET /api/api-keys エンドポイントを実装（Admin専用） → T011 GREEN
- [x] **T065** `coordinator/src/api/api_keys.rs` に
POST /api/api-keys エンドポイントを実装（Admin専用、平文キー返却） → T012 GREEN
- [x] **T066** `coordinator/src/api/api_keys.rs` に
DELETE /api/api-keys/:id エンドポイントを実装（Admin専用） → T013 GREEN
- [x] **T067** `coordinator/src/api/agent.rs` を修正して
POST /api/agents レスポンスに agent_token フィールドを追加 → T024 GREEN

## Phase 3.9: 初回起動処理

- [x] **T068** `coordinator/src/auth/bootstrap.rs` に
初回起動時の管理者作成関数を実装（環境変数チェック）
- [x] **T069** `coordinator/src/auth/bootstrap.rs` に
対話式管理者作成関数を実装（標準入力でユーザー名・パスワード取得）
- [x] **T070** `coordinator/src/main.rs` に
起動時の管理者作成処理を統合（環境変数優先、なければ対話式）

## Phase 3.10: ルーター統合

- [x] **T071** `coordinator/src/api/mod.rs` に
JWT認証ミドルウェアを管理APIに適用
（/api/agents, /api/models, /api/dashboard, /api/users, /api/api-keys）
→ T015, T016, T017 GREEN
- [x] **T072** `coordinator/src/api/mod.rs` に
APIキー認証ミドルウェアをOpenAI互換APIに適用
（/v1/chat/completions, /v1/completions, /v1/embeddings, /v1/models）
- [x] **T073** `coordinator/src/api/mod.rs` に
エージェントトークン認証ミドルウェアをエージェント通信APIに適用
（/api/health, /api/agents/:id/metrics, /api/tasks/:id/progress）
- [x] **T074** `coordinator/src/api/mod.rs` に
認証無効化モードを実装（AUTH_DISABLED=true で全ミドルウェアスキップ）
→ T023 GREEN

## Phase 3.11: フロントエンド実装 (並列実行可能)

- [x] **T075** [P] `coordinator/src/web/static/login.html` に
ログイン画面を作成（ユーザー名・パスワード入力フォーム）
- [x] **T076** [P] `coordinator/src/web/static/login.js` に
ログイン処理を実装（POST /api/auth/login, JWTをlocalStorageに保存）
- [x] **T077** [P] `coordinator/src/web/static/app.js` に
認証状態管理を追加（localStorage JWT確認、全APIリクエストにBearer付与）
- [x] **T078** [P] `coordinator/src/web/static/app.js` に
401エラーハンドリングを追加（自動的にログイン画面へリダイレクト）
- [x] **T079** [P] `coordinator/src/web/static/api-keys.html` に
APIキー管理画面を作成（タブ追加）
- [x] **T080** [P] `coordinator/src/web/static/api-keys.js` に
APIキー一覧表示を実装（GET /api/api-keys）
- [x] **T081** `coordinator/src/web/static/api-keys.js` に
APIキー発行機能を実装（POST /api/api-keys、平文キーのモーダル表示）
- [x] **T082** `coordinator/src/web/static/api-keys.js` に
APIキー削除機能を実装（DELETE /api/api-keys/:id）
- [x] **T083** [P] `coordinator/src/web/static/users.html` に
ユーザー管理画面を作成（タブ追加、Admin専用）
- [x] **T084** [P] `coordinator/src/web/static/users.js` に
ユーザー一覧表示を実装（GET /api/users）
- [x] **T085** `coordinator/src/web/static/users.js` に
ユーザー作成機能を実装（POST /api/users）
- [x] **T086** `coordinator/src/web/static/users.js` に
パスワード変更機能を実装（PUT /api/users/:id）
- [x] **T087** `coordinator/src/web/static/users.js` に
ユーザー削除機能を実装（DELETE /api/users/:id、最後の管理者警告）

## Phase 3.12: エージェント統合

- [x] **T088** `agent/src/main.rs` に
エージェント登録レスポンスからトークン抽出を実装
- [x] **T089** `agent/src/main.rs` に
トークン保存機能を実装（`~/.llm-node/token` ファイルに保存）
- [x] **T090** `agent/src/main.rs` に
全HTTPリクエストにX-Agent-Tokenヘッダー付与を実装

## Phase 3.13: E2Eテスト

- [x] **T091** [P] `coordinator/tests/e2e/auth_flow_test.rs` に
完全な認証フロー E2E テスト（ログイン → API呼び出し → ログアウト）
- [x] **T092** [P] `coordinator/tests/e2e/api_key_flow_test.rs` に
完全なAPIキーフロー E2E テスト（発行 → 使用 → 削除）
- [x] **T093** [P] `coordinator/tests/e2e/agent_flow_test.rs` に
完全なエージェントフロー E2E テスト（登録 → トークン使用 → ヘルスチェック）

## Phase 3.14: ドキュメント更新

- [x] **T094** [P] `README.md` を更新して認証機能を説明
（初回起動、ログイン、APIキー発行の手順）
- [x] **T095** [P] `README.md` に環境変数一覧を追加
（AUTH_DISABLED, JWT_SECRET, ADMIN_USERNAME, ADMIN_PASSWORD）
- [x] **T096** [P] `docs/api.md` を作成してAPI仕様を文書化
（OpenAPI仕様からMarkdown生成、または手動作成）

## Phase 3.15: ローカル検証

- [x] **T097** ローカル品質チェック実行: `cargo fmt --check`
- [x] **T098** ローカル品質チェック実行: `cargo clippy -- -D warnings`
- [x] **T099** ローカル品質チェック実行: `cargo test`
- [x] **T100** ローカル品質チェック実行: `pnpm dlx markdownlint-cli2 "**/*.md"`
- [x] **T101** ローカル品質チェック実行: `.specify/scripts/checks/check-commits.sh`
- [ ] **T102** 手動検証: `specs/SPEC-d4eb8796/quickstart.md` の全手順を実行

## 依存関係

**TDD順序（厳格）**:
- Tests (T004-T031) → Implementation (T032-T093) より**先**
- T004-T031 はすべて **RED** である必要がある

**実装依存関係**:
- T032-T038 (データモデル) が T039-T041 (マイグレーション) をブロック
- T039-T041 (マイグレーション) が T050-T056 (DB操作) をブロック
- T042-T046 (認証コア) が T047-T049 (ミドルウェア) をブロック
- T050-T056 (DB操作) が T057-T067 (API実装) をブロック
- T047-T049 (ミドルウェア) が T071-T074 (ルーター統合) をブロック
- T057-T067 (API実装) が T075-T087 (フロントエンド) をブロック
- T071-T074 (ルーター統合) が T091-T093 (E2E テスト) をブロック

## 並列実行例

### Setup (T001-T003)

すべて並列実行可能、異なるファイル

### Contract Tests (T004-T013)

すべて並列実行可能、異なるテストファイル
```bash
# 3つのテストファイルを並列実行
cargo test contract::auth_api_test &
cargo test contract::users_api_test &
cargo test contract::api_keys_api_test &
wait
```

### Integration Tests (T014-T026)

すべて並列実行可能、異なるテストファイル
```bash
cargo test integration::migration_test &
cargo test integration::auth_flow_test &
cargo test integration::api_key_flow_test &
cargo test integration::middleware_test &
cargo test integration::auth_disabled_test &
cargo test integration::agent_token_test &
wait
```

### Data Model (T032-T037)

すべて並列実行可能、異なる構造体定義

### Frontend (T075-T087)

異なるHTMLファイルは並列実行可能（T075, T079, T083）、
同じJSファイル内のタスクは順次実行

## 注意事項

- **[P] タスク** = 異なるファイル、依存関係なし
- **実装前にテストが失敗することを確認**（REDフェーズ必須）
- **各タスク後にコミット**（Conventional Commits準拠）
- **回避**: 曖昧なタスク、同じファイルの競合

## 検証チェックリスト

- [x] すべてのcontracts（auth-api, users-api, api-keys-api）に対応するテストがある
- [x] すべてのentities（User, ApiKey, AgentToken）にmodelタスクがある
- [x] すべてのテストが実装より先にある（T004-T031 → T032以降）
- [x] 並列タスクは本当に独立している（異なるファイル）
- [x] 各タスクは正確なファイルパスを指定
- [x] 同じファイルを変更する[P]タスクがない

---

**総タスク数**: 102
**推定工数**: 40〜60時間（TDD厳守、品質重視）
**次のステップ**: `/speckit.implement` またはタスクを手動で実行
