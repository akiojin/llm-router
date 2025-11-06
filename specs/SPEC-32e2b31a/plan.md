# 実装計画: Ollama Coordinator System

**機能ID**: `SPEC-32e2b31a` | **日付**: 2025-10-30 | **仕様**: [spec.md](./spec.md)
**入力**: `/specs/SPEC-32e2b31a/spec.md`の機能仕様

## 実行フロー

```
1. 入力パスから機能仕様を読み込み → 完了
2. 技術コンテキストを記入 → 完了 (Rust, Windows専用, Cargo Workspace)
3. 憲章チェックを評価 → 合格
4. Phase 0 を実行 → research.md生成
5. Phase 1 を実行 → contracts, data-model.md, quickstart.md作成
6. 憲章チェック再評価 → 合格確認
7. Phase 2 を計画 → タスク生成アプローチを記述
8. 停止 - /speckit.tasks コマンドの準備完了
```

## 概要

複数マシンで動作するOllamaインスタンスを中央集権的に管理するシステム。Coordinatorサーバー（中央管理）とAgentアプリ（各マシン）で構成され、統一APIエンドポイント、ロードバランシング、ヘルスチェック、リアルタイムダッシュボードを提供する。Rustで実装し、高いパフォーマンスとメモリ効率を追求する。

**主要コンポーネント**:
- **Coordinator**: Axum（高速非同期Webフレームワーク）ベースのサーバー
- **Agent**: Tauri（クロスプラットフォームGUI）ベースのシステムトレイアプリ
- **Common**: 共通型定義、プロトコル定義、設定管理

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+ (stable)
**主要依存関係**:
- **Coordinator**: `axum` (Web)、`tokio` (非同期)、`reqwest` (HTTP)、`sqlx` (DB)、`tower-http` (CORS/ロギング)、`tracing` (構造化ログ)
- **Agent**: `tauri` (GUI)、`tokio` (非同期)、`reqwest` (HTTP)、`sysinfo` (メトリクス)、`tray-icon` (システムトレイ)
- **Common**: `serde` (JSON)、`thiserror` (エラー)、`config` (設定)

**ストレージ**: SQLite (Coordinator側、エージェント情報・メトリクス履歴)
**テスト**: `cargo test` (unit/integration)、`tokio::test` (非同期テスト)
**対象プラットフォーム**: Windows 10+ (Agent), Linux/Windows (Coordinator)
**プロジェクトタイプ**: multi (Cargo Workspace: coordinator, agent, common)
**パフォーマンス目標**: エージェント登録<5秒、リクエスト振り分け<50ms、障害検知<60秒
**制約**: Windows GUI必須（Tauri）、非同期I/O優先、メモリ消費<100MB/Agent
**スケール/スコープ**: 10台のエージェント同時管理、100req/min処理、24時間連続稼働

## 憲章チェック

**シンプルさ**:
- プロジェクト数: 3 (coordinator, agent, common) ✅
- フレームワークを直接使用? ✅ (Axum/Tauriを直接使用、ラッパークラスなし)
- 単一データモデル? ✅ (Common crateで共通型定義、シリアライゼーション用以外にDTOなし)
- パターン回避? ✅ (Repository/UoW不使用、SQLxで直接クエリ実行)

**アーキテクチャ**:
- すべての機能をライブラリとして? ✅
- ライブラリリスト:
  - `ollama_coordinator_common`: 共通型定義、プロトコル、設定、エラー型
  - `ollama_coordinator_coordinator`: Coordinatorサーバー本体（バイナリ + ライブラリ）
  - `ollama_coordinator_agent`: Agentアプリ本体（バイナリ + ライブラリ）
- ライブラリごとのCLI:
  - `coordinator`: `--help`, `--version`, `--config`, `--port` オプション
  - `agent`: `--help`, `--version`, `--config`, `--coordinator-url` オプション

**テスト (妥協不可)**:
- RED-GREEN-Refactorサイクルを強制? ✅ (テストコミット → REDチェック → 実装コミット)
- Gitコミットはテストが実装より先に表示? ✅ (コミット履歴で検証)
- 順序: Contract→Integration→E2E→Unit を厳密に遵守? ✅
  - Contract tests: API契約テスト（エンドポイント定義）
  - Integration tests: Coordinator↔Agent通信、Coordinator↔Ollama通信、DB永続化
  - E2E tests: エンドツーエンドシナリオ（エージェント登録→リクエスト振り分け→レスポンス）
  - Unit tests: 個別関数（ロードバランサーロジック、ヘルスチェックロジック）
- 実依存関係を使用? ✅ (実SQLite DB、モックではない)
- Integration testの対象: 新クレート、契約変更、共通スキーマ ✅
- 禁止: テスト前の実装、REDフェーズのスキップ ✅

**可観測性**:
- 構造化ロギング含む? ✅ (`tracing` + `tracing-subscriber`)
- エラーコンテキスト十分? ✅ (`thiserror`でエラー型定義、コンテキスト付きエラー)

**バージョニング**:
- バージョン番号割り当て済み? ✅ (0.1.0から開始、Cargo.toml)
- 変更ごとにBUILDインクリメント? ✅
- 破壊的変更を処理? ✅ (API契約バージョニング)

## プロジェクト構造

### ドキュメント (この機能)

```
specs/SPEC-32e2b31a/
├── spec.md              # 機能仕様書
├── plan.md              # このファイル (実装計画)
├── research.md          # Phase 0 出力 (技術リサーチ)
├── data-model.md        # Phase 1 出力 (データモデル定義)
├── quickstart.md        # Phase 1 出力 (開発者クイックスタート)
├── contracts/           # Phase 1 出力 (API契約定義)
│   ├── coordinator-api.yaml    # Coordinator REST API (OpenAPI 3.0)
│   └── agent-protocol.md       # Agent↔Coordinator通信プロトコル
└── tasks.md             # Phase 2 出力 (/speckit.tasks)
```

### ソースコード (リポジトリルート)

```
ollama-coordinator/
├── Cargo.toml                    # Workspace定義
├── Cargo.lock
├── .cargo/
│   └── config.toml              # ビルド設定
│
├── common/                      # 共通ライブラリ
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── types.rs             # Agent, HealthStatus, Metrics
│       ├── protocol.rs          # 通信プロトコル (RegisterRequest, HealthCheckResponse)
│       ├── config.rs            # 設定構造体
│       └── error.rs             # 統一エラー型
│
├── coordinator/                 # Coordinatorサーバー
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs             # エントリポイント
│   │   ├── lib.rs              # ライブラリ公開
│   │   ├── config.rs           # 設定読み込み
│   │   ├── api/                # REST APIハンドラー
│   │   │   ├── mod.rs
│   │   │   ├── agents.rs       # エージェント登録・一覧
│   │   │   ├── health.rs       # ヘルスチェック受信
│   │   │   ├── proxy.rs        # Ollama統一APIプロキシ
│   │   │   └── dashboard.rs    # ダッシュボードAPI (WebSocket)
│   │   ├── balancer/           # ロードバランサー
│   │   │   ├── mod.rs
│   │   │   ├── round_robin.rs  # ラウンドロビン戦略
│   │   │   └── load_based.rs   # 負荷ベース戦略
│   │   ├── health/             # ヘルスチェック
│   │   │   ├── mod.rs
│   │   │   └── monitor.rs      # 定期ヘルスチェック
│   │   ├── registry/           # エージェント登録管理
│   │   │   ├── mod.rs
│   │   │   └── manager.rs      # エージェント状態管理
│   │   └── db/                 # データベースアクセス
│   │       ├── mod.rs
│   │       ├── schema.sql      # SQLiteスキーマ
│   │       └── queries.rs      # SQLxクエリ
│   └── tests/
│       ├── contract/           # API契約テスト
│       ├── integration/        # 統合テスト
│       └── unit/               # ユニットテスト
│
├── agent/                       # Agentアプリ
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs             # エントリポイント
│   │   ├── lib.rs              # ライブラリ公開
│   │   ├── config.rs           # 設定読み込み
│   │   ├── gui/                # GUI (Tauri)
│   │   │   ├── mod.rs
│   │   │   ├── tray.rs         # システムトレイ
│   │   │   └── window.rs       # 設定ウィンドウ
│   │   ├── client/             # Coordinator通信
│   │   │   ├── mod.rs
│   │   │   ├── register.rs     # 自己登録
│   │   │   └── heartbeat.rs    # ヘルスチェック送信
│   │   ├── ollama/             # Ollama管理
│   │   │   ├── mod.rs
│   │   │   ├── monitor.rs      # Ollama状態監視
│   │   │   └── proxy.rs        # Ollamaプロキシ
│   │   └── metrics/            # メトリクス収集
│   │       ├── mod.rs
│   │       └── collector.rs    # CPU/メモリ監視
│   ├── src-tauri/              # Tauri設定
│   │   ├── tauri.conf.json
│   │   └── icons/
│   └── tests/
│       ├── integration/
│       └── unit/
│
└── tests/                       # Workspaceレベルテスト
    └── e2e/                     # E2Eテスト
        ├── setup.rs             # テスト環境セットアップ
        └── scenarios/
            ├── agent_registration.rs
            ├── load_balancing.rs
            └── health_check.rs
```

**構造決定**: multi-project (Cargo Workspace)

## Phase 0: アウトライン＆リサーチ

### リサーチタスク

1. **Rustクレート選定**:
   - Webフレームワーク: Axum vs Actix-web vs Rocket (→ Axumを選定: Tokio統合、型安全性、パフォーマンス)
   - GUIフレームワーク: Tauri vs Iced vs egui (→ Tauriを選定: Windows統合、システムトレイ、リソース効率)
   - HTTPクライアント: reqwest vs hyper (→ reqwestを選定: 高レベルAPI、非同期対応)
   - データベース: SQLx vs Diesel (→ SQLxを選定: 非同期対応、コンパイル時クエリ検証)

2. **Windows GUI統合**:
   - Tauriのシステムトレイ実装パターン (`tray-icon` crate)
   - 設定ウィンドウの表示/非表示制御
   - Windows起動時の自動起動 (レジストリ登録 vs スタートアップフォルダ)

3. **非同期アーキテクチャ**:
   - Tokio非同期ランタイムの設計パターン
   - 非同期チャネル (`tokio::sync::mpsc`) でのタスク間通信
   - タイムアウトとキャンセレーション (`tokio::time::timeout`, `tokio::select!`)

4. **Ollama API統合**:
   - Ollama HTTP API仕様 (`/api/chat`, `/api/generate`, `/api/tags`)
   - ストリーミングレスポンスの処理 (Server-Sent Events)
   - エラーハンドリングと再試行戦略

5. **ロードバランシング戦略**:
   - ラウンドロビン実装（Atomicカウンター）
   - 負荷ベース実装（CPU/メモリメトリクス収集）
   - 重み付けアルゴリズム

6. **WebSocketリアルタイム更新**:
   - Axum WebSocketハンドラー
   - ブロードキャストチャネルでのイベント配信
   - クライアント側JavaScript実装

**出力**: `research.md` に各決定の理由、代替案、ベストプラクティスを文書化

## Phase 1: 設計＆契約

### 1. データモデル (`data-model.md`)

#### エンティティ定義

**Agent**:
```rust
pub struct Agent {
    pub id: Uuid,                  // 一意識別子
    pub machine_name: String,      // マシン名
    pub ip_address: IpAddr,        // IPアドレス
    pub ollama_version: String,    // Ollamaバージョン
    pub ollama_port: u16,          // Ollamaポート番号
    pub status: AgentStatus,       // オンライン/オフライン
    pub registered_at: DateTime<Utc>,  // 登録日時
    pub last_seen: DateTime<Utc>,      // 最終ヘルスチェック時刻
}

pub enum AgentStatus {
    Online,
    Offline,
}
```

**HealthMetrics**:
```rust
pub struct HealthMetrics {
    pub agent_id: Uuid,
    pub cpu_usage: f32,            // CPU使用率 (0.0-100.0)
    pub memory_usage: f32,         // メモリ使用率 (0.0-100.0)
    pub active_requests: u32,      // 処理中リクエスト数
    pub total_requests: u64,       // 累積リクエスト数
    pub timestamp: DateTime<Utc>,
}
```

**Request**:
```rust
pub struct Request {
    pub id: Uuid,
    pub agent_id: Uuid,            // 振り分け先エージェント
    pub endpoint: String,          // "/api/chat" など
    pub status: RequestStatus,     // 処理中/完了/エラー
    pub duration_ms: Option<u64>,  // 処理時間
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub enum RequestStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}
```

**Config (Coordinator)**:
```rust
pub struct CoordinatorConfig {
    pub host: String,              // "0.0.0.0"
    pub port: u16,                 // 8080
    pub database_url: String,      // "sqlite://coordinator.db"
    pub health_check_interval_secs: u64,  // 30秒
    pub agent_timeout_secs: u64,   // 60秒
}
```

**Config (Agent)**:
```rust
pub struct AgentConfig {
    pub coordinator_url: String,   // "http://coordinator:8080"
    pub ollama_url: String,        // "http://localhost:11434"
    pub heartbeat_interval_secs: u64,  // 10秒
    pub auto_start: bool,          // Windows起動時の自動起動
}
```

### 2. API契約 (`contracts/coordinator-api.yaml`)

**OpenAPI 3.0仕様**:

```yaml
openapi: 3.0.3
info:
  title: Ollama Coordinator API
  version: 0.1.0
  description: 複数Ollamaインスタンスを管理する中央集権型システム

servers:
  - url: http://localhost:8080
    description: ローカル開発環境

paths:
  /api/agents/register:
    post:
      summary: エージェント登録
      operationId: registerAgent
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/RegisterRequest'
      responses:
        '200':
          description: 登録成功
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/RegisterResponse'
        '400':
          description: 不正なリクエスト

  /api/agents:
    get:
      summary: エージェント一覧取得
      operationId: listAgents
      responses:
        '200':
          description: 成功
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Agent'

  /api/health:
    post:
      summary: ヘルスチェック情報送信
      operationId: reportHealth
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/HealthCheckRequest'
      responses:
        '200':
          description: 受信成功

  /api/chat:
    post:
      summary: Ollama Chat API プロキシ
      operationId: proxyChat
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/ChatRequest'
      responses:
        '200':
          description: 成功
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/ChatResponse'
        '503':
          description: 利用可能なエージェントなし

  /api/generate:
    post:
      summary: Ollama Generate API プロキシ
      operationId: proxyGenerate
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/GenerateRequest'
      responses:
        '200':
          description: 成功

  /dashboard:
    get:
      summary: ダッシュボード (HTML)
      operationId: getDashboard
      responses:
        '200':
          description: HTMLページ
          content:
            text/html:
              schema:
                type: string

  /ws/dashboard:
    get:
      summary: ダッシュボード WebSocket (リアルタイム更新)
      operationId: dashboardWebSocket
      responses:
        '101':
          description: WebSocketアップグレード

components:
  schemas:
    RegisterRequest:
      type: object
      required: [machine_name, ip_address, ollama_version, ollama_port]
      properties:
        machine_name:
          type: string
        ip_address:
          type: string
        ollama_version:
          type: string
        ollama_port:
          type: integer

    RegisterResponse:
      type: object
      properties:
        agent_id:
          type: string
          format: uuid
        status:
          type: string
          enum: [registered, updated]

    Agent:
      type: object
      properties:
        id:
          type: string
          format: uuid
        machine_name:
          type: string
        ip_address:
          type: string
        ollama_version:
          type: string
        status:
          type: string
          enum: [online, offline]
        registered_at:
          type: string
          format: date-time
        last_seen:
          type: string
          format: date-time

    HealthCheckRequest:
      type: object
      required: [agent_id, cpu_usage, memory_usage, active_requests]
      properties:
        agent_id:
          type: string
          format: uuid
        cpu_usage:
          type: number
          format: float
        memory_usage:
          type: number
          format: float
        active_requests:
          type: integer

    ChatRequest:
      type: object
      required: [model, messages]
      properties:
        model:
          type: string
        messages:
          type: array
          items:
            type: object
        stream:
          type: boolean

    ChatResponse:
      type: object
      properties:
        message:
          type: object
        done:
          type: boolean

    GenerateRequest:
      type: object
      required: [model, prompt]
      properties:
        model:
          type: string
        prompt:
          type: string
        stream:
          type: boolean
```

### 3. Agent↔Coordinator通信プロトコル (`contracts/agent-protocol.md`)

**プロトコル仕様**:

1. **エージェント登録**:
   - Agent → Coordinator: `POST /api/agents/register` (起動時)
   - Coordinator → Agent: `RegisterResponse` (agent_id返却)

2. **ヘルスチェック（ハートビート）**:
   - Agent → Coordinator: `POST /api/health` (10秒間隔)
   - Payload: CPU使用率、メモリ使用率、処理中リクエスト数

3. **リクエスト振り分け**:
   - Client → Coordinator: `POST /api/chat` or `POST /api/generate`
   - Coordinator → Agent: HTTP Proxy (選択されたエージェントのOllama URLへ転送)
   - Agent → Ollama: ローカルOllama APIへ転送
   - Ollama → Agent → Coordinator → Client: レスポンス返却

4. **エージェント切断検知**:
   - Coordinator: 60秒以上ヘルスチェックがないエージェントを「オフライン」とマーク

### 4. Contract Tests生成

**テストファイル**:
- `tests/contract/test_agent_registration.rs`: `/api/agents/register` の契約テスト
- `tests/contract/test_health_check.rs`: `/api/health` の契約テスト
- `tests/contract/test_proxy_chat.rs`: `/api/chat` の契約テスト

**テスト内容**:
- リクエスト/レスポンススキーマ検証
- HTTPステータスコード検証
- エラーレスポンス形式検証

### 5. Integration Testシナリオ

**ユーザーストーリーからのテストシナリオ**:

1. **P1: エージェント登録** (`tests/integration/test_agent_lifecycle.rs`):
   - エージェント起動 → 登録リクエスト送信 → Coordinator受信 → DB保存 → agent_id返却
   - エージェント終了 → 60秒後にタイムアウト → オフライン状態に変更

2. **P2: 統一APIプロキシ** (`tests/integration/test_proxy.rs`):
   - リクエスト送信 → エージェント選択 → Ollamaへ転送 → レスポンス返却
   - エージェント0台 → 503エラー

3. **P3: ロードバランシング** (`tests/integration/test_load_balancing.rs`):
   - 複数リクエスト → ラウンドロビン分散 → 各エージェント均等処理

4. **P4: ヘルスチェック** (`tests/integration/test_health_monitor.rs`):
   - エージェント強制終了 → 60秒後にオフライン検知 → 振り分け対象から除外

5. **P5: ダッシュボード** (`tests/integration/test_dashboard.rs`):
   - WebSocket接続 → エージェント登録イベント → クライアント受信

### 6. エージェントファイル更新

**CLAUDE.md更新** (開発ガイドライン):
- Rust開発環境セットアップ
- TDD厳守（Red-Green-Refactor）
- 非同期処理パターン
- エラーハンドリングベストプラクティス

**出力**: `data-model.md`, `contracts/coordinator-api.yaml`, `contracts/agent-protocol.md`, contract tests (失敗), integration tests (失敗), `quickstart.md`, `CLAUDE.md`更新

## Phase 2: タスク計画アプローチ

**タスク生成戦略**:

1. **Setupタスク** ([P] 並列実行可):
   - Cargo Workspace初期化
   - 依存クレート追加 (axum, tokio, tauri, etc.)
   - SQLiteスキーマ定義 (`coordinator/src/db/schema.sql`)
   - CI/CD設定 (GitHub Actions: テスト、ビルド、リリース)
   - リリース配布フォーマットの検証（Unix系=`.tar.gz`、Windows=`.zip`／主要ドキュメント同梱確認）
   - リリースワークフローで `main` ブランチへのマージ後のみ配布が行われるようガードを設置

2. **Common層実装** ([P]):
   - 共通型定義 (`common/src/types.rs`) → Unit Test
   - プロトコル定義 (`common/src/protocol.rs`) → Unit Test
   - エラー型定義 (`common/src/error.rs`) → Unit Test
   - 設定管理 (`common/src/config.rs`) → Unit Test

3. **Contract Testsタスク** ([P]):
   - エージェント登録契約テスト (RED)
   - ヘルスチェック契約テスト (RED)
   - プロキシ契約テスト (RED)

4. **Coordinator実装** (依存関係順):
   - エージェント登録API実装 → Contract Test GREEN
   - ヘルスチェックAPI実装 → Contract Test GREEN
   - プロキシAPI実装 → Contract Test GREEN
   - ロードバランサー実装 → Unit Test
   - ヘルスモニター実装 → Unit Test
   - DB永続化実装 → Integration Test
   - ダッシュボード実装 → Integration Test

5. **Agent実装** (依存関係順):
   - Coordinator通信クライアント → Integration Test
   - Ollama監視 → Unit Test
   - メトリクス収集 → Unit Test
   - GUI（Tauri） → 手動テスト
   - システムトレイ統合 → 手動テスト

6. **E2Eテスト** (ユーザーストーリー順):
   - P1: エージェント登録シナリオ
   - P2: 統一APIプロキシシナリオ
   - P3: ロードバランシングシナリオ
   - P4: ヘルスチェックシナリオ
   - P5: ダッシュボードシナリオ

7. **Polishタスク**:
   - エラーメッセージ改善
   - ロギング追加
   - パフォーマンス最適化
   - ドキュメント作成（README更新）

**順序戦略**:
- TDD順序: Contract Test → Integration Test → 実装 → Unit Test → E2E Test
- 依存関係順序: Common → Coordinator → Agent
- 並列実行: Common内のモジュール、Contract Tests、Setup tasks

**推定出力**: tasks.mdに約40-50個のタスク

**重要**: このフェーズは `/speckit.tasks` コマンドで実行

## Ollama自動ダウンロード機能強化（追加要件）

**背景**: 基本的なOllama自動ダウンロード・インストール機能は実装済み（`agent/src/ollama.rs:download()`）。以下の4つの機能強化を追加実装する。

### 新機能要件と技術設計

#### 1. ダウンロード進捗表示（FR-016d）

**目的**: ユーザーがOllamaバイナリ/モデルのダウンロード状況を把握できるようにする

**技術アプローチ**:
- **依存クレート**: `indicatif` (プログレスバー表示)
- **実装方針**:
  - `reqwest`の`Response::bytes_stream()`でチャンク単位でダウンロード
  - `Content-Length`ヘッダーから総サイズを取得
  - `ProgressBar`で進捗率（パーセンテージ）、ダウンロード速度、ETA（推定残り時間）を表示
  - モデルプル時はOllama APIの進捗情報をパース

**実装場所**: `agent/src/ollama.rs:download()`, `agent/src/ollama.rs:pull_model()`

**環境変数**: `OLLAMA_DOWNLOAD_PROGRESS=false` で進捗表示を無効化可能

#### 2. ネットワークエラー時の自動リトライ（FR-016e）

**目的**: ネットワークの一時的な障害に対して自動復旧する

**技術アプローチ**:
- **依存クレート**: `backoff` (指数バックオフ実装) または手動実装
- **リトライ戦略**:
  - 初回失敗: 1秒待機
  - 2回目失敗: 2秒待機
  - 3回目失敗: 4秒待機
  - 4回目失敗: 8秒待機
  - 5回目失敗: 16秒待機
  - 6回目以降: 60秒固定（最大値）
  - 最大リトライ回数: 5回（環境変数で変更可能）
- **リトライ対象エラー**:
  - `reqwest::Error::is_timeout()`
  - `reqwest::Error::is_connect()`
  - HTTP 5xx系エラー
- **リトライ非対象**:
  - HTTP 4xx系エラー（404, 403など）
  - ディスク容量不足エラー

**実装場所**: `agent/src/ollama.rs:download()`, `agent/src/ollama.rs:pull_model()`

**環境変数**:
- `OLLAMA_DOWNLOAD_MAX_RETRIES=5` (デフォルト: 5)
- `OLLAMA_DOWNLOAD_MAX_BACKOFF_SECS=60` (デフォルト: 60)

#### 3. SHA256チェックサム検証（FR-016f）

**目的**: ダウンロードしたバイナリの整合性を保証し、改ざん・破損を検出

**技術アプローチ**:
- **依存クレート**: `sha2` (SHA256ハッシュ計算)
- **チェックサム取得**:
  - GitHub Releases APIから`ollama-{platform}-{arch}.sha256`ファイルをダウンロード
  - または`OLLAMA_CHECKSUM`環境変数で直接指定
- **検証フロー**:
  1. バイナリダウンロード完了後、ファイル全体のSHA256を計算
  2. 公式チェックサムと比較
  3. 不一致の場合はファイル削除＋エラー報告
  4. 一致の場合は展開・インストール継続
- **フォールバック**: チェックサムファイルが取得できない場合は警告表示のみ（検証スキップ）

**実装場所**: `agent/src/ollama.rs:download()` 内に`verify_checksum()`関数を追加

**環境変数**:
- `OLLAMA_CHECKSUM=<sha256>` (手動チェックサム指定)
- `OLLAMA_SKIP_CHECKSUM_VERIFICATION=true` (検証スキップ、非推奨)

#### 4. プロキシ対応（FR-016g）

**目的**: 企業ネットワーク等のプロキシ環境でもダウンロード可能にする

**技術アプローチ**:
- **依存クレート**: `reqwest` (標準でプロキシサポート)
- **プロキシ検出順序**:
  1. 環境変数`HTTP_PROXY`, `HTTPS_PROXY`を確認
  2. 環境変数`ALL_PROXY`を確認
  3. プロキシなしで接続
- **認証付きプロキシ対応**:
  - `http://user:pass@proxy.example.com:8080` 形式をサポート
- **プロキシ除外**: `NO_PROXY`環境変数で除外ホストを指定可能

**実装場所**: `agent/src/ollama.rs:download()` でHTTPクライアント作成時にプロキシ設定

**環境変数**:
- `HTTP_PROXY=http://proxy.example.com:8080`
- `HTTPS_PROXY=https://proxy.example.com:8443`
- `ALL_PROXY=http://proxy.example.com:8080`
- `NO_PROXY=localhost,127.0.0.1,.local`

### 依存関係追加

**agent/Cargo.toml**:
```toml
[dependencies]
# 既存
reqwest = { version = "0.11", features = ["stream", "rustls-tls"] }
tokio = { version = "1", features = ["full"] }

# 新規追加
indicatif = "0.17"        # プログレスバー
sha2 = "0.10"             # SHA256チェックサム
backoff = "0.4"           # 指数バックオフ（オプション: 手動実装も可）
```

### テスト戦略

**Contract Tests**:
- ダウンロード進捗コールバックAPI定義

**Integration Tests**:
- モックHTTPサーバーで進捗表示テスト（`wiremock` crate使用）
- ネットワークエラーシミュレーション（タイムアウト、接続エラー）
- チェックサム検証テスト（正常/不一致/欠損）
- プロキシ経由ダウンロードテスト（`mockito` crate使用）

**Unit Tests**:
- 指数バックオフ計算ロジック
- SHA256ハッシュ計算
- プロキシURL解析

### 実装優先順位

1. **P0 (最優先)**: リトライ機能（FR-016e）- ネットワーク環境での安定性向上
2. **P1**: プロキシ対応（FR-016g）- 企業環境での利用可能性
3. **P2**: ダウンロード進捗表示（FR-016d）- UX改善
4. **P3**: チェックサム検証（FR-016f）- セキュリティ強化

### 成功基準

- リトライ機能: ネットワークエラー時に最大5回自動リトライし、最終的に成功または明確なエラー報告
- プロキシ対応: `HTTP_PROXY`設定環境で正常にダウンロード完了
- 進捗表示: ダウンロード中に進捗率、速度、ETAがリアルタイム更新
- チェックサム検証: 改ざんファイルを検出してエラー報告

## Phase 3+: 今後の実装

**Phase 3**: タスク実行 (`/speckit.tasks` コマンドが tasks.md を作成)
**Phase 4**: 実装 (TDDサイクルに従ってタスク実行)
**Phase 5**: 検証 (テスト実行、パフォーマンステスト、24時間連続稼働テスト)

## 複雑さトラッキング

| 違反 | 必要な理由 | より単純な代替案が却下された理由 |
|------|-----------|--------------------------------|
| なし | - | - |

## 進捗トラッキング

**フェーズステータス**:
- [x] Phase 0: Research完了
- [ ] Phase 1: Design完了
- [ ] Phase 2: Task planning完了 (アプローチのみ記述)
- [ ] Phase 3: Tasks生成済み (`/speckit.tasks` コマンド)
- [ ] Phase 4: 実装完了
- [ ] Phase 5: 検証合格

**ゲートステータス**:
- [x] 初期憲章チェック: 合格
- [ ] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱を文書化済み (違反なし)

---
*憲章 v1.0.0 に基づく - `/memory/constitution.md` 参照*
