# 共通ログシステム 実装計画

## 現状分析

### Router (Rust)

- **ファイル**: `router/src/logging.rs`
- **出力先**: `~/.llm-router/logs/coordinator.log.jsonl`（ファイルのみ）
- **形式**: JSON Lines（tracing-subscriber fmt::layer().json()）
- **ローテーション**: なし（append）
- **環境変数**: `ROUTER_LOG_LEVEL`, `RUST_LOG`, `LLM_ROUTER_DATA_DIR`

### Node (C++)

- **ファイル**: `node/src/utils/logger.cpp`, `node/include/utils/logger.h`
- **出力先**: stdout + オプションでファイル
- **形式**: テキスト or JSON（`LOG_FORMAT=json`）
- **ローテーション**: サイズベース（`LOG_MAX_SIZE_MB`, `LOG_MAX_FILES`）
- **環境変数**: `LOG_LEVEL`, `LOG_FILE`, `LOG_FORMAT`, `LOG_MAX_SIZE_MB`, `LOG_MAX_FILES`

## 技術設計

### ログディレクトリ構造

```text
~/.llm-router/
└── logs/
    ├── llm-router.jsonl.2025-11-28
    ├── llm-router.jsonl.2025-11-27
    ├── llm-node.jsonl.2025-11-28
    └── llm-node.jsonl.2025-11-27
```

### 環境変数（統一）

| 変数名 | 説明 | デフォルト値 |
|--------|------|-------------|
| `LLM_LOG_DIR` | ログディレクトリ | `~/.llm-router/logs` |
| `LLM_LOG_LEVEL` | ログレベル | `info` |
| `LLM_LOG_RETENTION_DAYS` | 保持日数 | `7` |

既存の環境変数（`ROUTER_LOG_LEVEL`, `LOG_LEVEL`等）も後方互換として維持。

### ログエントリ形式（共通）

```json
{"ts":"2025-11-28T12:00:00.000Z","level":"info","category":"api","msg":"Request received"}
```

## Router側修正

### 修正ファイル

- `router/src/logging.rs`

### 変更内容

1. ログディレクトリを `~/.llm-router/logs/` に変更
2. ファイル名を `llm-router.jsonl.YYYY-MM-DD` に変更
3. 日付ベースローテーション実装（tracing-appender::rolling::daily）
4. 起動時に7日超の古いファイル削除
5. `target`フィールドを`category`として出力するカスタムフォーマッタ
6. 新環境変数のサポート

### 依存関係追加

```toml
[dependencies]
chrono = "0.4"
```

## Node側修正

### 修正ファイル

- `node/src/utils/logger.cpp`
- `node/include/utils/logger.h`

### 変更内容

1. デフォルト出力先を `~/.llm-router/logs/llm-node.jsonl.YYYY-MM-DD` に変更
2. stdout出力を削除
3. 日付ベースローテーション実装（spdlog::sinks::daily_file_sink）
4. 起動時に7日超の古いファイル削除
5. カテゴリフィールド追加
6. 新環境変数のサポート

### 依存関係

spdlogの既存機能で対応可能（daily_file_sink_mtを使用）。

## 共通ログ読み取り

### 修正ファイル

- `common/src/log.rs`

### 変更内容

1. ログディレクトリパスを新形式に対応
2. 複数日付ファイルの読み取りサポート

## 実装順序（TDD）

### Phase 1: Router側

1. テスト作成（ローテーション、フォーマット、古いファイル削除）
2. `logging.rs`修正
3. テスト実行確認

### Phase 2: Node側

1. テスト作成（ローテーション、フォーマット、古いファイル削除）
2. `logger.cpp`/`logger.h`修正
3. テスト実行確認

### Phase 3: 統合確認

1. Router/Node両方を起動してログ出力確認
2. 既存の`/api/logs`エンドポイント動作確認
