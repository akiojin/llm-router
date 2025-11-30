# タスク: ロードバランシングシステム

**ステータス**: ✅ **実装完了** (Phase 1-2完了: 24/24タスク = 100%)
**入力**: `/llm-router/specs/SPEC-589f2df1/`の設計ドキュメント

---

## ✅ Phase 1: ラウンドロビン方式（実装済み）

**実装完了**: PR #1でマージ済み（2025-10-30）

### セットアップ＆実装

- [x] **T001** `coordinator/src/registry/mod.rs` にround_robin_indexフィールド追加
  - フィールド: `round_robin_index: AtomicUsize`
  - 初期化: `AtomicUsize::new(0)`

- [x] **T002** `coordinator/src/registry/mod.rs` にselect_agent()メソッド実装
  - 機能: オンラインノード一覧取得 → ラウンドロビンで選択
  - アルゴリズム: `index % online_agents.len()`

### テスト

- [x] **T003** `coordinator/tests/integration/proxy_test.rs` にラウンドロビン動作テスト
  - 前提: 3台のノード登録済み
  - 実行: 9回連続リクエスト送信
  - 検証: 各ノードが3リクエストずつ処理

**Phase 1 実装時間**: 約1時間（SPEC-63acef08に含む）

---

## ✅ Phase 2: メトリクスベース選択（実装完了）

**実装時間**: 約10時間

### Phase 2.1: セットアップ

- [x] **T004** [P] Cargo.toml依存関係追加: `sysinfo`（CPU/メモリ監視）
  - ✅ coordinator/Cargo.tomlにsysinfo 0.32を追加
- [x] **T005** [P] モジュール宣言: `coordinator/src/metrics/mod.rs` 作成
  - ✅ metricsモジュールとcoordinator/src/lib.rsに宣言を追加
- [x] **T006** [P] データモデル定義: `common/src/types.rs` にAgentMetrics構造体追加
  - ✅ agent_id, cpu_usage, memory_usage, active_requests, avg_response_time_ms, timestampフィールドを定義

**推定時間**: 30分 ✅ 完了

### Phase 2.2: テストファースト（TDD）

#### Contract Tests

- [x] **T007** [P] `coordinator/tests/contract/test_metrics.rs` に POST /api/agents/:id/metrics のcontract test
  - ✅ 3つのContract Test作成（成功ケース、存在しないノード、不正な値）
  - ✅ coordinator/tests/contract_tests.rs にエントリーポイント作成
  - ✅ RED状態確認完了（TDD準拠）

#### Integration Tests

- [x] **T008** `coordinator/tests/integration/test_metrics.rs` にメトリクス収集テスト
  - ✅ 3つのIntegration Test作成（収集と保存、更新、存在しないノード）
  - ✅ RED状態確認完了（TDD準拠）

- [x] **T009** `coordinator/tests/integration/loadbalancer_test.rs` に負荷ベース選択テスト
  - ✅ 3台ノード中1台高負荷時の低負荷優先選択テスト作成
  - ✅ RED状態確認完了（TDD準拠）

- [x] **T010** `coordinator/tests/integration/loadbalancer_test.rs` に全ノード高負荷時のフォールバックテスト
  - ✅ 全ノードCPU 95%時のラウンドロビンフォールバックテスト作成
  - ✅ RED状態確認完了（TDD準拠）

**推定時間**: 2時間 ✅ 完了

### Phase 2.3: コア実装

#### データモデル

- [x] **T011** [P] `common/src/types.rs` にAgentMetrics実装
  - ✅ T006で既に実装済み（agent_id, cpu_usage, memory_usage, active_requests, avg_response_time_ms, timestamp）
  - ✅ Debug, Clone, Serialize, Deserializeを実装

#### メトリクスストレージ

- [x] **T012** `coordinator/src/registry/mod.rs` にmetricsフィールド追加
  - ✅ AgentRegistryにmetrics: Arc<RwLock<HashMap<Uuid, AgentMetrics>>>を追加
  - ✅ new()とwith_storage()で空のHashMapとして初期化

- [x] **T013** `coordinator/src/registry/mod.rs` にupdate_metrics()メソッド実装
  - ✅ ノード存在確認 → メトリクス保存の実装完了

#### 負荷ベース選択ロジック

- [x] **T014** `coordinator/src/balancer/mod.rs` にselect_agent_by_metrics()メソッド実装
  - ✅ 負荷スコア計算: cpu_usage + memory_usage + (active_requests * 10)
  - ✅ 最小スコアノード選択ロジック実装

- [x] **T015** `coordinator/src/balancer/mod.rs` にフォールバックロジック実装
  - ✅ 全ノードCPU > 80%時のラウンドロビンフォールバック実装

#### メトリクス収集API

- [x] **T016** `coordinator/src/api/metrics.rs` にupdate_metrics()ハンドラー実装
  - ✅ POST /api/agents/:id/metrics エンドポイント実装
  - ✅ AgentMetrics受信 → registry.update_metrics() → 204 No Content返却
  - ✅ coordinator/src/api/mod.rsにルート登録

**推定時間**: 3時間 ✅ 完了

### Phase 2.4: 統合

- [x] **T017** `coordinator/src/main.rs` にメトリクスルート追加
  - ✅ T016でapi/mod.rsにルート登録済み（create_router()経由で有効）

- [x] **T018** `coordinator/src/api/proxy.rs` でselect_agent_by_metrics()使用
  - ✅ 環境変数LOAD_BALANCER_MODEで切り替え実装
  - ✅ "metrics": select_agent_by_metrics()使用
  - ✅ その他（デフォルト）: 既存のselect_agent()使用

- [x] **T019** 起動ログにロードバランサーモード追加
  - ✅ main.rsに`println!("Load balancer mode: {}", load_balancer_mode);`追加

**推定時間**: 1時間 ✅ 完了

### Phase 2.5: 仕上げ

#### Unit Tests

- [x] **T020** [P] `coordinator/src/registry/mod.rs` に負荷スコア計算のunit test
  - ✅ 正常ケース: 低負荷ノードが高スコア
  - ✅ エッジケース: メトリクスなしノードは最低優先度

- [x] **T021** [P] `common/src/types.rs` にAgentMetrics型のunit test
  - ✅ JSONシリアライゼーション/デシリアライゼーションテスト

#### ドキュメント

- [x] **T022** [P] README.md にメトリクスベースロードバランシングセクション追加
  - ✅ 使用例、エンドポイント説明、環境変数説明

- [x] **T023** [P] Rustdocコメント追加
  - ✅ select_agent_by_metrics() にドキュメントコメント

#### パフォーマンステスト

- [x] **T024** [P] `coordinator/benches/loadbalancer_bench.rs` にベンチマーク追加
  - ✅ 測定: select_agent_by_metrics() の実行時間
  - ✅ 目標: 1000ノードで < 10ms
  - ✅ **実測結果**（2025-11-02）:
    - 10ノード: 2.8 µs
    - 50ノード: 15.5 µs
    - 100ノード: 34.4 µs
    - 500ノード: 203.0 µs
    - **1000ノード: 0.447 ms** ← 目標10msの**22倍高速** ✅

**推定時間**: 3.5時間 ✅ 完了

---

## タスク統計

| フェーズ | タスク数 | 完了 | 未完了 | 推定時間 |
|---------|---------|------|--------|----------|
| Phase 1: Roundrobin | 3 | 3 (100%) | 0 | 1時間（完了） |
| Phase 2.1: Setup | 3 | 3 (100%) | 0 | 30分（完了） |
| Phase 2.2: Tests | 4 | 4 (100%) | 0 | 2時間（完了） |
| Phase 2.3: Core | 6 | 6 (100%) | 0 | 3時間（完了） |
| Phase 2.4: Integration | 3 | 3 (100%) | 0 | 1時間（完了） |
| Phase 2.5: Polish | 5 | 5 (100%) | 0 | 3.5時間（完了） |
| **Phase 3: GPUスペック優先度** | 4 | 4 (100%) | 0 | 1.5時間（完了） |
| **合計** | **28** | **28 (100%)** | **0** | **約12.5時間（完了）** |

---

## 実装完了確認

**Phase 1（ラウンドロビン）**:
- [x] すべてのテストが合格
- [x] TDD遵守
- [x] cargo clippy: エラー/警告ゼロ
- [x] cargo fmt --check: フォーマット準拠

**Phase 3（GPUスペック優先）**:
- [x] TDDでRED→GREENを確認（スペック優先テスト→実装→GREEN）
- [x] cargo clippy: エラー/警告ゼロ
- [x] cargo fmt --check: フォーマット準拠

---

## Phase 3: GPUスペック優先度（新規）

- [x] **T025** SPEC更新（FR-013追加）
  - GPU能力スコア優先、ビジー時フォールバック、メトリクス欠如時の扱いを仕様に追記
- [x] **T026** `coordinator/src/balancer/mod.rs` の優先度ロジック刷新
  - `agent_spec_score`/`compare_spec_*` ヘルパー追加
  - メトリクス有無に関わらず「スペック→ビジー判定→ラウンドロビン」の順序に統一
- [x] **T027** 通常ロードバランサTDD（RED→GREEN）
  - `select_agent_prefers_higher_spec_until_it_becomes_busy()` を追加し、高性能→次点フォールバックを検証
  - RED確認後、実装でGREEN化
- [x] **T028** メトリクスモードTDD（RED→GREEN）
  - `select_agent_by_metrics_prefers_higher_spec_until_busy()` を追加し、`LOAD_BALANCER_MODE=metrics` 相当の挙動を固定
  - RED確認後、実装でGREEN化

**Phase 2（メトリクスベース）**:
- [x] すべてのテストが合格
- [x] TDD遵守
- [x] パフォーマンス目標達成（< 10ms）
- [x] cargo clippy: エラー/警告ゼロ
- [x] cargo fmt --check: フォーマット準拠

---

## 関連ドキュメント

- [機能仕様書](./spec.md)
- [実装計画](./plan.md)
