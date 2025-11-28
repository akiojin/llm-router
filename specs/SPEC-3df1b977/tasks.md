# タスク: モデルファイル破損時の自動修復機能

**入力**: `/specs/SPEC-3df1b977/`の設計ドキュメント
**前提条件**: plan.md (必須), research.md, data-model.md, quickstart.md

## フォーマット: `[ID] [P?] 説明`

- **[P]**: 並列実行可能 (異なるファイル、依存関係なし)
- 説明には正確なファイルパスを含める

## Phase 3.1: セットアップ

- [ ] T001 `node/include/models/model_repair.h` にModelRepairクラスのヘッダー作成
- [ ] T002 `node/src/models/model_repair.cpp` に空の実装ファイル作成
- [ ] T003 `node/CMakeLists.txt` にmodel_repair.cppを追加

## Phase 3.2: テストファースト (TDD) - 3.3の前に完了必須

**重要: これらのテストは記述され、実装前に失敗する必要がある**

### ユニットテスト (破損検出)

- [ ] T004 [P] `node/tests/unit/model_repair_test.cpp` に破損ファイル検出テスト
  - needsRepair()がゼロサイズファイルでtrue返却
  - needsRepair()が無効GGUFヘッダーでtrue返却
  - needsRepair()が正常ファイルでfalse返却
- [ ] T005 [P] `node/tests/unit/llama_manager_test.cpp` にModelLoadError列挙テスト追加
  - loadModel()が存在しないファイルでFileNotFoundを返却
  - loadModel()が破損ファイルでCorruptedを返却

### 統合テスト (自動修復フロー)

- [ ] T006 [P] `node/tests/integration/auto_repair_test.cpp` に自動修復成功テスト
  - 破損モデルでリクエスト → 自動再ダウンロード → 成功
- [ ] T007 [P] `node/tests/integration/auto_repair_test.cpp` に修復失敗テスト
  - ネットワーク障害時に適切なエラーメッセージ返却
- [ ] T008 [P] `node/tests/integration/auto_repair_test.cpp` に重複修復防止テスト
  - 同時リクエストで修復が1回のみ実行
- [ ] T009 `node/tests/integration/auto_repair_test.cpp` にタイムアウトテスト
  - 修復タイムアウト時に504エラー返却

## Phase 3.3: コア実装 (テストが失敗した後のみ)

### データ型定義

- [ ] T010 `node/include/core/llama_manager.h` にModelLoadError列挙型を追加
- [ ] T011 `node/include/models/model_repair.h` にRepairStatus, RepairResult構造体を追加

### 破損検出実装

- [ ] T012 `node/src/models/model_repair.cpp` にneedsRepair()を実装
  - ファイルサイズチェック
  - GGUFマジックナンバー検証
- [ ] T013 `node/src/core/llama_manager.cpp` にloadModel()のエラー種別返却を実装
  - 戻り値を`std::pair<bool, ModelLoadError>`に変更

### 修復ロジック実装

- [ ] T014 `node/src/models/model_repair.cpp` にrepair()を実装
  - ModelDownloaderを使用した再ダウンロード
  - 進捗ログ出力
- [ ] T015 `node/src/models/model_repair.cpp` に重複修復防止を実装
  - std::condition_variableで待機
  - repairing_models_マップで進行中追跡

### 設定実装

- [ ] T016 `node/src/core/llama_manager.cpp` にsetAutoRepair(), setRepairTimeout()を実装
- [ ] T017 `node/src/runtime/config.cpp` に環境変数読み込みを追加
  - LLM_AUTO_REPAIR
  - LLM_REPAIR_TIMEOUT_SECS

## Phase 3.4: 統合

- [x] T018 `node/src/core/inference_engine.cpp` に自動修復フローを統合
  - loadModel失敗時にModelRepair::repair()を呼び出し
  - 修復成功後に再ロード試行
- [x] T019 `node/src/api/openai_endpoints.cpp` にHTTPステータスコード修正
  - 202: 修復中 (Accepted)
  - ModelRepairingException例外を追加

## Phase 3.5: 仕上げ

- [ ] T020 [P] `node/tests/unit/model_repair_test.cpp` にエッジケーステスト追加
  - 部分的破損ファイル
  - 権限エラー
- [ ] T021 ログメッセージの改善
  - 修復開始/進捗/完了/失敗のINFO/ERRORログ
- [ ] T022 `specs/SPEC-3df1b977/quickstart.md` のシナリオを手動実行して検証
- [ ] T023 全テスト実行と品質チェック
  - `make quality-checks` 合格確認

## 依存関係

```
T001-T003 (Setup) → T004-T009 (Tests) → T010-T017 (Core) → T018-T019 (Integration) → T020-T023 (Polish)

詳細:
- T004, T005: T010に依存 (ModelLoadError定義)
- T006-T009: T001-T003に依存 (ファイル存在)
- T012: T011に依存 (RepairStatus定義)
- T013: T010に依存 (ModelLoadError定義)
- T014-T015: T012に依存 (needsRepair実装)
- T018: T013, T14, T15に依存
- T019: T018に依存
```

## 並列実行例

```bash
# Phase 3.2 のテストを並列実行:
# T004, T005, T006, T007, T008 は異なるファイル/テストなので並列可能

# Phase 3.3 のT010, T011 は異なるファイルなので並列可能
```

## 検証チェックリスト

*ゲート: 完了前にチェック*

- [ ] すべてのユーザーストーリー (P1-P3) に対応するテストがある
- [ ] すべてのFR (FR-001〜FR-007) に対応する実装がある
- [ ] すべてのテストが実装より先のタスク番号
- [ ] 並列タスクは本当に独立している
- [ ] 各タスクは正確なファイルパスを指定
- [ ] 同じファイルを変更する[P]タスクがない

## FR対応表

| FR | タスク | 説明 |
|----|-------|------|
| FR-001 | T012 | モデル読み込み時の破損検出 |
| FR-002 | T014 | 自動再ダウンロード開始 |
| FR-003 | T018 | 再読み込み試行 |
| FR-004 | T019 | 具体的エラーメッセージ |
| FR-005 | T009, T15 | 修復中リクエストの待機 |
| FR-006 | T008, T15 | 重複修復防止 |
| FR-007 | T021 | 進捗ログ記録 |
