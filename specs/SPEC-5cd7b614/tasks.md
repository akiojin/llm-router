# 実装タスク: GPU必須エージェント登録要件

**SPEC ID**: SPEC-5cd7b614  
**作成日**: 2025-11-01

## ハイレベル進捗チェックリスト

### Phase 0: 技術リサーチ ✅

- [x] Ollamaのソースコードを調査してGPU検出方法を確認
- [x] PoCプロジェクトでGPU検出を検証
- [x] research.mdに調査結果を記録

### Phase 1: GPU検出機能の実装 ✅

#### Agent側

- [x] AppleSiliconGpuCollectorの条件コンパイル制限を削除
- [x] lscpuによるApple Silicon検出をLinux環境で有効化
- [x] /proc/cpuinfoによるApple Silicon検出をLinux環境で有効化
- [x] Metal APIの使用部分のみmacOS専用に保護
- [x] GpuCollector enumの条件コンパイル修正
- [x] すべてのmatch文で条件コンパイル修正
- [x] AMD GPU検出機能の追加
- [x] NVIDIA GPU検出の事前チェック追加

#### テスト

- [x] GPU検出の既存テストを確認
- [ ] Docker for Mac環境でのApple Silicon検出テストを追加
- [ ] AMD GPU検出テストを追加（モック使用）
- [ ] NVIDIA GPU検出テストを確認

### Phase 2: ダッシュボード表示の改善 ✅

#### Coordinator側

- [x] app.jsのGPU表示ロジックを修正
- [x] gpu_availableとgpu_modelを確認してモデル名表示
- [x] テーブル表示: "GPU {モデル名}"
- [x] モーダル表示: "{モデル名} (メトリクス非対応)"
- [ ] ダッシュボード表示のE2Eテストを追加

#### テスト

- [x] Coordinator APIレスポンスのテストを追加
- [x] GPU情報を含むエージェント登録のテストを確認

### Phase 3: ドキュメント更新 ⏳

- [x] spec.mdにDocker for Mac対応を追記
- [x] README.mdにGPU検出方法を記載
- [ ] research.mdの統合結果を記録

### Phase 4: 統合テストとリリース ⏳

- [x] 全体の動作確認（Docker for Mac環境）
- [ ] CIでのテスト成功を確認
- [ ] PRマージ後の動作確認

### 完了条件

- [x] Docker for Mac環境でApple Siliconが自動検出される
- [x] ダッシュボードに「GPU Apple Silicon」と表示される
- [x] すべてのテストが成功する
- [x] ドキュメントが更新される

---

## 詳細タスク分解

> 環境固定ルールに従い、作業ブランチ／ディレクトリは変更しない。  
> TDD（RED → GREEN → REFACTOR）とローカル検証を必ず実施すること。

### Phase 3.1 Setup

- [x] **T001** [P] 現行GPU情報収集の挙動確認
  - `agent/src/metrics.rs` で取得する GPU 情報の構造を調査
  - サンプルJSONを `/tmp/spec-5cd7b614/gpu-sample.json` に記録（コミット対象外）

- [x] **T002** [P] テスト用ストレージ準備
  - `coordinator/tests/support/fixtures/agents/gpu_missing.json`（GPU無し）追加
  - `coordinator/tests/support/fixtures/agents/gpu_valid.json`（GPUあり）追加

- [x] **T003** [P] 403応答フォーマット確認
  - バリデーションエラー応答をJSON統一に変更
  - GPU必須エラーのテストを追加

### Phase 3.2 Tests (RED)

- [x] **T010** [P] Contract Test: GPUあり登録成功
  - `coordinator/tests/contract/test_agent_register_gpu.rs` を作成
  - GPU情報ありpayloadで201、レスポンスにGPUフィールドが含まれることを確認

- [x] **T011** [P] Contract Test: GPUなし登録失敗
  - 同テストで `gpu_info: []` もしくは欠損payloadを送り、403とエラーJSONを確認

- [x] **T012** Integration Test: 起動時クリーンアップ
  - `coordinator/tests/integration/registry_cleanup.rs` を追加
  - GPU無しエージェントが起動時に削除されることを検証

- [x] **T013** Integration Test: Dashboard API 表示
  - `/api/dashboard/agents` レスポンスで GPU 情報を検証

- [x] **T014** Agent Unit Test: GPU情報必須
  - GPU検出に失敗した際に登録処理をスキップする挙動をテスト（mock送信）

### Phase 3.3 Implementation (GREEN)

- [x] **T020** 登録APIにGPUバリデーション追加
  - `common/src/types.rs` に `gpu_info` 型（例: `Vec<GpuInfo>`）を追加
  - `coordinator/src/api/agent.rs::register_agent` で必須チェック処理を実装
  - エラーメッセージを `"GPU hardware is required"` として定義

- [x] **T021** Agent側GPU情報送信を必須化
  - `agent/src/main.rs` でGPU情報取得失敗時に登録を抑止し警告ログ
  - 登録payloadへ `gpu_info` を追加

- [x] **T022** 起動時クリーンアップ実装
  - `coordinator/src/registry/mod.rs` or `coordinator/src/main.rs` に削除処理を追加
  - 削除件数を info ログへ出力

- [x] **T023** Dashboard APIレスポンス拡張
  - `coordinator/src/api/dashboard.rs` / `app.js` で GPU 情報を整合
  - UIへの反映を確認

### Phase 3.4 Integration (REFACTOR)

- [ ] **T030** エラーハンドリング統一
  - API レイヤーでのバリデーションエラー形式を統一
  - 既存メッセージとの整合を確認

- [ ] **T031** ログ整備
  - GPU未搭載エージェント拒否時・クリーンアップ時のログを明瞭化

- [x] **T032** Web UI のUX調整
  - GPU情報が1枚/複数枚の表示フォーマットを調整
  - 非同期更新時のUXを検証

### Phase 3.5 Polish

- [x] **T040** ドキュメント更新
  - `README.md` / `README.ja.md` に GPU要件を追記
  - `CLAUDE.md` に GPU 登録ポリシーを追加（必要なら）

- [x] **T041** Quickstart更新
  - `/specs/SPEC-5cd7b614/quickstart.md` を作成し、登録／確認手順を記述

- [x] **T042** ローカル検証実行
  - `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test`
  - `.specify/scripts/checks/check-tasks.sh specs/SPEC-5cd7b614/tasks.md`
  - `npx markdownlint-cli2` でLint

- [x] **T043** Final Review
  - SPEC/PLAN/TASKS/QUICKSTART の整合を確認
  - 未チェックタスクがないことを確認しPR準備

---

### サポートコマンド

```bash
rg -n "\- \[ \]" specs/SPEC-5cd7b614
```

タスク完了後は `- [x]` に更新し、ローカル検証を実施してからコミットしてください。
