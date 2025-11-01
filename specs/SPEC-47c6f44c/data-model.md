# データモデル: 自動マージ機能

**機能ID**: `SPEC-47c6f44c` | **日付**: 2025-10-30

## エンティティ概要

この機能は、GitHub Actionsワークフローという「ステートレス」な実行モデルを使用します。データモデルは、ワークフローYAML定義とGitHub APIレスポンスの構造を記述します。

## エンティティ1: QualityChecksWorkflow

**目的**: PR作成時に品質チェックを並列実行し、全体の成功/失敗を判定

**属性**:

- `name`: "Quality Checks" (string) - ワークフロー名
- `trigger`: (object) - トリガー条件
  - `pull_request.branches`: ["main"] - PRターゲットブランチ
  - `push.branches`: ["feature/**"] - pushトリガーブランチ
- `jobs`: (array) - 並列実行ジョブ
  - `tasks-check`: tasks.md完了チェック
  - `rust-test`: Rustテスト実行（matrix: ubuntu-latest, windows-latest）
  - `rust-lint`: Rust lintチェック（fmt, clippy）
  - `commitlint`: コミットメッセージ検証
  - `markdownlint`: マークダウンファイルlint
- `conclusion`: "success" | "failure" | "cancelled" (string) - 全体結果

**関係**:

- → AutoMergeWorkflow: workflow_runトリガーで連携
- → CheckScripts: 各ジョブが`.specify/scripts/checks/`を呼び出し

**検証ルール**:

- すべてのジョブが`success`の場合のみ、全体が`success`
- 1つでもジョブが失敗した場合、全体が`failure`
- ジョブは独立して実行可能（依存関係なし）

**状態遷移**:

```text
[queued] → [in_progress] → [success/failure/cancelled]
     ↓
   [timeout (6時間)] → [timed_out]
```

## エンティティ2: AutoMergeWorkflow

**目的**: QualityChecksWorkflow完了後、条件判定してPRをマージ

**属性**:

- `name`: "Auto Merge" (string) - ワークフロー名
- `trigger`: (object) - トリガー条件
  - `workflow_run.workflows`: ["Quality Checks"] - トリガー元ワークフロー
  - `workflow_run.types`: ["completed"] - トリガー種別
- `conditions`: (object) - マージ条件
  - `conclusion`: "success" (必須)
  - `event`: "pull_request" (必須)
  - `isDraft`: false (必須)
  - `mergeable`: "MERGEABLE" (必須)
  - `mergeStateStatus`: "CLEAN" | "UNSTABLE" (許容)
- `merge_method`: "MERGE" (string) - マージ方法
- `permissions`: (object) - 必要な権限
  - `contents`: "write"
  - `pull-requests`: "write"

**関係**:

- ← QualityChecksWorkflow: workflow_runトリガーで受信
- → GitHub GraphQL API: mergePullRequest mutation実行
- → GitHub PR API: PR状態取得

**検証ルール**:

- すべての`conditions`が真の場合のみマージ実行
- 1つでも条件が偽の場合、マージスキップ（ログ記録のみ）
- マージ失敗時、エラーログ記録（リトライなし）

**状態遷移**:

```text
[workflow_run triggered] → [check conditions]
                                ↓
                            [all true] → [merge PR] → [success]
                                ↓
                           [any false] → [skip merge] → [skipped]
                                ↓
                          [merge error] → [failure]
```

## エンティティ3: CommitlintConfig

**目的**: Conventional Commits準拠のコミットメッセージ検証

**属性**:

- `extends`: ["@commitlint/config-conventional"] (array) - ベース設定
- `rules`: (object) - カスタムルール
  - `type-enum`: (array) - 許可されたタイプ
    - 値: ["feat", "fix", "docs", "style", "refactor", "test", "chore", "revert"]
    - レベル: 2 (error)
  - `subject-case`: (array) - タイトルのcase検証
    - 値: [0] (無効化、日本語対応)
  - `body-max-line-length`: (array) - 本文の最大行長
    - 値: [2, "always", 200] (error, 200文字)

**関係**:

- ← QualityChecksWorkflow.commitlint: commitlintジョブで使用
- → `.specify/scripts/checks/check-commits.sh`: スクリプトで読み込み

**検証ルール**:

- コミットメッセージは `<type>: <subject>` 形式必須
- `<type>` は `type-enum` に含まれる値のみ許可
- `<subject>` は空でない
- `<body>` は200文字以内（省略可）

## エンティティ4: PRStatus (GitHub API レスポンス)

**目的**: PR状態を表現（GitHub GraphQL APIから取得）

**属性**:

- `id`: (string) - PR ID (GraphQL)
- `number`: (integer) - PR番号
- `isDraft`: (boolean) - ドラフトフラグ
- `mergeable`: "MERGEABLE" | "CONFLICTING" | "UNKNOWN" (enum) - マージ可能性
- `mergeStateStatus`: "CLEAN" | "UNSTABLE" | "DIRTY" | "BLOCKED" (enum) - マージ状態
- `headBranch`: (string) - PRのブランチ名

**関係**:

- ← AutoMergeWorkflow: 条件判定で使用
- ← GitHub PR API: `gh pr view`で取得

**検証ルール**:

- `mergeable == "MERGEABLE"` かつ `mergeStateStatus in ["CLEAN", "UNSTABLE"]` の場合のみマージ可能
- `isDraft == true` の場合、マージ不可

## データフロー

```text
1. PR作成
    ↓
2. QualityChecksWorkflow起動
    ↓
3. 5つのジョブ並列実行
    - tasks-check
    - rust-test (ubuntu, windows)
    - rust-lint
    - commitlint (CommitlintConfigを使用)
    - markdownlint
    ↓
4. 全ジョブ成功 → conclusion="success"
    ↓
5. AutoMergeWorkflow起動 (workflow_run)
    ↓
6. PRStatus取得 (GitHub API)
    ↓
7. 条件判定 (conditions)
    ↓ (全条件true)
8. PRマージ (GraphQL API, MERGE method)
    ↓
9. AutoMergeWorkflowログでリモートブランチ削除完了を記録（GitHub自動処理、ローカル操作不要）
```

## エラーシナリオ

### シナリオ1: tasks-check失敗

- 原因: tasks.mdに未完了タスク存在（`- [ ]`）
- 結果: QualityChecksWorkflow.conclusion = "failure"
- 影響: AutoMergeWorkflowはスキップ（条件不一致）

### シナリオ2: commitlint失敗

- 原因: コミットメッセージが規約違反
- 結果: QualityChecksWorkflow.conclusion = "failure"
- 影響: AutoMergeWorkflowはスキップ

### シナリオ3: ドラフトPR

- 原因: PR作成時に`--draft`オプション使用
- 結果: QualityChecksWorkflow.conclusion = "success" but PRStatus.isDraft = true
- 影響: AutoMergeWorkflow条件判定でスキップ

### シナリオ4: コンフリクト

- 原因: mainブランチとPRブランチでコンフリクト発生
- 結果: PRStatus.mergeable = "CONFLICTING"
- 影響: AutoMergeWorkflow条件判定でスキップ

---

**データモデル完了日**: 2025-10-30
