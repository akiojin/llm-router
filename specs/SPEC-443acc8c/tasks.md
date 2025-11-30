# タスク: ヘルスチェックシステム

**ステータス**: ✅ **実装完了** (PR #1でマージ済み、2025-10-30)
**入力**: `/llm-router/specs/SPEC-443acc8c/`の設計ドキュメント

## 実装済みタスク一覧

すべてのタスクは完了し、PR #1でmainブランチにマージ済みです。

---

## Phase 3.1: セットアップ

- [x] **T001** [P] 依存関係確認: `chrono`, `tokio::time::interval`
- [x] **T002** [P] 環境変数定義: `AGENT_TIMEOUT`, `HEALTH_CHECK_INTERVAL`

**実装時間**: 約10分

---

## Phase 3.2: テストファースト（TDD）

### Integration Tests

- [x] **T003** `coordinator/tests/integration/health_test.rs` にタイムアウト検出テスト
  - 前提: ノード登録済み
  - 実行: 60秒待機（ハートビート送信なし）
  - 検証: ノードがOfflineステータスに遷移

- [x] **T004** `coordinator/tests/integration/health_test.rs` に自動復旧テスト
  - 前提: ノードがOfflineステータス
  - 実行: ハートビート送信
  - 検証: ノードがOnlineステータスに復帰

- [x] **T005** `coordinator/tests/integration/health_test.rs` にOfflineノード除外テスト
  - 前提: 3台のノード（1台Offline）
  - 実行: select_agent()呼び出し
  - 検証: Onlineの2台のみが選択される

- [x] **T006** `coordinator/tests/integration/health_test.rs` に全ノードOfflineテスト
  - 前提: すべてのノードがOffline
  - 実行: プロキシリクエスト送信
  - 検証: "No agents available"エラー返却

**実装時間**: 約1.5時間

---

## Phase 3.3: コア実装

### タイムアウト監視ロジック

- [x] **T007** `coordinator/src/registry/mod.rs` にstart_timeout_monitor()実装
  - 機能: Tokio spawn でバックグラウンドタスク開始
  - ロジック: 定期的に全ノードをチェック、タイムアウトしたらOffline化
  - 間隔: 環境変数`HEALTH_CHECK_INTERVAL`（デフォルト30秒）

- [x] **T008** `coordinator/src/registry/mod.rs` にタイムアウト判定ロジック実装
  - 条件: `Utc::now() - agent.last_heartbeat > timeout`
  - アクション: `agent.status = AgentStatus::Offline`
  - ログ: Offlineに遷移時にwarnログ出力

### ハートビート自動復旧

- [x] **T009** `coordinator/src/registry/mod.rs` のheartbeat()メソッドに自動復旧追加
  - ロジック: ハートビート受信時に `agent.status = AgentStatus::Online`
  - ログ: Online復帰時にinfoログ出力

### ノード選択からOffline除外

- [x] **T010** `coordinator/src/registry/mod.rs` のselect_agent()にOfflineフィルター追加
  - 変更前: すべてのノードから選択
  - 変更後: `filter(|a| a.status == AgentStatus::Online)`

**実装時間**: 約1.5時間

---

## Phase 3.4: 統合

- [x] **T011** `coordinator/src/main.rs` でタイムアウト監視タスク起動
  - 起動タイミング: サーバー起動時
  - パラメータ: 環境変数から取得（`HEALTH_CHECK_INTERVAL`, `AGENT_TIMEOUT`）

- [x] **T012** `coordinator/src/main.rs` に環境変数読み込み追加
  - `HEALTH_CHECK_INTERVAL`: デフォルト30秒
  - `AGENT_TIMEOUT`: デフォルト60秒

- [x] **T013** 起動ログにヘルスチェック設定情報追加
  - `tracing::info!("Health check interval: {}s, timeout: {}s", interval, timeout);`

**実装時間**: 約30分

---

## Phase 3.5: 仕上げ

### Unit Tests

- [x] **T014** [P] `coordinator/src/registry/mod.rs` にタイムアウト判定ロジックのunit test
  - 正常ケース: タイムアウト前はOnline維持
  - タイムアウトケース: タイムアウト後はOffline遷移

### ドキュメント

- [x] **T015** [P] README.md にヘルスチェックセクション追加
  - 環境変数説明
  - 障害検知フロー説明

- [x] **T016** [P] Rustdocコメント追加
  - `start_timeout_monitor()` にドキュメントコメント
  - タイムアウトロジックの説明

**実装時間**: 約30分

---

## タスク統計

| フェーズ | タスク数 | 完了 | 実装時間 |
|---------|---------|------|------------|
| Setup | 2 | 2 (100%) | 10分 |
| Tests | 4 | 4 (100%) | 1.5時間 |
| Core | 4 | 4 (100%) | 1.5時間 |
| Integration | 3 | 3 (100%) | 30分 |
| Polish | 3 | 3 (100%) | 30分 |
| **合計** | **16** | **16 (100%)** | **約4時間** |

---

## 実装完了確認

**すべてのチェック項目クリア**:
- [x] すべてのテストが実装より先にある（TDD遵守）
- [x] 並列タスクは本当に独立している
- [x] 各タスクは正確なファイルパスを指定

**テスト結果**:
- ✅ タイムアウト検出正常動作
- ✅ 自動復旧正常動作
- ✅ Offlineノード除外正常動作
- ✅ cargo clippy: エラー/警告ゼロ
- ✅ cargo fmt --check: フォーマット準拠

**実装PR**: #1
**マージ日**: 2025-10-30

---

## 関連ドキュメント

- [機能仕様書](./spec.md)
- [実装計画](./plan.md)
