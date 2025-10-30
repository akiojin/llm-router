# タスク: Ollama Coordinator System

**入力**: `/specs/SPEC-32e2b31a/`の設計ドキュメント
**前提条件**: plan.md (完了)

## 実行フロー

```
1. plan.mdから技術スタック抽出 → Rust Cargo Workspace (coordinator, agent, common)
2. データモデル抽出 → Agent, HealthMetrics, Request, Config
3. API契約抽出 → OpenAPI 3.0 (エージェント登録、ヘルスチェック、プロキシ)
4. タスク生成 → TDD順序 (Contract→Integration→実装→Unit→E2E)
5. 並列実行マーク → 異なるファイル/独立タスクに[P]
6. 依存関係検証 → テストが実装より先
```

## フォーマット: `[ID] [P?] 説明`

- **[P]**: 並列実行可能 (異なるファイル、依存関係なし)
- 説明には正確なファイルパスを含める

## パス規約

- **Cargo Workspace**: `coordinator/`, `agent/`, `common/`
- **テスト**: `coordinator/tests/`, `agent/tests/`, `tests/e2e/`
- `common/` → `coordinator/` と `agent/` の依存関係

---

## Phase 3.1: セットアップ (並列実行可能)

- [ ] **T001** [P] `Cargo.toml` にCargo Workspaceを定義 (members: coordinator, agent, common)
- [ ] **T002** [P] `common/Cargo.toml` を作成し、共通依存クレートを追加 (serde, thiserror, config, uuid, chrono)
- [ ] **T003** [P] `coordinator/Cargo.toml` を作成し、Coordinator依存クレートを追加 (axum, tokio, reqwest, sqlx, tower-http, tracing)
- [ ] **T004** [P] `agent/Cargo.toml` を作成し、Agent依存クレートを追加 (tauri, tokio, reqwest, sysinfo, tray-icon)
- [ ] **T005** [P] `coordinator/src/db/schema.sql` にSQLiteスキーマを定義 (agents, health_metrics, requests)
- [ ] **T006** [P] `.cargo/config.toml` にビルド設定を追加 (SQLx offline mode)
- [ ] **T007** [P] `.github/workflows/ci.yml` にCI/CD設定 (テスト、ビルド、リリース)
- [ ] **T008** [P] `rustfmt.toml` と `clippy.toml` を作成

---

## Phase 3.2: テストファースト (TDD - RED) ⚠️ Phase 3.3の前に完了必須

**重要: これらのテストは記述され、実装前に失敗する必要がある (RED)**

### Contract Tests (並列実行可能)

- [ ] **T009** [P] `coordinator/tests/contract/test_agent_registration.rs` にエージェント登録 Contract Test (POST /api/agents/register)
- [ ] **T010** [P] `coordinator/tests/contract/test_health_check.rs` にヘルスチェック Contract Test (POST /api/health)
- [ ] **T011** [P] `coordinator/tests/contract/test_proxy_chat.rs` にプロキシChat Contract Test (POST /api/chat)
- [ ] **T012** [P] `coordinator/tests/contract/test_proxy_generate.rs` にプロキシGenerate Contract Test (POST /api/generate)
- [ ] **T013** [P] `coordinator/tests/contract/test_agents_list.rs` にエージェント一覧 Contract Test (GET /api/agents)

### Integration Tests (並列実行可能)

- [ ] **T014** [P] `coordinator/tests/integration/test_agent_lifecycle.rs` にエージェントライフサイクル Integration Test (登録→ヘルスチェック→オフライン検知)
- [ ] **T015** [P] `coordinator/tests/integration/test_proxy.rs` にプロキシ Integration Test (リクエスト振り分け→Ollama転送)
- [ ] **T016** [P] `coordinator/tests/integration/test_load_balancing.rs` にロードバランシング Integration Test (複数リクエスト分散)
- [ ] **T017** [P] `coordinator/tests/integration/test_health_monitor.rs` にヘルスモニター Integration Test (タイムアウト検知)
- [ ] **T018** [P] `coordinator/tests/integration/test_dashboard.rs` にダッシュボード Integration Test (WebSocket接続)

---

## Phase 3.3: Common層実装 (テストが失敗した後のみ - GREEN)

**依存関係**: T009-T018が完了（RED状態確認済み）

### 共通型定義 (並列実行可能)

- [ ] **T019** [P] `common/src/lib.rs` にモジュール公開設定
- [ ] **T020** [P] `common/src/types.rs` にAgent, AgentStatus, HealthMetrics, Request, RequestStatus型定義
- [ ] **T021** [P] `common/src/protocol.rs` にRegisterRequest, RegisterResponse, HealthCheckRequest定義
- [ ] **T022** [P] `common/src/config.rs` にCoordinatorConfig, AgentConfig定義
- [ ] **T023** [P] `common/src/error.rs` にエラー型定義 (thiserror使用)

### Common層Unit Tests (並列実行可能)

- [ ] **T024** [P] `common/tests/unit/test_types.rs` に型のシリアライゼーション Unit Test
- [ ] **T025** [P] `common/tests/unit/test_protocol.rs` にプロトコルのバリデーション Unit Test
- [ ] **T026** [P] `common/tests/unit/test_config.rs` に設定ファイル読み込み Unit Test

---

## Phase 3.4: Coordinator実装 (依存関係順)

**依存関係**: T019-T026が完了（Common層完成）

### エージェント登録API (Contract Test T009をGREENに)

- [ ] **T027** `coordinator/src/api/mod.rs` にAPIモジュール構造定義
- [ ] **T028** `coordinator/src/api/agents.rs` にエージェント登録ハンドラー実装 (POST /api/agents/register)
- [ ] **T029** `coordinator/src/registry/mod.rs` にエージェント登録管理モジュール
- [ ] **T030** `coordinator/src/registry/manager.rs` にAgentRegistryManager実装 (登録・更新・削除)
- [ ] **T031** **検証**: Contract Test T009が合格 (GREEN)

### ヘルスチェックAPI (Contract Test T010をGREENに)

- [ ] **T032** `coordinator/src/api/health.rs` にヘルスチェックハンドラー実装 (POST /api/health)
- [ ] **T033** `coordinator/src/health/mod.rs` にヘルスチェックモジュール
- [ ] **T034** `coordinator/src/health/monitor.rs` にHealthMonitor実装 (定期チェック、タイムアウト検知)
- [ ] **T035** **検証**: Contract Test T010が合格 (GREEN)

### プロキシAPI (Contract Test T011-T012をGREENに)

- [ ] **T036** `coordinator/src/api/proxy.rs` にOllamaプロキシハンドラー実装 (POST /api/chat, /api/generate)
- [ ] **T037** `coordinator/src/balancer/mod.rs` にロードバランサーモジュール
- [ ] **T038** `coordinator/src/balancer/round_robin.rs` にRoundRobinBalancer実装 (Atomicカウンター使用)
- [ ] **T039** **検証**: Contract Test T011-T012が合格 (GREEN)

### エージェント一覧API (Contract Test T013をGREENに)

- [ ] **T040** `coordinator/src/api/agents.rs` にエージェント一覧ハンドラー追加 (GET /api/agents)
- [ ] **T041** **検証**: Contract Test T013が合格 (GREEN)

### DB永続化 (Integration Test T014をGREENに)

- [ ] **T042** `coordinator/src/db/mod.rs` にデータベースモジュール
- [ ] **T043** `coordinator/src/db/queries.rs` にSQLxクエリ実装 (agents, health_metrics, requests)
- [ ] **T044** Coordinatorサーバー起動時にSQLiteマイグレーション実行
- [ ] **T045** **検証**: Integration Test T014が合格 (GREEN)

### ロードバランシング強化 (Integration Test T015-T016をGREENに)

- [ ] **T046** `coordinator/src/balancer/load_based.rs` にLoadBasedBalancer実装 (CPU/メモリベース選択)
- [ ] **T047** **検証**: Integration Test T015-T016が合格 (GREEN)

### ヘルスモニター統合 (Integration Test T017をGREENに)

- [ ] **T048** Coordinator起動時にHealthMonitorバックグラウンドタスク開始
- [ ] **T049** **検証**: Integration Test T017が合格 (GREEN)

### ダッシュボード (Integration Test T018をGREENに)

- [ ] **T050** `coordinator/src/api/dashboard.rs` にダッシュボードHTML配信ハンドラー (GET /dashboard)
- [ ] **T051** `coordinator/src/api/dashboard.rs` にWebSocketハンドラー実装 (GET /ws/dashboard)
- [ ] **T052** `coordinator/static/dashboard.html` にダッシュボードHTMLファイル作成
- [ ] **T053** **検証**: Integration Test T018が合格 (GREEN)

### Coordinatorメインアプリケーション

- [ ] **T054** `coordinator/src/main.rs` にエントリポイント実装 (Axumサーバー起動、ルーティング設定)
- [ ] **T055** `coordinator/src/config.rs` に設定ファイル読み込み実装 (環境変数 + TOML)
- [ ] **T056** `coordinator/src/lib.rs` にライブラリ公開設定

---

## Phase 3.5: Agent実装 (依存関係順)

**依存関係**: T027-T056が完了（Coordinator完成）

### Coordinator通信クライアント (並列実行可能)

- [ ] **T057** [P] `agent/src/client/mod.rs` にCoordinator通信モジュール
- [ ] **T058** [P] `agent/src/client/register.rs` に自己登録クライアント実装 (POST /api/agents/register)
- [ ] **T059** [P] `agent/src/client/heartbeat.rs` にハートビートクライアント実装 (POST /api/health、10秒間隔)

### Ollama管理 (並列実行可能)

- [ ] **T060** [P] `agent/src/ollama/mod.rs` にOllama管理モジュール
- [ ] **T061** [P] `agent/src/ollama/monitor.rs` にOllama状態監視実装 (HTTP APIヘルスチェック)
- [ ] **T062** [P] `agent/src/ollama/proxy.rs` にOllamaプロキシ実装 (Coordinator→Agent→Ollama転送)

### メトリクス収集 (並列実行可能)

- [ ] **T063** [P] `agent/src/metrics/mod.rs` にメトリクスモジュール
- [ ] **T064** [P] `agent/src/metrics/collector.rs` にメトリクスコレクター実装 (sysinfo使用、CPU/メモリ監視)

### GUI（Tauri） (順次実行)

- [ ] **T065** `agent/src-tauri/tauri.conf.json` にTauri設定ファイル作成 (アプリ名、バージョン、アイコン)
- [ ] **T066** `agent/src/gui/mod.rs` にGUIモジュール
- [ ] **T067** `agent/src/gui/tray.rs` にシステムトレイ実装 (tray-icon使用)
- [ ] **T068** `agent/src/gui/window.rs` に設定ウィンドウ実装 (CoordinatorURL設定、接続状態表示)
- [ ] **T069** `agent/src-tauri/icons/` にアイコンファイル追加 (32x32.png, 128x128.png, icon.ico)

### Agentメインアプリケーション

- [ ] **T070** `agent/src/main.rs` にエントリポイント実装 (Tauriアプリ起動、バックグラウンドタスク開始)
- [ ] **T071** `agent/src/config.rs` に設定ファイル読み込み実装 (環境変数 + TOML)
- [ ] **T072** `agent/src/lib.rs` にライブラリ公開設定

### Windows統合

- [ ] **T073** Agentアプリのスタートアップ登録機能実装 (Windowsレジストリ操作)
- [ ] **T074** WiXインストーラー定義ファイル作成 (`agent/installer.wxs`)

---

## Phase 3.6: E2Eテスト (ユーザーストーリー順)

**依存関係**: T057-T074が完了（Agent完成）

### E2Eテストセットアップ

- [ ] **T075** [P] `tests/e2e/setup.rs` にE2Eテスト環境セットアップ (Coordinator起動、Agent起動、モックOllama)

### ユーザーストーリーE2Eテスト (並列実行可能)

- [ ] **T076** [P] `tests/e2e/scenarios/agent_registration.rs` にP1エージェント登録E2Eテスト
- [ ] **T077** [P] `tests/e2e/scenarios/proxy_api.rs` にP2統一APIプロキシE2Eテスト
- [ ] **T078** [P] `tests/e2e/scenarios/load_balancing.rs` にP3ロードバランシングE2Eテスト
- [ ] **T079** [P] `tests/e2e/scenarios/health_check.rs` にP4ヘルスチェックE2Eテスト
- [ ] **T080** [P] `tests/e2e/scenarios/dashboard.rs` にP5ダッシュボードE2Eテスト

---

## Phase 3.7: 仕上げ (並列実行可能)

**依存関係**: T075-T080が完了（E2Eテスト合格）

### Unit Tests

- [ ] **T081** [P] `coordinator/tests/unit/test_round_robin.rs` にRoundRobinBalancer Unit Test
- [ ] **T082** [P] `coordinator/tests/unit/test_load_based.rs` にLoadBasedBalancer Unit Test
- [ ] **T083** [P] `coordinator/tests/unit/test_agent_manager.rs` にAgentRegistryManager Unit Test
- [ ] **T084** [P] `agent/tests/unit/test_metrics_collector.rs` にMetricsCollector Unit Test
- [ ] **T085** [P] `agent/tests/unit/test_ollama_monitor.rs` にOllamaMonitor Unit Test

### パフォーマンステスト

- [ ] **T086** [P] `tests/performance/test_request_latency.rs` にリクエスト振り分けレイテンシテスト (<50ms)
- [ ] **T087** [P] `tests/performance/test_agent_registration.rs` にエージェント登録速度テスト (<5秒)
- [ ] **T088** [P] `tests/performance/test_health_check_timeout.rs` に障害検知速度テスト (<60秒)

### ドキュメント

- [ ] **T089** [P] `README.md` を更新 (プロジェクト説明、セットアップ手順、使用方法)
- [ ] **T090** [P] `README.ja.md` を作成 (日本語README)
- [ ] **T091** [P] `CLAUDE.md` を更新 (Rust開発ガイドライン、TDD遵守、非同期パターン)
- [ ] **T092** [P] `specs/SPEC-32e2b31a/quickstart.md` を作成 (開発者クイックスタート)

### コード品質

- [ ] **T093** [P] `cargo clippy` でリンティング、警告をすべて解消
- [ ] **T094** [P] `cargo fmt` でフォーマット統一
- [ ] **T095** [P] 重複コードをリファクタリング
- [ ] **T096** [P] エラーメッセージを改善（ユーザーフレンドリーなメッセージ）

---

## 依存関係グラフ

```
Setup (T001-T008) → すべての実装タスクをブロック
  ↓
Contract Tests (T009-T013) → RED確認必須
Integration Tests (T014-T018) → RED確認必須
  ↓
Common層 (T019-T026) → Coordinator/Agent実装をブロック
  ↓
Coordinator実装 (T027-T056) → Agent実装をブロック
  ↓
Agent実装 (T057-T074)
  ↓
E2Eテスト (T075-T080)
  ↓
仕上げ (T081-T096)
```

## 並列実行例

### セットアップフェーズ (T001-T008)

```bash
# すべて並列実行可能
cargo init --name ollama-coordinator
mkdir -p common coordinator agent tests/e2e
```

### Contract Testsフェーズ (T009-T013)

```rust
// すべて異なるファイルなので並列実行可能
// T009: coordinator/tests/contract/test_agent_registration.rs
// T010: coordinator/tests/contract/test_health_check.rs
// T011: coordinator/tests/contract/test_proxy_chat.rs
// T012: coordinator/tests/contract/test_proxy_generate.rs
// T013: coordinator/tests/contract/test_agents_list.rs
```

### Common層実装 (T019-T026)

```rust
// 型定義は並列実行可能 (異なるファイル)
// T020: common/src/types.rs
// T021: common/src/protocol.rs
// T022: common/src/config.rs
// T023: common/src/error.rs
```

### E2Eテスト (T076-T080)

```rust
// すべて並列実行可能 (異なるシナリオ)
// T076: tests/e2e/scenarios/agent_registration.rs
// T077: tests/e2e/scenarios/proxy_api.rs
// T078: tests/e2e/scenarios/load_balancing.rs
// T079: tests/e2e/scenarios/health_check.rs
// T080: tests/e2e/scenarios/dashboard.rs
```

---

## 注意事項

- **TDD厳守**: テストコミット → RED確認 → 実装コミット → GREEN確認
- **[P]タスク**: 異なるファイル、依存関係なし → 並列実行可能
- **順次タスク**: 同じファイル、依存関係あり → 順番に実行
- **各タスク後にコミット**: git commit -m "test(xxx): xxxのテスト追加" → git commit -m "feat(xxx): xxx実装"
- **Contract Test合格確認**: 各API実装後、必ず対応するContract Testが合格することを確認
- **Integration Test合格確認**: 統合機能実装後、必ず対応するIntegration Testが合格することを確認

---

## タスク生成ルール

1. **テストファースト**: Contract Tests → Integration Tests → 実装 → Unit Tests → E2E Tests
2. **依存関係順序**: Common → Coordinator → Agent
3. **並列実行**: 異なるファイル = [P]マーク
4. **ファイルパス明確**: 各タスクに正確なファイルパスを記載
5. **検証ステップ**: Contract Test合格確認タスクを明示的に追加

---

**総タスク数**: 96タスク
**並列実行可能タスク**: 約60タスク（[P]マーク）
**推定完了時間**: 4-6週間（TDDサイクル遵守、パフォーマンス最適化含む）

---

*生成元: `/specs/SPEC-32e2b31a/plan.md`*
*TDD順序: Contract→Integration→E2E→Unit*
*憲章 v1.0.0 準拠*
