# タスク分解: GPU必須エージェント登録要件

> 環境固定ルールのため、作業ブランチ／ディレクトリを変更せずに実施する。  
> TDD（RED → GREEN → REFACTOR）＋ローカル検証必須。

## Phase 3.1 Setup

- [ ] **T001** [P] 現行GPU情報収集の挙動確認
  - `agent/src/metrics.rs` で取得する GPU 情報の構造を調査し、サンプル JSON を記録
  - 参考ログを `/tmp/spec-5cd7b614/gpu-sample.json` に保存
  - コミットしない一時ファイルとする

- [ ] **T002** [P] テスト用ストレージ準備
  - `coordinator/tests/support/fixtures/agents/gpu_missing.json` を追加（GPU無しエージェントデータ）
  - `coordinator/tests/support/fixtures/agents/gpu_valid.json` を追加（GPU有エージェントデータ）

- [ ] **T003** [P] 403応答フォーマット確認
  - 既存バリデーションエラー応答のJSON形式を調査
  - 共通エラーレスポンス構造がない場合、`common/src/error.rs` に追加する案をメモ

## Phase 3.2 Tests (RED)

- [ ] **T010** [P] Contract Test: GPUあり登録成功
  - `coordinator/tests/contract/` に `test_agent_register_gpu.rs` を追加
  - GPU情報ありのpayloadで201、レスポンスにGPUフィールドが含まれることを確認

- [ ] **T011** [P] Contract Test: GPUなし登録失敗
  - 同テストで `gpu_info: []` もしくは欠損のpayloadを送り、403とエラーメッセージを確認

- [ ] **T012** Integration Test: 起動時クリーンアップ
  - `coordinator/tests/integration/registry_cleanup.rs` を追加
  - GPUなしエージェントを含むストレージを読み込み → 起動後に削除されていることを確認

- [ ] **T013** Integration Test: Dashboard API 表示
  - `/api/dashboard/agents` のレスポンスに `gpu_info` が含まれることを検証

- [ ] **T014** Agent Unit Test: GPU情報必須
  - Agent起動時に GPU 検出が失敗した場合、登録リクエストを送らないことをテスト（mock送信関数）

## Phase 3.3 Implementation (GREEN)

- [ ] **T020** 登録APIにGPUバリデーション追加
  - `common/src/types.rs` に GPU 情報の型（例: `Vec<GpuInfo>`）を追加/更新
  - `coordinator/src/api/agent.rs::register_agent` で GPU 情報の必須チェック
  - 403 応答メッセージを定義（例: `"GPU hardware is required"`）

- [ ] **T021** Agent側GPU情報送信を必須化
  - `agent/src/main.rs` 起動フローで、GPU情報取得失敗時は登録処理をスキップし警告ログ
  - 登録 payload に `gpu_info` を含める

- [ ] **T022** 起動時クリーンアップ実装
  - `coordinator/src/registry/mod.rs` または `coordinator/src/main.rs` でストレージを走査
  - `gpu_info` が空/None のエントリを削除し、削除件数を info ログに出す

- [ ] **T023** Dashboard APIレスポンス拡張
  - `coordinator/src/api/dashboard.rs` / `app.js` で GPU 情報のレスポンスを整合させる
  - 新しいフィールドが UI に正しく表示されるよう調整

## Phase 3.4 Integration (REFACTOR)

- [ ] **T030** エラーハンドリング統一
  - API レイヤーでバリデーションエラーを共通フォーマットに揃える
  - 既存のエラーメッセージとの整合を確認

- [ ] **T031** ログ整備
  - GPU未搭載エージェント拒否時・クリーンアップ時のログメッセージを分かりやすく

- [ ] **T032** Web UI のUX調整
  - GPU情報が1枚の場合の表示、複数枚の場合のフォーマット調整
  - 非同期更新時のちらつきがないか確認

## Phase 3.5 Polish

- [ ] **T040** ドキュメント更新
  - `README.md` / `README.ja.md` に GPU要件を追記
  - `CLAUDE.md` に GPU 登録ポリシーを追加（必要なら）

- [ ] **T041** Quickstart更新
  - `/specs/SPEC-5cd7b614/quickstart.md` を新規作成し、登録手順／ダッシュボード確認手順を記述

- [ ] **T042** ローカル検証実行
  - `cargo fmt --check` → `cargo clippy -- -D warnings` → `cargo test`
  - `.specify/scripts/checks/check-tasks.sh specs/SPEC-5cd7b614/tasks.md`（都度更新）
  - `npx markdownlint-cli2` でドキュメントLint

- [ ] **T043** Final Review
  - SPEC/PLAN/TASKS/QUICKSTART の整合を確認
  - 未チェックタスクが残っていないことを確認し、PR準備

---

### チェックコマンド

```bash
rg -n "\- \[ \]" specs/SPEC-5cd7b614
```

タスク完了時は必ず `- [x]` へ更新し、ローカル検証を再実行してからコミットすること。
