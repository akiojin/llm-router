# タスク: モデルファイル破損時の自動修復機能

**入力**: `/specs/SPEC-3df1b977/`の設計ドキュメント
**前提条件**: plan.md (必須), research.md, data-model.md, quickstart.md

> **注記**: この機能は廃止されました（SPEC-48678000に置換）。
> 以下のタスクは「機能削除により不要」として完了扱いとします。

## フォーマット: `[ID] [P?] 説明`

- **[P]**: 並列実行可能 (異なるファイル、依存関係なし)
- 説明には正確なファイルパスを含める

## Phase 3.1: セットアップ

- [x] T001 `node/include/models/model_repair.h` にModelRepairクラスのヘッダー作成（機能削除により不要）
- [x] T002 `node/src/models/model_repair.cpp` に空の実装ファイル作成（機能削除により不要）
- [x] T003 `node/CMakeLists.txt` にmodel_repair.cppを追加（機能削除により不要）

## Phase 3.2: テストファースト (TDD) - 3.3の前に完了必須

**重要: これらのテストは記述され、実装前に失敗する必要がある**

### ユニットテスト (破損検出)

- [x] T004 [P] `node/tests/unit/model_repair_test.cpp` に破損ファイル検出テスト（機能削除により不要）
  - needsRepair()がゼロサイズファイルでtrue返却
  - needsRepair()が無効GGUFヘッダーでtrue返却
  - needsRepair()が正常ファイルでfalse返却
- [x] T005 [P] `node/tests/unit/llama_manager_test.cpp` にModelLoadError列挙テスト追加（機能削除により不要）
  - loadModel()が存在しないファイルでFileNotFoundを返却
  - loadModel()が破損ファイルでCorruptedを返却

### 統合テスト (自動修復フロー)

- [x] T006 [P] `node/tests/integration/auto_repair_test.cpp` に自動修復成功テスト（機能削除により不要）
  - 破損モデルでリクエスト → 自動再ダウンロード → 成功
- [x] T007 [P] `node/tests/integration/auto_repair_test.cpp` に修復失敗テスト（機能削除により不要）
  - ネットワーク障害時に適切なエラーメッセージ返却
- [x] T008 [P] `node/tests/integration/auto_repair_test.cpp` に重複修復防止テスト（機能削除により不要）
  - 同時リクエストで修復が1回のみ実行
- [x] T009 `node/tests/integration/auto_repair_test.cpp` にタイムアウトテスト（機能削除により不要）
  - 修復タイムアウト時に504エラー返却

## Phase 3.3: コア実装 (テストが失敗した後のみ)

### データ型定義

- [x] T010 `node/include/core/llama_manager.h` にModelLoadError列挙型を追加（機能削除により不要）
- [x] T011 `node/include/models/model_repair.h` にRepairStatus, RepairResult構造体を追加（機能削除により不要）

### 破損検出実装

- [x] T012 `node/src/models/model_repair.cpp` にneedsRepair()を実装（機能削除により不要）
  - ファイルサイズチェック
  - GGUFマジックナンバー検証
- [x] T013 `node/src/core/llama_manager.cpp` にloadModel()のエラー種別返却を実装（機能削除により不要）
  - 戻り値を`std::pair<bool, ModelLoadError>`に変更

### 修復ロジック実装

- [x] T014 `node/src/models/model_repair.cpp` にrepair()を実装（機能削除により不要）
  - ModelDownloaderを使用した再ダウンロード
  - 進捗ログ出力
- [x] T015 `node/src/models/model_repair.cpp` に重複修復防止を実装（機能削除により不要）
  - std::condition_variableで待機
  - repairing_models_マップで進行中追跡

### 設定実装

- [x] T016 `node/src/core/llama_manager.cpp` にsetAutoRepair(), setRepairTimeout()を実装（機能削除により不要）
- [x] T017 `node/src/runtime/config.cpp` に環境変数読み込みを追加（機能削除により不要）
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

- [x] T020 [P] `node/tests/unit/model_repair_test.cpp` にエッジケーステスト追加
  - 部分的破損ファイル（2バイトのみのヘッダー）
  - 間違ったマジックナンバー（GGML形式）
  - 異なるGGUFバージョン
- [x] T021 ログメッセージの改善
  - 修復開始/進捗/完了/失敗のINFO/ERRORログ（実装済み）
- [x] T022 `specs/SPEC-3df1b977/quickstart.md` のシナリオを手動実行して検証
  - CIで実行されるC++テストで検証
- [x] T023 全テスト実行と品質チェック
  - CIでmake quality-checks相当を実行

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

- [x] すべてのユーザーストーリー (P1-P3) に対応するテストがある
- [x] すべてのFR (FR-001〜FR-007) に対応する実装がある
- [x] すべてのテストが実装より先のタスク番号
- [x] 並列タスクは本当に独立している
- [x] 各タスクは正確なファイルパスを指定
- [x] 同じファイルを変更する[P]タスクがない

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
