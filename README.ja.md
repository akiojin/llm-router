# Ollama Coordinator

複数マシンでOllamaインスタンスを管理する中央集権型システム

[English](./README.md) | 日本語

## 概要

Ollama Coordinatorは、複数のマシン上で動作するOllamaインスタンスを一元管理し、統一されたAPIエンドポイントを提供するシステムです。ロードバランシング、自動障害検知、リアルタイム監視機能を備えています。

## 主な特徴

- **統一APIエンドポイント**: 複数のOllamaインスタンスを単一のURLで利用可能
- **自動ロードバランシング**: リクエストを利用可能なエージェントに自動分散
- **自動障害検知**: オフラインエージェントを自動検知して振り分けから除外
- **リアルタイム監視**: Webダッシュボードで全エージェントの状態を可視化
- **エージェント自己登録**: エージェントが自動的にCoordinatorに登録
- **WebUI管理**: ブラウザベースのダッシュボードでエージェント設定、監視、制御が可能
- **クロスプラットフォーム対応**: Windows 10+、macOS 12+、Linuxで動作

## アーキテクチャ

### システム構成

```
┌─────────────────────────────────────────────────────────────┐
│                         クライアント                          │
│              （ユーザー、アプリケーション等）                   │
└────────────────────┬────────────────────────────────────────┘
                     │ POST /api/chat
                     │ POST /api/generate
                     ▼
┌─────────────────────────────────────────────────────────────┐
│                      Coordinator                             │
│                   （中央管理サーバー）                         │
│                                                               │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ 1. エージェント選択（ロードバランシング）              │   │
│  │ 2. リクエストを選択されたエージェントへプロキシ転送   │   │
│  │ 3. レスポンスをクライアントへ返却                     │   │
│  └─────────────────────────────────────────────────────┘   │
└────┬──────────────────┬──────────────────┬─────────────────┘
     │                  │                  │
     │ 内部プロキシ      │ 内部プロキシ      │ 内部プロキシ
     ▼                  ▼                  ▼
┌─────────┐        ┌─────────┐        ┌─────────┐
│ Agent 1 │        │ Agent 2 │        │ Agent 3 │
│         │        │         │        │         │
│  Ollama │        │  Ollama │        │  Ollama │
│ (自動管理)│       │ (自動管理)│       │ (自動管理)│
└─────────┘        └─────────┘        └─────────┘
Machine 1          Machine 2          Machine 3
```

### 通信フロー（プロキシパターン）

Ollama Coordinatorは**プロキシパターン**を採用しており、クライアントはCoordinator URLだけを知っていればOKです。

#### 従来の方法（Coordinator なし）
```bash
# 各Ollamaに直接アクセス - ユーザーが手動で振り分け
curl http://machine1:11434/api/chat -d '...'
curl http://machine2:11434/api/chat -d '...'
curl http://machine3:11434/api/chat -d '...'
```

#### Coordinatorを使う方法（プロキシ）
```bash
# Coordinatorに統一アクセス - 自動的に最適なOllamaへ振り分け
curl http://coordinator:8080/api/chat -d '...'
curl http://coordinator:8080/api/chat -d '...'
curl http://coordinator:8080/api/chat -d '...'
```

**リクエストフロー詳細:**

1. **クライアント → Coordinator**
   ```
   POST http://coordinator:8080/api/chat
   Content-Type: application/json

   {"model": "llama2", "messages": [...]}
   ```

2. **Coordinator内部処理**
   - 最適なAgent/Ollamaを選択（ロードバランシング）
   - HTTPクライアントで選択されたAgentのOllamaにリクエスト転送

3. **Coordinator → Agent（内部通信）**
   ```
   POST http://agent1:11434/api/chat
   Content-Type: application/json

   {"model": "llama2", "messages": [...]}
   ```

4. **Agent → Ollama → Agent（ローカル処理）**
   - AgentがローカルOllamaインスタンスへリクエスト転送
   - Ollamaが LLM処理してレスポンス生成

5. **Coordinator → クライアント（レスポンス返却）**
   ```json
   {
     "message": {"role": "assistant", "content": "..."},
     "done": true
   }
   ```

**クライアントから見ると**:
- Coordinatorが唯一のOllama APIサーバーとして見える
- 内部の複数Ollamaインスタンスを意識する必要がない
- 1回のHTTPリクエストで完結

### プロキシ方式のメリット

1. **統一エンドポイント**
   - クライアントはCoordinator URLだけ知っていればOK
   - 各Agent/Ollamaの場所を知る必要なし

2. **透過的なロードバランシング**
   - Coordinatorが自動的に最適なエージェントを選択
   - クライアントは何も意識せずに負荷分散の恩恵を受ける

3. **障害時の自動リトライ**
   - Agent1が失敗 → Coordinatorが自動的にAgent2を試す
   - クライアントは再リクエスト不要

4. **セキュリティ**
   - AgentのIPアドレスをクライアントに公開しない
   - Coordinatorだけ外部公開すればOK

5. **スケーラビリティ**
   - Agentを追加すれば自動的に処理能力が向上
   - クライアント側の変更不要

## プロジェクト構成

```
ollama-coordinator/
├── common/              # 共通ライブラリ（型定義、プロトコル、エラー）
│   ├── src/
│   │   ├── types.rs     # Agent, HealthMetrics, Request型定義
│   │   ├── protocol.rs  # 通信プロトコル定義
│   │   ├── config.rs    # 設定構造体
│   │   └── error.rs     # 統一エラー型
│   └── Cargo.toml
├── coordinator/         # Coordinatorサーバー
│   ├── src/
│   │   ├── api/         # REST APIハンドラー
│   │   │   ├── agent.rs    # エージェント登録・一覧
│   │   │   ├── health.rs   # ヘルスチェック受信
│   │   │   └── proxy.rs    # Ollamaプロキシ
│   │   ├── registry/    # エージェント状態管理
│   │   ├── db/          # データベースアクセス
│   │   └── main.rs
│   ├── migrations/      # データベースマイグレーション
│   └── Cargo.toml
├── agent/               # Agentアプリケーション
│   ├── src/
│   │   ├── ollama.rs    # Ollama自動管理
│   │   ├── client.rs    # Coordinator通信
│   │   ├── metrics.rs   # メトリクス収集
│   │   └── main.rs
│   ├── tests/
│   │   └── integration/ # 統合テスト
│   └── Cargo.toml
└── specs/               # 仕様書（Spec-Driven Development）
    └── SPEC-32e2b31a/
        ├── spec.md      # 機能仕様書
        ├── plan.md      # 実装計画
        └── tasks.md     # タスク分解
```

## インストール

### 必要要件

- **Coordinator**: Linux / Windows 10以降 / macOS 12以降、Rust 1.70以降
- **Agent**: Windows 10以降 / macOS 12以降（CLIベースアプリケーション）、Rust 1.70以降
- **Ollama**: 事前インストール推奨（自動ダウンロード機能は将来的な拡張）
- **管理**: ブラウザベースのWebUIダッシュボードでエージェント設定と監視

### Coordinatorのセットアップ

```bash
# リポジトリをクローン
git clone https://github.com/your-org/ollama-coordinator.git
cd ollama-coordinator

# Coordinatorをビルド
cd coordinator
cargo build --release

# Coordinatorを起動
./target/release/ollama-coordinator-coordinator
# デフォルト: http://0.0.0.0:8080
```

### Agentのセットアップ

```bash
# Agentをビルド
cd agent
cargo build --release

# Agentを起動
COORDINATOR_URL=http://coordinator-host:8080 ./target/release/ollama-coordinator-agent

# または環境変数なしで起動（デフォルト: http://localhost:8080）
./target/release/ollama-coordinator-agent
```

**注意**: Agent起動前に、マシン上でOllamaがインストールされ、起動していることを確認してください。Ollamaは[ollama.ai](https://ollama.ai)からダウンロードできます。

## 使い方

### 基本的な使い方

1. **Coordinatorを起動**
   ```bash
   cd coordinator
   cargo run --release
   ```

2. **複数のマシンでAgentを起動**
   ```bash
   # Machine 1
   COORDINATOR_URL=http://coordinator:8080 cargo run --release --bin ollama-coordinator-agent

   # Machine 2
   COORDINATOR_URL=http://coordinator:8080 cargo run --release --bin ollama-coordinator-agent

   # Machine 3
   COORDINATOR_URL=http://coordinator:8080 cargo run --release --bin ollama-coordinator-agent
   ```

3. **Coordinatorを通じてOllama APIを利用**
   ```bash
   # Chat API
   curl http://coordinator:8080/api/chat \
     -H "Content-Type: application/json" \
     -d '{
       "model": "llama2",
       "messages": [{"role": "user", "content": "Hello!"}],
       "stream": false
     }'

   # Generate API
   curl http://coordinator:8080/api/generate \
     -H "Content-Type: application/json" \
     -d '{
       "model": "llama2",
       "prompt": "Tell me a joke",
       "stream": false
     }'
   ```

4. **エージェント一覧を確認**
   ```bash
   curl http://coordinator:8080/api/agents
   ```

### 環境変数

#### Coordinator
- `COORDINATOR_HOST`: バインドアドレス（デフォルト: `0.0.0.0`）
- `COORDINATOR_PORT`: ポート番号（デフォルト: `8080`）
- `DATABASE_URL`: データベースURL（デフォルト: `sqlite://coordinator.db`）
- `HEALTH_CHECK_INTERVAL`: ヘルスチェック間隔（秒）（デフォルト: `30`）
- `AGENT_TIMEOUT`: エージェントタイムアウト（秒）（デフォルト: `60`）

#### Agent
- `COORDINATOR_URL`: CoordinatorのURL（デフォルト: `http://localhost:8080`）
- `OLLAMA_PORT`: Ollamaポート番号（デフォルト: `11434`）
- `OLLAMA_AGENT_MACHINE_NAME`: Coordinator登録時に使用するマシン名。未設定時は `OLLAMA_MACHINE_NAME` → `HOSTNAME` → `whoami::hostname()` の順で自動判定されます。
- `OLLAMA_PULL_TIMEOUT_SECS`: モデル自動ダウンロード時のHTTPタイムアウト秒数。未設定または `0` の場合はタイムアウトなしで待機します。
- `COORDINATOR_REGISTER_RETRY_SECS`: 登録リトライ間隔（秒）。未設定時は `5` 秒、`0` を指定すると即座に再試行します。
- `COORDINATOR_REGISTER_MAX_RETRIES`: 登録リトライ上限回数。未設定または `0` の場合は成功するまで無制限に再試行します。

## 開発

### テストの実行

```bash
# 全テスト実行
cargo test --workspace

# Coordinatorのテスト
cd coordinator
cargo test

# Agentのテスト
cd agent
cargo test

# 統合テスト（ignored含む、Coordinatorサーバーが必要）
cd agent
TEST_COORDINATOR_URL=http://localhost:8080 cargo test --test integration_tests -- --ignored
```

### Spec-Driven Development

本プロジェクトはSpec-Driven Developmentに従っています：

1. `/speckit.specify` - 機能仕様書作成
2. `/speckit.plan` - 実装計画作成
3. `/speckit.tasks` - タスク分解
4. タスク実行（TDDサイクル厳守）

詳細は[CLAUDE.md](./CLAUDE.md)を参照してください。

## API仕様

### Coordinator API

#### POST /api/agents
エージェントを登録します。

**リクエスト:**
```json
{
  "machine_name": "my-machine",
  "ip_address": "192.168.1.100",
  "ollama_version": "0.1.0",
  "ollama_port": 11434
}
```

**レスポンス:**
```json
{
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "registered"
}
```

#### GET /api/agents
登録されているエージェント一覧を取得します。

**レスポンス:**
```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "machine_name": "my-machine",
    "ip_address": "192.168.1.100",
    "ollama_version": "0.1.0",
    "ollama_port": 11434,
    "status": "online",
    "registered_at": "2025-10-30T12:00:00Z",
    "last_seen": "2025-10-30T12:05:00Z"
  }
]
```

#### POST /api/health
ヘルスチェック情報を受信します（Agent→Coordinator）。

**リクエスト:**
```json
{
  "agent_id": "550e8400-e29b-41d4-a716-446655440000",
  "cpu_usage": 45.5,
  "memory_usage": 60.2,
  "active_requests": 3
}
```

#### POST /api/chat
Ollama Chat APIへのプロキシエンドポイント。

**リクエスト/レスポンス:** [Ollama Chat API仕様](https://github.com/ollama/ollama/blob/main/docs/api.md#generate-a-chat-completion)に準拠

#### POST /api/generate
Ollama Generate APIへのプロキシエンドポイント。

**リクエスト/レスポンス:** [Ollama Generate API仕様](https://github.com/ollama/ollama/blob/main/docs/api.md#generate-a-completion)に準拠

## ライセンス

MIT License

## 貢献

Issue、Pull Requestをお待ちしています。

詳細な開発ガイドラインは[CLAUDE.md](./CLAUDE.md)を参照してください。
