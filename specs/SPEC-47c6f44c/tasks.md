# タスク: 自動マージ機能の実装

**機能ID**: `SPEC-47c6f44c` | **入力**: `/ollama-coordinator/specs/SPEC-47c6f44c/`の設計ドキュメント
**前提条件**: plan.md ✅, research.md ✅, data-model.md ✅, contracts/ ✅, quickstart.md ✅

## フォーマット: `[ID] [P?] 説明`

- **[P]**: 並列実行可能 (異なるファイル、依存関係なし)
- 説明には正確なファイルパスを含める

## Phase 3.1: セットアップ (並列実行可能)

**目的**: プロジェクト初期化、設定ファイル作成

- [x] **T001** [P] `.commitlintrc.json`を作成（Conventional Commits準拠設定）
  - **ファイル**: `/ollama-coordinator/.commitlintrc.json`
  - **参照**: `contracts/quality-checks.contract.yml` の commitlint ジョブ、`research.md` の commitlint設定
  - **内容**: `@commitlint/config-conventional` を拡張、日本語サポート、カスタムルール

- [x] **T002** [P] 既存`ci.yml`をバックアップ
  - **ファイル**: `/ollama-coordinator/.github/workflows/ci.yml`
  - **コマンド**: `cp .github/workflows/ci.yml .github/workflows/ci.yml.backup`
  - **理由**: 段階的移行、ロールバック可能性

## Phase 3.2: テストファースト (TDD - RED) ⚠️ Phase 3.3の前に完了必須

**重要**: これらのテストは記述され、実装前に失敗する必要がある

### Contract Tests (並列実行可能)

- [x] **T003** [P] `test-quality-checks.yml`を作成（品質チェックワークフローのテスト）
  - **ファイル**: `/ollama-coordinator/.github/workflows/test-quality-checks.yml`
  - **参照**: `contracts/quality-checks.contract.yml`
  - **目的**: 各ジョブ（tasks-check、rust-test、rust-lint、commitlint、markdownlint）が独立して実行可能かテスト
  - **期待**: 実装前なので失敗する（RED）

- [x] **T004** [P] `test-auto-merge.yml`を作成（自動マージワークフローのテスト）
  - **ファイル**: `/ollama-coordinator/.github/workflows/test-auto-merge.yml`
  - **参照**: `contracts/auto-merge.contract.yml`
  - **目的**: workflow_runトリガー、条件判定、GraphQL APIマージをテスト
  - **期待**: 実装前なので失敗する（RED）

- [x] **T005** テストワークフロー実行 → 失敗確認（RED確認）
  - **コマンド**: ダミーPRを作成してテストワークフローを起動
  - **確認**: `gh run list --workflow="test-quality-checks"` で失敗を確認
  - **コミット**: `test(workflow): quality-checks契約テスト追加` → `test(workflow): auto-merge契約テスト追加`

## Phase 3.3: コア実装 (TDD - GREEN)

**前提条件**: Phase 3.2完了、テストが失敗していることを確認

- [x] **T006** `quality-checks.yml`を作成（品質チェック統合ワークフロー）
  - **ファイル**: `/ollama-coordinator/.github/workflows/quality-checks.yml`
  - **参照**: `contracts/quality-checks.contract.yml`
  - **内容**:
    - Job 1: `tasks-check` - `.specify/scripts/checks/check-tasks.sh`呼び出し
    - Job 2: `rust-test` - `cargo test --all-features --workspace` (matrix: ubuntu-latest, windows-latest)
    - Job 3: `rust-lint` - `cargo fmt --check` + `cargo clippy`
    - Job 4: `commitlint` - `.specify/scripts/checks/check-commits.sh`呼び出し（PR時のみ）
    - Job 5: `markdownlint` - `npx markdownlint-cli`
  - **トリガー**: `pull_request` (main), `push` (feature/**)
  - **コミット**: `feat(workflow): quality-checksワークフロー実装`

- [x] **T007** `auto-merge.yml`を作成（自動マージワークフロー）
  - **ファイル**: `/ollama-coordinator/.github/workflows/auto-merge.yml`
  - **参照**: `contracts/auto-merge.contract.yml`, `research.md` のGraphQL API実装
  - **内容**:
    - トリガー: `workflow_run` on "Quality Checks" completion
    - 条件: `conclusion == 'success'` && `event == 'pull_request'`
    - Step 1: PR番号取得（`gh pr list --head $BRANCH`）
    - Step 2: PR状態チェック（`gh pr view --json isDraft,mergeable,mergeStateStatus`）
    - Step 3: 条件判定マージ（`isDraft == false` && `mergeable == 'MERGEABLE'` && `mergeStateStatus in ['CLEAN', 'UNSTABLE']`）
    - Step 4: GraphQL APIマージ（`gh api graphql`, MERGE method）
  - **permissions**: `contents: write`, `pull-requests: write`
  - **コミット**: `feat(workflow): auto-mergeワークフロー実装`

- [x] **T008** テストワークフロー再実行 → 合格確認（GREEN確認）
  - **コマンド**: 同じダミーPRでテストワークフローを再実行
  - **確認**: `gh run list --workflow="test-quality-checks"` で成功を確認
  - **リファクタリング**: 必要に応じてワークフローYAMLを最適化（REFACTOR）

## Phase 3.4: 統合テスト (Integration)

**前提条件**: Phase 3.3完了、コア実装が動作することを確認

### 統合テストシナリオ1: 未完了タスク → tasks-check失敗

- [ ] **T009** ダミーfeatureブランチ作成 → 未完了タスク含むPR → tasks-check失敗確認
  - **ブランチ**: `feature/test-tasks-fail`
  - **手順**:
    1. 未完了タスク（`- [ ]`）を含むtasks.mdを作成
    2. PR作成
    3. quality-checksワークフロー実行確認
    4. tasks-checkジョブが失敗することを確認
    5. auto-mergeがスキップされることを確認
  - **参照**: `quickstart.md` のテストシナリオ2
  - **確認**: `gh run view <RUN_ID>` でtasks-check失敗ログ確認

### 統合テストシナリオ2: 規約違反コミット → commitlint失敗

- [ ] **T010** 規約違反コミット含むPR → commitlint失敗確認
  - **ブランチ**: `feature/test-commitlint-fail`
  - **手順**:
    1. 規約違反コミット（例: `新機能追加`、プレフィックスなし）を作成
    2. PR作成
    3. quality-checksワークフロー実行確認
    4. commitlintジョブが失敗することを確認
    5. 失敗コミットのリストが表示されることを確認
  - **参照**: `quickstart.md` のテストシナリオ2（commitlint失敗）
  - **確認**: `gh run view <RUN_ID>` でcommitlint失敗ログ確認

### 統合テストシナリオ3: 全チェック合格 → 自動マージ成功

- [ ] **T011** 全チェック合格PR → auto-merge起動 → マージ成功確認
  - **ブランチ**: `feature/test-auto-merge-success`
  - **手順**:
    1. 全タスク完了（`- [x]`）、規約準拠コミットでPR作成
    2. quality-checksワークフロー実行 → 全ジョブ成功確認
    3. auto-mergeワークフロー起動確認
    4. PRがmainにマージされることを確認
    5. featureブランチが削除されることを確認
  - **参照**: `quickstart.md` のテストシナリオ1
  - **確認**: `gh pr view <PR_NUMBER>` でマージステータス確認

### 統合テストシナリオ4: ドラフトPR → 自動マージスキップ

- [ ] **T012** ドラフトPR → 品質チェック実行 → 自動マージスキップ確認
  - **ブランチ**: `feature/test-draft-pr`
  - **手順**:
    1. `--draft`オプションでPR作成
    2. quality-checksワークフロー実行 → 全ジョブ成功確認
    3. auto-mergeワークフロー起動確認
    4. `isDraft == true`のためマージスキップされることを確認
    5. ドラフト解除 → 再実行 → マージ成功確認
  - **参照**: `quickstart.md` のテストシナリオ3
  - **確認**: `gh run view <RUN_ID>` で「PR is a draft」ログ確認

## Phase 3.5: 仕上げ (Polish)

**前提条件**: Phase 3.4完了、統合テストがすべて合格

- [x] **T013** [P] `finish-feature.sh`のPRボディを更新
  - **ファイル**: `/ollama-coordinator/.specify/scripts/bash/finish-feature.sh`
  - **更新内容**:
    - PRボディの「GitHub Actionsが品質チェックを実行する」説明を実際のワークフロー名（"Quality Checks"、"Auto Merge"）に更新
    - 自動実行される処理の詳細を追加（tasks-check、rust-test、rust-lint、commitlint、markdownlint）
    - 自動マージ条件を明記（全チェック合格、ドラフトでない、マージ可能）
  - **コミット**: `docs(scripts): finish-feature.shのPRボディを更新`

- [x] **T014** [P] `CLAUDE.md`の自動マージセクションを更新
  - **ファイル**: `/ollama-coordinator/CLAUDE.md`
  - **更新内容**:
    - 「Worktree＆ブランチ運用」セクションに自動マージフローの詳細を追加
    - 「作業完了フロー」セクションの「自動実行される処理」を実際のワークフロー内容に更新
    - 品質チェックの詳細（5つのジョブ）を記載
    - ドラフトPRの扱いを明記
  - **コミット**: `docs: CLAUDE.mdに自動マージセクション追加`

- [ ] **T015** 既存`ci.yml`の統合または削除
  - **ファイル**: `/ollama-coordinator/.github/workflows/ci.yml`
  - **選択肢**:
    - **オプション1**: `quality-checks.yml`に統合（coverageジョブ追加）してから`ci.yml`削除
    - **オプション2**: 並行実行期間（1-2週間）後に削除
  - **推奨**: オプション2（段階的移行、リスク最小化）
  - **参照**: `research.md` の既存ワークフロー統合
  - **コミット**: `chore(workflow): ci.ymlを削除（quality-checksに統合済み）`

- [x] **T016** [P] ドキュメント最終確認とmarkdownlintチェック
  - **ファイル**: `CLAUDE.md`, `finish-feature.sh` のコメント、新規作成ワークフローYAML
  - **コマンド**: `npx markdownlint-cli '**/*.md' --ignore node_modules --ignore .git`
  - **修正**: markdownlint警告/エラーを修正
  - **コミット**: `style(docs): markdownlint修正`

- [ ] **T017** [P] 最終動作確認（E2Eテスト）
  - **手順**:
    1. 実際のfeatureブランチで`finish-feature.sh`実行
    2. PR作成 → quality-checks実行 → auto-merge実行 → mainマージ確認
    3. 全フロー成功確認
  - **参照**: `quickstart.md` の検証チェックリスト
  - **確認**: 本番環境でのフルフロー動作確認

- [ ] **T018** [P] テスト用ブランチとPRのクリーンアップ
  - **対象**:
    - `feature/test-tasks-fail`
    - `feature/test-commitlint-fail`
    - `feature/test-auto-merge-success`
    - `feature/test-draft-pr`
  - **コマンド**:
    - `gh pr close <PR_NUMBER>`
    - `git branch -D <BRANCH_NAME>`
    - `git push origin --delete <BRANCH_NAME>`
  - **確認**: すべてのテスト用リソースが削除されたことを確認

## 依存関係グラフ

```text
Phase 3.1: Setup
  T001 [P] ─┐
  T002 [P] ─┤
            └─→ Phase 3.2: Tests

Phase 3.2: Tests (TDD - RED)
  T003 [P] ─┐
  T004 [P] ─┤
            ├─→ T005 (確認) ─→ Phase 3.3: Implementation

Phase 3.3: Implementation (TDD - GREEN)
  T006 ───→ T008 (確認)
    ↓
  T007 ───→ T008 (確認) ─→ Phase 3.4: Integration

Phase 3.4: Integration
  T009 ───┐
  T010 ───┤
  T011 ───┤ (順次実行、互いに依存)
  T012 ───┘
            └─→ Phase 3.5: Polish

Phase 3.5: Polish
  T013 [P] ─┐
  T014 [P] ─┤
  T015 ─────┤
  T016 [P] ─┤
  T017 [P] ─┤
  T018 [P] ─┘
```

## 並列実行例

### Setup Phase (全タスク並列実行可能)

```bash
# T001, T002を並列実行
Task 1: "`.commitlintrc.json`を作成"
Task 2: "既存`ci.yml`をバックアップ"
```

### Test Phase (Contract tests並列実行可能)

```bash
# T003, T004を並列実行
Task 1: "`test-quality-checks.yml`を作成"
Task 2: "`test-auto-merge.yml`を作成"
```

### Polish Phase (独立タスク並列実行可能)

```bash
# T013, T014, T016, T017, T018を並列実行
Task 1: "`finish-feature.sh`のPRボディを更新"
Task 2: "`CLAUDE.md`の自動マージセクションを更新"
Task 3: "ドキュメント最終確認とmarkdownlintチェック"
Task 4: "最終動作確認（E2Eテスト）"
Task 5: "テスト用ブランチとPRのクリーンアップ"
```

## タスク完全性検証

- [x] すべてのcontractsに対応するテストがある
  - `quality-checks.contract.yml` → T003
  - `auto-merge.contract.yml` → T004
- [x] すべてのentitiesにmodelタスクがある
  - N/A (ワークフローYAML定義のみ、データモデルなし)
- [x] すべてのテストが実装より先にある
  - T003-T005 (Tests) → T006-T008 (Implementation)
- [x] 並列タスクは本当に独立している
  - [P]マークのタスクは異なるファイルを変更、依存関係なし
- [x] 各タスクは正確なファイルパスを指定
  - すべてのタスクに絶対パスまたは明確な相対パスを記載
- [x] 同じファイルを変更する[P]タスクがない
  - 確認済み、同じファイルを変更するタスクは順次実行

## 注意事項

- [P] タスク = 異なるファイル、依存関係なし、並列実行可能
- **TDD厳守**: Phase 3.2（Tests）が必ずPhase 3.3（Implementation）より先
- **RED-GREEN-REFACTOR**: T005でRED確認、T006-T007でGREEN、T008でREFACTOR
- **各タスク後にコミット**: TDDサイクルのコミット履歴を保持
- **統合テストは順次実行**: T009-T012は互いに影響を与える可能性があるため順次実行

## 総タスク数

**18タスク** (Setup: 2, Tests: 3, Implementation: 3, Integration: 4, Polish: 6)

**並列実行可能タスク**: 10タスク ([P]マーク)
**順次実行必須タスク**: 8タスク

**推定実装時間**: 3-4時間
- Setup: 15分
- Tests (RED): 30分
- Implementation (GREEN): 1時間
- Integration: 1時間
- Polish: 30分
- Refactor: 30分

---

**タスク生成完了日**: 2025-10-30
**次のステップ**: `/speckit.implement`またはタスクの手動実行
