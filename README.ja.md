# Ollama Coordinator

複数マシンでOllamaインスタンスを管理する中央集権型システム

[English](./README.md) | 日本語

## 概要

Ollama Coordinatorは、複数のマシン上で動作するOllamaインスタンスを一元管理し、統一されたAPIエンドポイントを提供するシステムです。インテリジェントなロードバランシング、自動障害検知、リアルタイム監視機能を備えています。

## 主な特徴

- **統一APIエンドポイント**: 複数のOllamaインスタンスを単一のURLで利用可能
- **自動ロードバランシング**: リクエストを利用可能なエージェントに自動分散
- **自動障害検知**: オフラインエージェントを自動検知して振り分けから除外
- **リアルタイム監視**: Webダッシュボードで全エージェントの状態を可視化
- **リクエスト履歴記録**: 完全なリクエスト/レスポンスログを7日間保持
- **エージェント自己登録**: エージェントが自動的にCoordinatorに登録
- **WebUI管理**: ブラウザベースのダッシュボードでエージェント設定、監視、制御が可能
- **クロスプラットフォーム対応**: Windows 10+、macOS 12+、Linuxで動作
- **GPU対応ルーティング**: GPU能力と可用性に基づくインテリジェントなリクエストルーティング

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
- **Ollama**: 未インストールの場合は初回起動時に自動ダウンロード・インストール（進捗表示・再試行・SHA256検証付き）
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

**注意**: Agentは起動時にOllamaの存在を確認し、未インストールなら自動的にバイナリをダウンロード・検証・展開してから起動します。手動インストールが必要な場合は[ollama.ai](https://ollama.ai)から取得できます。

### リリースバイナリの作成と公開

GitHubリリースには各プラットフォーム向けのバイナリを同梱します。基本手順は以下のとおりです。

1. リリース用のタグを作成する前に `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test` を通し、品質チェックを完了させる。
2. ターゲットごとにリリースビルドを実行する。
   ```bash
   # Linux (x86_64)
   cargo build --release --target x86_64-unknown-linux-gnu

   # Windows
   cargo build --release --target x86_64-pc-windows-msvc

   # macOS (Apple Silicon)
   cargo build --release --target aarch64-apple-darwin

   # macOS (Intel)
   cargo build --release --target x86_64-apple-darwin
   ```
3. 生成されたバイナリ（`target/<target>/release/` 配下の `ollama-coordinator-coordinator` と `ollama-coordinator-agent`）を `.tar.gz` もしくは `.zip` にまとめ、README・CHANGELOGなど必要ファイルを同梱する。
4. GitHubリポジトリでリリースを作成し、各プラットフォーム向けアーカイブをアップロードする。リリースノートには対応プラットフォーム・ハッシュ値（任意）・既知の制限事項を記載する。
5. 必要に応じて自動化（GitHub Actions 等）で上記手順を再現し、リリースタグ作成と同時にアーティファクトをアップロードする。  
   本リポジトリでは `.github/workflows/semantic-release.yml` が Conventional Commits からバージョンを決定して `Cargo.toml` 群と `CHANGELOG.md` を更新し、その後 `.github/workflows/release-binaries.yml` を呼び出して各プラットフォーム向けアーカイブを生成・検証した上で GitHub Release に添付します。
   - `main` ブランチが保護されている場合、GitHub Actions の既定トークンではリリースコミットやタグ作成がブロックされます。対象リポジトリに限定した **Fine-grained Personal Access Token** を作成し、リポジトリシークレット `PERSONAL_ACCESS_TOKEN` に登録してください。最低限必要な権限は次のとおりです: Contents (Read & write)、Metadata (Read)、Actions (Read)、Workflows (Read & write)、Issues (Read & write)、Pull requests (Read & write)、Releases (Read & write)。有効期限を短めに設定し、定期的にローテーションすることを推奨します。詳細手順は `CLAUDE.md` の「semantic-release トークン設定」を参照してください。

### リリース自動化

ブランチ戦略は `feature/*` → `develop` → `main` です。

- `feature/*`: 個別機能。短期間で `develop` に統合します。
- `develop`: リリース候補を常に保持する統合ブランチ。
- `main`: 本番/リリース履歴。ここへのマージがそのまま出荷されます。

このリポジトリでは、`akiojin/unity-mcp-server` と同じ release ブランチ方式（方法B）で正式リリースを自動化しています。

1. 開発者は `develop` ブランチ上で `/release` コマンド、もしくは `./scripts/create-release-branch.sh` を実行します。内部では `scripts/create-release-branch.sh` が `gh workflow run create-release.yml --ref develop` を呼び出し、semantic-release のドライランで次バージョンを計算しつつ `release/vX.Y.Z` ブランチを作成・push します。
2. release ブランチの push を契機に `.github/workflows/release.yml` が起動し、semantic-release 本番実行 → CHANGELOG / Cargo.toml / バージョンタグ更新 → main への自動マージ → develop へのバックマージ → release ブランチ削除までを一括で行います。
3. main へのマージにより `.github/workflows/publish.yml` が動作し、`release-binaries.yml` を呼び出して Linux / macOS (x86_64, ARM64) / Windows 向けバイナリをビルド・検証し、GitHub Release に添付します。

人手が必要なのは `/release` の実行と、必要に応じた進捗モニタリング（`gh run watch …`）だけです。バージョン決定からリリースノート生成、develop への同期まで CI が自動で完了させます。

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

### コミットフック

HuskyによるGitフックを有効化するため、クローンごとに一度JavaScriptツールチェーンをセットアップしてください。

```bash
pnpm install
```

- `prepare`スクリプト経由でHuskyのフックディレクトリを設定します。
- `commit-msg`フックで `commitlint --edit "$1"` が実行され、CIの前にローカルで規約違反を検出できます。
- コミット範囲を手動でチェックしたい場合は `pnpm run lint:commits`（デフォルト: `origin/main..HEAD`）を利用してください。

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

# 品質ゲート一式（fmt / clippy / workspaceテスト / specifyチェック / markdownlint / OpenAIテスト）
make quality-checks

# OpenAI互換APIのみ個別に実行する場合
make openai-tests
```

### macOSビルド（クロスコンパイル）

Linux環境（Docker）からmacOS向けバイナリをビルドできます。

#### 前提条件

1. macOS SDKの取得（Mac上で実施）

   詳細は [docs/macos-sdk-setup.md](./docs/macos-sdk-setup.md) を参照してください。

   ```bash
   # macOS上で実行（概要）
   cd /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/
   tar -cJf ~/MacOSX14.2.sdk.tar.xz MacOSX14.2.sdk

   # プロジェクトに配置
   mkdir -p /path/to/ollama-coordinator/.sdk
   cp ~/MacOSX14.2.sdk.tar.xz /path/to/ollama-coordinator/.sdk/
   ```

2. Dockerイメージのビルド

   ```bash
   # SDKバージョンを指定してビルド（デフォルト: 14.2）
   docker-compose build

   # または環境変数で指定
   SDK_VERSION=14.5 docker-compose build
   ```

#### ビルド手順

```bash
# Docker環境に入る
docker-compose run --rm ollama-coordinator bash

# Intel Mac向けビルド
make build-macos-x86_64

# Apple Silicon向けビルド
make build-macos-aarch64

# 両方のアーキテクチャをビルド
make build-macos-all
```

成果物は以下に出力されます：

- `target/x86_64-apple-darwin/release/ollama-coordinator-coordinator`
- `target/x86_64-apple-darwin/release/ollama-coordinator-agent`
- `target/aarch64-apple-darwin/release/ollama-coordinator-coordinator`
- `target/aarch64-apple-darwin/release/ollama-coordinator-agent`

**注意**: macOSバイナリのコード署名とnotarizationは、macOS環境で実施する必要があります。

### Spec-Driven Development

本プロジェクトはSpec-Driven Developmentに従っています：

1. `/speckit.specify` - 機能仕様書作成
2. `/speckit.plan` - 実装計画作成
3. `/speckit.tasks` - タスク分解
4. タスク実行（TDDサイクル厳守）

詳細は[CLAUDE.md](./CLAUDE.md)を参照してください。

## リクエスト履歴

Ollama Coordinatorは、デバッグ、監査、分析のために、すべてのリクエストと
レスポンスを自動的にログ記録します。

### 機能

- **完全なリクエスト/レスポンスログ**: リクエスト本文、レスポンス本文、
メタデータを完全に記録
- **自動保持**: 7日間履歴を保持し、自動的にクリーンアップ
- **Webダッシュボード**: Web画面でリクエスト履歴の閲覧、フィルタ、検索が可能
- **エクスポート機能**: JSON形式またはCSV形式で履歴をエクスポート
- **フィルタオプション**: モデル、エージェント、ステータス、時間範囲でフィルタ

### リクエスト履歴へのアクセス

#### Webダッシュボード経由

1. コーディネーターダッシュボードを開く: `http://localhost:8080/dashboard`
2. 「リクエスト履歴」セクションに移動
3. フィルタを使用して特定のリクエストを絞り込み
4. 任意のリクエストをクリックして、リクエスト/レスポンス本文を含む
完全な詳細を表示

#### API経由

**リクエスト履歴一覧取得:**
```bash
GET /api/dashboard/request-responses?page=1&per_page=50
```

**リクエスト詳細取得:**
```bash
GET /api/dashboard/request-responses/{id}
```

**履歴エクスポート:**
```bash
# JSON形式
GET /api/dashboard/request-responses/export

# CSV形式（ダッシュボードUI経由）
```

### ストレージ

リクエスト履歴はJSON形式で以下の場所に保存されます：
- Linux/macOS: `~/.ollama-coordinator/request_history.json`
- Windows: `%USERPROFILE%\.ollama-coordinator\request_history.json`

ファイルは以下の機能により自動管理されます：
- アトミック書き込み（一時ファイル + rename）による破損防止
- ファイルロックによる並行アクセス制御
- 7日より古いレコードの自動クリーンアップ

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
  "ollama_port": 11434,
  "gpu_available": true,
  "gpu_devices": [
    { "model": "NVIDIA RTX 4090", "count": 2 }
  ]
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
    "last_seen": "2025-10-30T12:05:00Z",
    "gpu_available": true,
    "gpu_devices": [
      { "model": "NVIDIA RTX 4090", "count": 2 }
    ],
    "gpu_count": 2,
    "gpu_model": "NVIDIA RTX 4090"
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
