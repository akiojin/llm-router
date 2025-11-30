# 実装計画: CLI インターフェース整備

**機能ID**: `SPEC-a7e6d40a` | **日付**: 2025-11-30 | **仕様**: [spec.md](./spec.md)
**入力**: `/specs/SPEC-a7e6d40a/spec.md`の機能仕様

## 概要

LLM Routerに対してCLIインターフェースを追加する。

- 引数なしで実行した場合は従来通りルーターサービスを起動
- `--help`, `--version` オプションを追加
- `user` サブコマンドでユーザー管理（list/add/delete）
- 環境変数名を `LLM_ROUTER_*` プレフィックスに統一

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+
**主要依存関係**: clap（CLI引数パース）、sqlx（DB）、bcrypt（パスワードハッシュ）
**ストレージ**: SQLite (`~/.llm-router/router.db`)
**テスト**: cargo test
**対象プラットフォーム**: Linux, macOS, Windows
**プロジェクトタイプ**: single
**パフォーマンス目標**: CLI応答は即座（< 100ms）
**制約**: 設定はすべて環境変数経由、CLIオプションで設定値を渡さない

## 憲章チェック

**シンプルさ**:

- プロジェクト数: 1 (router)
- フレームワークを直接使用: ✅ clap/sqlxを直接使用
- 単一データモデル: ✅ 既存のUserモデルを使用
- パターン回避: ✅ 不要な抽象化なし

**アーキテクチャ**:

- すべての機能をライブラリとして: ✅ `llm_router::cli` モジュールとして追加
- ライブラリリスト: llm-router (CLI + サーバー機能)
- CLI: `llm-router --help/--version` + `llm-router user --help`

**テスト (妥協不可)**:

- RED-GREEN-Refactorサイクルを強制: ✅
- Gitコミットはテストが実装より先に表示: ✅
- 順序: Contract→Integration→E2E→Unit: ✅
- 実依存関係を使用: ✅ 実SQLiteDB

**可観測性**:

- 構造化ロギング含む: ✅ 既存のtracingを活用
- エラーコンテキスト十分: ✅

**バージョニング**:

- バージョン番号: Cargo.tomlから取得 (`env!("CARGO_PKG_VERSION")`)
- semantic-release: ✅

## プロジェクト構造

### ドキュメント (この機能)

```text
specs/SPEC-a7e6d40a/
├── spec.md              # 機能仕様
├── plan.md              # このファイル
└── tasks.md             # タスク一覧（/speckit.tasksで生成）
```

### ソースコード変更

```text
router/
├── Cargo.toml           # clap依存関係追加
└── src/
    ├── main.rs          # CLIエントリポイント修正
    ├── cli/             # 新規: CLIモジュール
    │   ├── mod.rs       # CLI定義（clap）
    │   └── user.rs      # userサブコマンド実装
    ├── auth/
    │   └── bootstrap.rs # 環境変数名更新
    └── ...              # その他環境変数名更新
```

## Phase 0: リサーチ

### 現在の環境変数使用状況

| 現在の変数名 | 使用場所 | 新しい変数名 |
|-------------|---------|------------|
| `ROUTER_HOST` | main.rs:17 | `LLM_ROUTER_HOST` |
| `ROUTER_PORT` | main.rs:18 | `LLM_ROUTER_PORT` |
| `JWT_SECRET` | main.rs:168 | `LLM_ROUTER_JWT_SECRET` |
| `ADMIN_USERNAME` | auth/bootstrap.rs:35 | `LLM_ROUTER_ADMIN_USERNAME` |
| `ADMIN_PASSWORD` | auth/bootstrap.rs:26 | `LLM_ROUTER_ADMIN_PASSWORD` |
| `LLM_LOG_LEVEL` | logging.rs:25 | `LLM_ROUTER_LOG_LEVEL` |
| `ROUTER_LOG_LEVEL` | logging.rs:28 (レガシー) | 削除 |
| `DATABASE_URL` | main.rs:145 | `LLM_ROUTER_DATABASE_URL` |
| `HEALTH_CHECK_INTERVAL` | main.rs:115 | `LLM_ROUTER_HEALTH_CHECK_INTERVAL` |
| `NODE_TIMEOUT` | main.rs:119 | `LLM_ROUTER_NODE_TIMEOUT` |
| `LOAD_BALANCER_MODE` | main.rs:131 | `LLM_ROUTER_LOAD_BALANCER_MODE` |
| `LLM_ROUTER_DATA_DIR` | db/mod.rs:42 | そのまま |
| `OPENAI_API_KEY` | (要調査) | `LLM_ROUTER_OPENAI_API_KEY` |
| `ANTHROPIC_API_KEY` | (要調査) | `LLM_ROUTER_ANTHROPIC_API_KEY` |
| `GOOGLE_API_KEY` | (要調査) | `LLM_ROUTER_GOOGLE_API_KEY` |

### clap ライブラリ選定

**決定**: `clap` v4 (derive マクロ)

**理由**:

- Rust CLIの事実上の標準
- derive マクロで宣言的に定義可能
- サブコマンド対応
- 自動ヘルプ生成

**検討した代替案**:

- `argh`: シンプルだがサブコマンド対応が弱い
- `structopt`: clapに統合済み（非推奨）

## Phase 1: 設計

### CLI構造

```text
llm-router [OPTIONS] [COMMAND]

Commands:
  user    Manage users
  help    Print help information

Options:
  -h, --help     Print help information
  -V, --version  Print version information

Environment Variables:
  LLM_ROUTER_PORT              Server port (default: 8080)
  LLM_ROUTER_HOST              Server host (default: 0.0.0.0)
  LLM_ROUTER_LOG_LEVEL         Log level (default: info)
  LLM_ROUTER_JWT_SECRET        JWT signing secret
  LLM_ROUTER_ADMIN_USERNAME    Initial admin username (default: admin)
  LLM_ROUTER_ADMIN_PASSWORD    Initial admin password (required on first run)
```

```text
llm-router user <COMMAND>

Commands:
  list    List all users
  add     Add a new user
  delete  Delete a user
  help    Print help information
```

```text
llm-router user add <USERNAME> --password <PASSWORD>

Arguments:
  <USERNAME>  Username for the new user

Options:
  -p, --password <PASSWORD>  Password (min 8 characters)
```

### データモデル

既存の `User` モデルを使用（変更なし）:

```rust
struct User {
    id: Uuid,
    username: String,
    password_hash: String,
    role: String,  // "admin" | "user"
    created_at: DateTime<Utc>,
}
```

### 環境変数マイグレーション戦略

**方針**: 新旧両方をサポートし、旧変数使用時に警告を表示

```rust
fn get_env_with_fallback(new_name: &str, old_name: &str) -> Option<String> {
    if let Ok(val) = std::env::var(new_name) {
        return Some(val);
    }
    if let Ok(val) = std::env::var(old_name) {
        tracing::warn!(
            "{} is deprecated, use {} instead",
            old_name, new_name
        );
        return Some(val);
    }
    None
}
```

## Node（C++）CLI設計

### CLI構造

```text
llm-node [OPTIONS]

Options:
  -h, --help     Print help information
  -V, --version  Print version information

Environment Variables:
  LLM_ROUTER_URL              Router URL (default: http://127.0.0.1:8080)
  LLM_NODE_PORT               Node listen port (default: 11435)
  LLM_NODE_LOG_LEVEL          Log level (default: info)
  LLM_NODE_MODELS_DIR         Model storage directory
```

### 実装方針

- C++標準ライブラリのみ使用（外部CLI解析ライブラリ不要）
- `argc`/`argv` を手動でパース
- `--help`, `--version` のみ対応（サブコマンドなし）

### 変更対象ファイル

- `node/src/main.cpp` - CLI引数処理追加
- `node/src/utils/config.cpp` - 環境変数名更新
- `node/src/utils/logger.cpp` - 環境変数名更新
- `node/include/utils/version.h` - バージョン定数（新規）

### Node環境変数マイグレーション

| 現在の変数名 | 新しい変数名 |
|-------------|------------|
| `LLM_MODELS_DIR` | `LLM_NODE_MODELS_DIR` |
| `LLM_HEARTBEAT_SECS` | `LLM_NODE_HEARTBEAT_SECS` |
| `LLM_ALLOW_NO_GPU` | `LLM_NODE_ALLOW_NO_GPU` |
| `LLM_BIND_ADDRESS` | `LLM_NODE_BIND_ADDRESS` |
| `LLM_LOG_DIR` | `LLM_NODE_LOG_DIR` |
| `LLM_LOG_LEVEL` | `LLM_NODE_LOG_LEVEL` |
| `LLM_LOG_RETENTION_DAYS` | `LLM_NODE_LOG_RETENTION_DAYS` |

## JWT_SECRET ファイル永続化設計

### 現状の問題

- `JWT_SECRET` は環境変数で設定（設定されない場合はハードコードされたデフォルト値）
- 再起動のたびに異なるシークレットだと既存トークンが無効化される
- 管理者がシークレットを知る必要はない（内部用）

### 解決策

1. 初回起動時にUUIDv4でシークレットを自動生成
2. `~/.llm-router/jwt_secret` に保存（パーミッション600）
3. 以降の起動時はファイルから読み込み
4. 環境変数 `LLM_ROUTER_JWT_SECRET` で上書き可能（K8s等での運用向け）

### 優先順位

1. 環境変数 `LLM_ROUTER_JWT_SECRET` が設定されていれば使用
2. ファイル `~/.llm-router/jwt_secret` が存在すれば読み込み
3. どちらもなければ自動生成してファイルに保存

### ファイル形式

- プレーンテキスト（UUIDv4形式: `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`）
- 改行なし

### 変更対象ファイル

- `router/src/main.rs` - JWT_SECRET読み込みロジック変更

## Phase 2: タスク計画アプローチ

**タスク生成戦略**:

**Router (Rust)**:

1. clap依存関係追加 → Cargo.toml
2. CLIモジュール作成 → `router/src/cli/mod.rs`
3. 引数なし実行でサーバー起動のテスト（既存動作維持）
4. `--help` 表示テスト → 実装
5. `--version` 表示テスト → 実装
6. `user list` テスト → 実装
7. `user add` テスト → 実装
8. `user delete` テスト → 実装
9. 環境変数名統一（LLM_ROUTER_*）テスト → 実装
10. 旧環境変数のフォールバック＆警告テスト → 実装
11. JWT_SECRETファイル永続化テスト → 実装

**Node (C++)**:

1. `--help` 表示テスト → 実装
2. `--version` 表示テスト → 実装
3. 環境変数名統一（LLM_NODE_*）テスト → 実装
4. 旧環境変数のフォールバック＆警告テスト → 実装

**順序戦略**:

- TDD順序: テストが実装より先
- 依存関係順序: CLI基盤 → サブコマンド → 環境変数統一
- 並列実行: 独立したサブコマンドは [P] マーク
- Router / Node は独立して並列実行可能

**推定出力**: tasks.mdに30-35個のタスク

## 複雑さトラッキング

| 違反 | 必要な理由 | より単純な代替案が却下された理由 |
|------|-----------|--------------------------------|
| なし | - | - |

## 進捗トラッキング

**フェーズステータス**:

- [x] Phase 0: Research完了
- [x] Phase 1: Design完了
- [x] Phase 2: Task planning完了（アプローチのみ記述）
- [ ] Phase 3: Tasks生成済み (/speckit.tasks コマンド)
- [ ] Phase 4: 実装完了
- [ ] Phase 5: 検証合格

**ゲートステータス**:

- [x] 初期憲章チェック: 合格
- [x] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱を文書化済み

---

*憲章 v1.0.0 に基づく - `/memory/constitution.md` 参照*
