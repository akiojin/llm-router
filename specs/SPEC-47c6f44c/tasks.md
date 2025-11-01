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
  - **手順**: メンテナが検証用PRに対して`Quality Checks`ワークフローを再実行
  - **確認**: `gh run list --workflow="Quality Checks"` で成功を確認（ローカル環境は移動せず実行）
  - **リファクタリング**: 必要に応じてワークフローYAMLを最適化（REFACTOR）

## Phase 3.4: 統合テスト (Integration)

**前提条件**: Phase 3.3完了、コア実装が動作することを確認

### 統合テストシナリオ1: 未完了タスク → tasks-check失敗

- [x] **T009** 未完了タスクシナリオ（メンテナ起動） → tasks-check失敗確認
  - **担当**: メンテナが「Auto Merge QA」ワークフローを未完了タスクモードで起動
  - **手順**:
    1. ワークフロー実行により未完了タスクを含む検証PRが生成される
    2. `Quality Checks` ワークフローが自動実行されることを確認
    3. tasks-checkジョブが失敗し、未完了タスクの詳細がログ出力されることを確認
    4. Auto Mergeワークフローが条件不一致でスキップされることを確認
  - **参照**: `quickstart.md` のテストシナリオ2
  - **確認**: `gh run view <RUN_ID>` でtasks-check失敗ログを閲覧（読み取りのみ）

### 統合テストシナリオ2: 規約違反コミット → commitlint失敗

- [x] **T010** 規約違反コミットシナリオ（メンテナ起動） → commitlint失敗確認
  - **担当**: メンテナが規約違反コミットを含む検証PRを生成するワークフローを起動
  - **手順**:
    1. 検証PRで`Quality Checks`ワークフローが自動実行されることを確認
    2. commitlintジョブが失敗し、違反コミットがログに列挙されることを確認
    3. Auto Mergeワークフローがスキップされることを確認
  - **参照**: `quickstart.md` のテストシナリオ2（commitlint失敗）
  - **確認**: `gh run view <RUN_ID>` でcommitlint失敗ログを閲覧

### 統合テストシナリオ3: 全チェック合格 → 自動マージ成功

- [x] **T011** 全チェック合格シナリオ → 自動マージ成功確認
  - **担当**: メンテナが全チェック合格用の検証PRを生成
  - **手順**:
    1. `Quality Checks` ワークフローが成功し、全ジョブが合格していることを確認
    2. Auto Mergeワークフローが起動し、マージmutationを実行することを確認
    3. PRタイムラインで「Merged automatically by GitHub Actions」が表示されることを確認
    4. Auto Mergeログにリモートブランチ削除完了メッセージが記録されていることを確認（ローカル操作不要）
  - **参照**: `quickstart.md` のテストシナリオ1
  - **確認**: `gh pr view <PR_NUMBER>` でマージステータスを閲覧

### 統合テストシナリオ4: ドラフトPR → 自動マージスキップ

- [x] **T012** ドラフトPRシナリオ → 自動マージスキップ確認
  - **担当**: メンテナがドラフト状態の検証PRを生成
  - **手順**:
    1. `Quality Checks` ワークフローが実行され、全ジョブ成功ログを確認
    2. Auto Mergeワークフローが起動するが、`isDraft == true` のためマージがスキップされることを確認
    3. メンテナがPRをドラフト解除し、再度ワークフロー実行 → マージ成功ログを確認
  - **参照**: `quickstart.md` のテストシナリオ3
  - **確認**: `gh run view <RUN_ID>` で「PR is a draft」ログを閲覧

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

- [x] **T015** 既存`ci.yml`の統合または削除
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

- [x] **T017** [P] 最終動作確認（E2Eテスト）
  - **手順**:
    1. リポジトリメンテナに依頼し、検証用ワークフローを「本番相当フロー」で実行してもらう。
    2. `Quality Checks` → `Auto Merge` の一連のCI結果を確認し、mainブランチへの自動マージ完了をGitHub上でレビューする。
    3. 結果サマリとログの要約を `docs/qa/auto-merge-report.md` に追記し、共有する。
  - **参照**: `quickstart.md` の検証チェックリスト
  - **確認**: 本番環境と同等のシナリオで自動マージが問題なく完了すること。

- [x] **T018** [P] テスト用ブランチとPRのクリーンアップ
  - **対象**: 検証ワークフローが生成した自動テストPR（`Auto Merge QA` ラベル付き）
  - **依頼先**: リポジトリメンテナ
  - **実施内容**:
    - GitHubのUIからPRをクローズまたはマージ結果を確認したのちに削除
    - リモートに残ったテスト用ブランチをGitHub上で削除（ローカルで`git branch -D`等は実行しない）
    - 必要に応じてスクリプトで自動削除が成功したかを再確認
  - **確認**: すべてのテスト用リソースがGitHub側で整理され、ローカル環境に変化がないこと。

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
