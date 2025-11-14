# タスク: コーディネーター主導のモデル自動配布機能

**機能ID**: `SPEC-8ae67d67`
**入力**: `/specs/SPEC-8ae67d67/`の設計ドキュメント
**前提条件**: plan.md, research.md

## 実行フロー

```
1. ✅ 機能ディレクトリからplan.mdを読み込み
2. ✅ 設計ドキュメント（research.md）を読み込み
3. → カテゴリ別にタスクを生成
4. → タスクルールを適用（TDD、並列実行）
5. → タスクを順次番号付け
6. → 依存関係グラフを生成
7. → 実行準備完了
```

## パス規約

- **Coordinator**: `coordinator/src/`, `coordinator/tests/`
- **Agent**: `agent/src/`, `agent/tests/`
- **ダッシュボード**: `coordinator/src/web/static/`

---

## Phase 3.1: セットアップ (Setup)

- [x] **T0*01** [P] データモデル定義: `coordinator/src/registry/models.rs` に `ModelInfo`, `InstalledModel`, `DownloadTask` 構造体を定義
- [x] **T0*02** [P] タスク管理モジュール: `coordinator/src/tasks/mod.rs` に `DownloadTaskManager` 構造体を定義
- [x] **T0*03** [P] Ollama公式API通信モジュール: `coordinator/src/ollama/mod.rs` と `coordinator/src/ollama/client.rs` を作成（構造のみ、実装は後）

---

## Phase 3.2: テストファースト (TDD) ⚠️ Phase 3.3の前に完了必須

**重要: これらのテストは記述され、実装前に失敗する必要がある (RED)**

### Contract Tests

- [x] **T0*04** [P] Contract test: `coordinator/tests/contract/models_api_test.rs` に `test_get_available_models_contract()` を作成
- [x] **T0*05** [P] Contract test: `coordinator/tests/contract/models_api_test.rs` に `test_distribute_models_contract()` を作成
- [x] **T0*06** [P] Contract test: `coordinator/tests/contract/models_api_test.rs` に `test_get_agent_models_contract()` を作成
- [x] **T0*07** [P] Contract test: `coordinator/tests/contract/models_api_test.rs` に `test_pull_model_contract()` を作成
- [x] **T0*08** [P] Contract test: `coordinator/tests/contract/models_api_test.rs` に `test_get_task_progress_contract()` を作成

### Integration Tests - ユーザーストーリー1（自動配布）

- [x] **T0*09** [P] Integration test: `coordinator/tests/integration/auto_download_test.rs` に `test_auto_download_on_registration_16gb_gpu()` を作成
- [x] **T0*10** [P] Integration test: `coordinator/tests/integration/auto_download_test.rs` に `test_auto_download_on_registration_8gb_gpu()` を作成
- [x] **T0*11** [P] Integration test: `coordinator/tests/integration/auto_download_test.rs` に `test_auto_download_on_registration_4_5gb_gpu()` を作成
- [x] **T0*12** [P] Integration test: `coordinator/tests/integration/auto_download_test.rs` に `test_auto_download_on_registration_small_gpu()` を作成
- [x] **T0*13** [P] Integration test: `coordinator/tests/integration/auto_download_test.rs` に `test_progress_display_during_download()` を作成

### Integration Tests - ユーザーストーリー2（手動配布）

- [x] **T0*14** [P] Integration test: `coordinator/tests/integration/manual_distribution_test.rs` に `test_manual_distribution_to_specific_agent()` を作成
- [x] **T0*15** [P] Integration test: `coordinator/tests/integration/manual_distribution_test.rs` に `test_bulk_distribution_to_all_agents()` を作成
- [x] **T0*16** [P] Integration test: `coordinator/tests/integration/manual_distribution_test.rs` に `test_progress_tracking_multiple_agents()` を作成
- [x] **T0*17** [P] Integration test: `coordinator/tests/integration/manual_distribution_test.rs` に `test_offline_agent_error_handling()` を作成

### Integration Tests - ユーザーストーリー3（可視化）

- [x] **T0*18** [P] Integration test: `coordinator/tests/integration/model_info_test.rs` に `test_list_available_models_from_ollama_library()` を作成
- [x] **T0*19** [P] Integration test: `coordinator/tests/integration/model_info_test.rs` に `test_list_installed_models_on_agent()` を作成
- [x] **T0*20** [P] Integration test: `coordinator/tests/integration/model_info_test.rs` に `test_model_matrix_view_multiple_agents()` を作成

### Unit Tests

- [x] **T0*21** [P] Unit test: `coordinator/tests/unit/gpu_model_selector_test.rs` に `test_select_model_by_gpu_memory_16gb()` など4ケースを作成
- [x] **T0*22** [P] Unit test: `coordinator/tests/unit/model_repository_test.rs` に `test_task_lifecycle()` を作成

---

## Phase 3.3: コア実装 (テストが失敗した後のみ) (Core Implementation - GREEN)

### データモデル実装

- [x] **T0*23** データモデル実装: `coordinator/src/registry/models.rs` の構造体にメソッド実装（`new()`, `to_json()` など）

### タスク管理実装

- [x] **T0*24** タスク管理実装: `coordinator/src/tasks/mod.rs` の `DownloadTaskManager` 実装
  - `create_task()`, `update_progress()`, `get_task()`, `list_tasks()` メソッド
  - `Arc<Mutex<HashMap<Uuid, DownloadTask>>>` でスレッドセーフな状態管理

### Ollama公式API通信実装

- [x] **T0*25** Ollama通信実装: `coordinator/src/ollama/client.rs` に `OllamaClient` 実装
  - エージェント経由でモデル一覧取得（`GET /api/tags`）
  - 事前定義モデルリスト管理

### GPU能力ベースモデル選択

- [x] **T0*26** [P] GPU判定ロジック: `coordinator/src/models/gpu_selector.rs` に `select_model_by_gpu_memory()` 関数を実装

### モデル管理API実装

- [x] **T0*27** API実装: `coordinator/src/api/models.rs` に `get_available_models` ハンドラーを実装（`GET /api/models/available`）
- [x] **T0*28** API実装: `coordinator/src/api/models.rs` に `distribute_models` ハンドラーを実装（`POST /api/models/distribute`）
- [x] **T0*29** API実装: `coordinator/src/api/models.rs` に `get_agent_models` ハンドラーを実装（`GET /api/agents/{agent_id}/models`）
- [x] **T0*30** API実装: `coordinator/src/api/models.rs` に `pull_model_to_agent` ハンドラーを実装（`POST /api/agents/{agent_id}/models/pull`）
- [x] **T0*31** API実装: `coordinator/src/api/models.rs` に `get_task_progress` ハンドラーを実装（`GET /api/tasks/{task_id}`）

### エージェント登録時自動配布

- [x] **T0*32** 自動配布ロジック: `coordinator/src/api/agent.rs` の `register_agent` ハンドラーを拡張
  - 登録完了後に `select_model_by_gpu_memory()` を呼び出し
  - バックグラウンドで `distribute_models` を実行（`tokio::spawn`）

### エージェント側モデルプル拡張

- [x] **T0*33** [P] エージェント側API: `agent/src/api/mod.rs` と `agent/src/api/models.rs` を作成
  - `POST /pull` エンドポイント（コーディネーターからの指示を受ける）
  - 既存の `agent/src/ollama.rs` の `pull_model()` を呼び出し

### 進捗報告機能

- [x] **T0*34** 進捗報告: `agent/src/api/models.rs` で `pull_model()` の進捗をコーディネーターに送信
  - ストリーミングレスポンスをパースして進捗計算
  - `POST /api/tasks/{task_id}/progress` でコーディネーターに送信

---

## Phase 3.4: 統合 (Integration)

### ルーター統合

- [x] **T0*35** ルーター統合: `coordinator/src/main.rs` のaxumルーターに新しいエンドポイントを追加
  - `/api/models/*` ルート
  - `/api/tasks/*` ルート
  - `/api/agents/:id/models/*` ルート

### ダッシュボードUI拡張

- [x] **T036** UI: `coordinator/src/web/static/index.html` に「モデル管理」タブを追加
- [x] **T037** UI: `coordinator/src/web/static/models.js` を作成
  - `fetchAvailableModels()` 関数
  - `distributeModel()` 関数
  - `fetchAgentModels()` 関数
  - `monitorProgress()` 関数（5秒ポーリング）
- [x] **T038** UI: `coordinator/src/web/static/app.js` を拡張
  - エージェント詳細モーダルに「インストール済みモデル」セクション追加
  - ダウンロード進捗表示（HTML5 `<progress>` タグ）

### エラーハンドリング

- [x] **T039** エラーハンドリング: 各APIハンドラーにエラーレスポンス実装
  - オフラインエージェントへの配布試行: 503 Service Unavailable
  - ディスク容量不足: 507 Insufficient Storage
  - モデル名不正: 400 Bad Request

---

## Phase 3.5: 仕上げ (Polish)

### ロギング強化

- [x] **T040** [P] ロギング: すべてのモデル管理操作に構造化ログを追加
  - `tracing::info!`, `tracing::error!` を使用
  - ダウンロード開始/完了/失敗をログ記録

### ドキュメント更新

- [x] **T041** [P] ドキュメント: `specs/SPEC-8ae67d67/quickstart.md` を作成（3シナリオ）
- [x] **T042** [P] ドキュメント: `CLAUDE.md` にモデル自動配布機能のセクションを追加
- [x] **T043** [P] ドキュメント: `README.md` に新機能の説明を追加

### ローカル品質チェック

- [x] **T044** 品質チェック: `cargo fmt --check` を実行して合格確認
- [x] **T045** 品質チェック: `cargo clippy -- -D warnings` を実行して合格確認
- [x] **T046** 品質チェック: `cargo test` を実行してすべてのテスト合格確認
- [x] **T047** 品質チェック: `npx markdownlint-cli '**/*.md' --ignore node_modules --ignore .git` を実行して合格確認
- [x] **T048** 品質チェック: `.specify/scripts/checks/check-tasks.sh` を実行して合格確認

---

## 依存関係

### フェーズ依存関係
- **Setup (T001-T003)** → **Tests (T004-T022)** → **Core (T023-T034)** → **Integration (T035-T039)** → **Polish (T040-T048)**

### タスク依存関係
- T001 (データモデル定義) が T004-T022 (テスト), T023 (実装) をブロック
- T002 (タスク管理モジュール) が T024 (実装) をブロック
- T004-T022 (すべてのテスト) が T023-T034 (実装) より先に完了必須 (TDD)
- T023-T034 (コア実装) が T035-T039 (統合) をブロック
- T035 (ルーター統合) が T036-T038 (UI) をブロック
- T041-T043 (ドキュメント) は T044-T048 (品質チェック) より先
- T044-T047 (個別チェック) が T048 (総合チェック) より先

---

## 並列実行例

### Setup Phase（すべて並列実行可能）
```
Task T001, T002, T003 を並列実行
```

### Test Phase（Contract Tests - すべて並列実行可能）
```
Task T004, T005, T006, T007, T008 を並列実行
```

### Test Phase（Integration Tests - ストーリー別に並列実行可能）
```
# ユーザーストーリー1
Task T009, T010, T011, T012, T013 を並列実行

# ユーザーストーリー2
Task T014, T015, T016, T017 を並列実行

# ユーザーストーリー3
Task T018, T019, T020 を並列実行

# Unit Tests
Task T021, T022 を並列実行
```

### Core Phase（一部並列実行可能）
```
# 独立モジュール
Task T026 (GPU判定) を単独実行
Task T033 (エージェント側API) を単独実行

# モデル管理API（T027-T031は順次または慎重に並列化）
Task T027, T028, T029, T030, T031 を実装
```

### Polish Phase（ドキュメント - 並列実行可能）
```
Task T040, T041, T042, T043 を並列実行
```

---

## 注意事項

- **[P] タスク** = 異なるファイル、依存関係なし → 並列実行可能
- **TDD厳守**: T004-T022（テスト）はすべてREDフェーズ確認後、T023-T034（実装）でGREENに
- **各タスク後にコミット**: Conventional Commits形式で日本語メッセージ
- **同じファイルは順次実行**: 例えば `coordinator/src/api/models.rs` への変更（T027-T031）は慎重に
- **品質チェック必須**: T044-T048はコミット前に必ず実行・合格

---

## 検証チェックリスト

- [x] すべてのAPI契約（5エンドポイント）に対応するContract testがある（T004-T008）
- [x] すべてのデータモデル（3構造体）にモデルタスクがある（T001, T023）
- [x] すべてのテスト（T004-T022）が実装（T023-T034）より先にある
- [x] 並列タスク [P] は本当に独立している（異なるファイル）
- [x] 各タスクは正確なファイルパスを指定
- [x] 同じファイルを変更する [P] タスクがない（T027-T031は [P] なし）

---

**総タスク数**: 48タスク
**推定並列実行**: Setup 3並列, Tests 20並列, Core 一部並列, Polish 4並列
**TDD遵守**: Phase 3.2（テスト）→ Phase 3.3（実装）の順序厳守

---

*このタスクリストは plan.md と research.md に基づいて生成されました。*
*憲章準拠: TDD、シンプルさ、テストファースト原則を遵守*
