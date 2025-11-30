# タスク: CLI インターフェース整備

**入力**: `/specs/SPEC-a7e6d40a/`の設計ドキュメント
**前提条件**: plan.md (必須)

## フォーマット: `[ID] [P?] 説明`

- **[P]**: 並列実行可能 (異なるファイル、依存関係なし)
- 説明には正確なファイルパスを含める

## Phase 3.1: セットアップ

- [x] T001 `router/Cargo.toml` に clap 依存関係を追加
  - `clap = { version = "4", features = ["derive", "env"] }`
- [x] T002 `router/src/cli/mod.rs` にCLIモジュールの基本構造を作成
  - Cliアーキタイプ構造体（clap derive）
  - Commands enum（None/User）
- [x] T003 `router/src/cli/user.rs` にuserサブコマンドモジュールを作成
  - UserCommand enum（List/Add/Delete）
- [x] T004 `router/src/lib.rs` に `pub mod cli;` を追加

## Phase 3.2: テストファースト (TDD)

**重要: これらのテストは記述され、実装前に失敗する必要がある**

- [x] T005-T010 `router/tests/cli_tests.rs` にCLIパーステストを統合
  - `--help` / `--version` 表示テスト
  - `user list` / `add` / `delete` コマンドパーステスト

## Phase 3.3: コア実装 (テストが失敗した後のみ)

- [x] T011 `router/src/main.rs` を修正してCLI引数をパース
  - clap でコマンドライン引数を処理
  - 引数なし → サーバー起動（既存動作維持）
  - `--help` → ヘルプ表示
  - `--version` → バージョン表示
  - `user` → サブコマンドへディスパッチ
- [x] T012 `router/src/cli/mod.rs` にCli構造体を実装
  - `#[command(version, about)]` 属性
  - 環境変数一覧をhelpに表示
- [x] T013 `router/src/cli/user.rs` に user list を実装
  - DBからユーザー一覧を取得
  - ユーザー名とロールを表示
- [x] T014 `router/src/cli/user.rs` に user add を実装
  - パスワードバリデーション（8文字以上）
  - bcryptでハッシュ化
  - DBに挿入
- [x] T015 `router/src/cli/user.rs` に user delete を実装
  - ユーザー存在確認
  - DBから削除

## Phase 3.4: 環境変数統一

- [x] T016 `router/src/config.rs` を新規作成（環境変数ヘルパー）
  - `get_env_with_fallback(new_name, old_name)` 関数
  - 旧変数使用時の警告ログ
- [x] T017 `router/src/main.rs` の環境変数を更新
  - `ROUTER_HOST` → `LLM_ROUTER_HOST`（フォールバック付き）
  - `ROUTER_PORT` → `LLM_ROUTER_PORT`（フォールバック付き）
  - `JWT_SECRET` → `LLM_ROUTER_JWT_SECRET`（フォールバック付き）
  - `DATABASE_URL` → `LLM_ROUTER_DATABASE_URL`（フォールバック付き）
  - `HEALTH_CHECK_INTERVAL` → `LLM_ROUTER_HEALTH_CHECK_INTERVAL`
  - `NODE_TIMEOUT` → `LLM_ROUTER_NODE_TIMEOUT`
  - `LOAD_BALANCER_MODE` → `LLM_ROUTER_LOAD_BALANCER_MODE`
- [x] T018 `router/src/auth/bootstrap.rs` の環境変数を更新
  - `ADMIN_USERNAME` → `LLM_ROUTER_ADMIN_USERNAME`（フォールバック付き）
  - `ADMIN_PASSWORD` → `LLM_ROUTER_ADMIN_PASSWORD`（フォールバック付き）
- [x] T019 `router/src/logging.rs` の環境変数を更新
  - `ROUTER_LOG_LEVEL` を削除（レガシー）
  - `LLM_LOG_LEVEL` → `LLM_ROUTER_LOG_LEVEL`（フォールバック付き）
- [x] T020 Cloud API環境変数 - スコープ外（既存のままで良い）

## Phase 3.5: JWT_SECRET ファイル永続化

- [x] T021-T023 `router/src/jwt_secret.rs` にJWT_SECRETファイル永続化実装
  - `get_or_create_jwt_secret()` 関数
  - UUIDv4生成
  - ファイル読み書き（パーミッション600）
  - テストも含めて実装済み

## Phase 3.6: Node（C++）CLI

- [x] T024 [P] `node/tests/unit/cli_test.cpp` に --help テスト
  - `llm-node --help` の出力に環境変数の説明が含まれることを確認
- [x] T025 [P] `node/tests/unit/cli_test.cpp` に --version テスト
  - `llm-node --version` の出力にバージョン番号が含まれることを確認
- [x] T026 `node/include/utils/version.h` を新規作成
  - `LLM_NODE_VERSION` 定数
- [x] T027 `node/src/main.cpp` にCLI引数処理を追加
  - `--help` / `-h` → ヘルプ表示して終了
  - `--version` / `-V` → バージョン表示して終了
  - 引数なし → 既存動作（ノード起動）

## Phase 3.7: Node 環境変数統一

- [x] T028 [P] `node/tests/unit/utils_config_test.cpp` に環境変数テスト追加
  - `LLM_NODE_MODELS_DIR` が使用されることを確認
  - 旧 `LLM_MODELS_DIR` 使用時に警告が出ることを確認
- [x] T029 `node/src/utils/config.cpp` の環境変数を更新
  - `LLM_MODELS_DIR` → `LLM_NODE_MODELS_DIR`（フォールバック付き）
  - `LLM_HEARTBEAT_SECS` → `LLM_NODE_HEARTBEAT_SECS`（フォールバック付き）
  - `LLM_ALLOW_NO_GPU` → `LLM_NODE_ALLOW_NO_GPU`（フォールバック付き）
  - `LLM_BIND_ADDRESS` → `LLM_NODE_BIND_ADDRESS`（フォールバック付き）
- [x] T030 `node/src/utils/logger.cpp` の環境変数を更新
  - `LLM_LOG_DIR` → `LLM_NODE_LOG_DIR`（フォールバック付き）
  - `LLM_LOG_LEVEL` → `LLM_NODE_LOG_LEVEL`（フォールバック付き）
  - `LLM_LOG_RETENTION_DAYS` → `LLM_NODE_LOG_RETENTION_DAYS`（フォールバック付き）

## Phase 3.8: 仕上げ

- [x] T031 [P] `docs/authentication.md` を更新
  - CLI user コマンドの使用例を追加
  - 環境変数 `LLM_ROUTER_ADMIN_PASSWORD` / `LLM_ROUTER_JWT_SECRET` を記載
- [x] T032 [P] `INSTALL.md` を更新
  - 新しい環境変数名を表形式で記載（Router: LLM_ROUTER_*, Node: LLM_NODE_*）
- [x] T033 [P] `README.md` を更新
  - 環境変数セクションを更新済み
- [x] T034 すべてのテストを実行して合格を確認
  - Router: `cargo test` (121 tests) / `cargo clippy -- -D warnings` / `cargo fmt --check` ✓
  - Node: 91 unit tests ✓
- [x] T035 手動テスト実行
  - `llm-node --help` → 環境変数一覧表示 ✓
  - `llm-node --version` → "llm-node 1.0.0" ✓

## 依存関係

```text
# Router (Rust)
T001 → T002 → T003 → T004
T004 → T005-T010 (並列可能)
T005-T010 → T011-T015
T011 → T012 → T013, T014, T015
T013, T014, T015 → T016
T016 → T017, T018, T019, T020
T017-T020 → T021-T023

# Node (C++)
T024-T025 (並列可能) → T026 → T027
T028 → T029, T030

# 仕上げ
T023, T030 → T031-T033 (並列可能)
T031-T034 → T035
```

## 並列実行例

```bash
# Phase 3.2 のテストを並列起動:
# T005-T010 を同時に実行可能（異なるファイル）

# Phase 3.6 のNode CLIテストを並列起動:
# T024-T025 を同時に実行可能（異なるファイル）

# Phase 3.8 のドキュメント更新を並列起動:
# T031-T033 を同時に実行可能（異なるファイル）

# Router と Node は独立して並列実行可能
```

## 検証チェックリスト

- [x] すべてのユーザーストーリーに対応するテストがある
- [x] すべてのテストが実装より先にある
- [x] 並列タスクは本当に独立している
- [x] 各タスクは正確なファイルパスを指定
- [x] 同じファイルを変更する[P]タスクがない
- [x] Node CLI要件が含まれている
- [x] JWT_SECRET永続化要件が含まれている
