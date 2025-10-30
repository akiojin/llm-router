# 技術リサーチ: 自動マージ機能

**機能ID**: `SPEC-47c6f44c` | **日付**: 2025-10-30

## 参考実装: @akiojin/claude-worktree

### 決定: GitHub Actions workflow_run + GraphQL API

**参考リポジトリ**:

- URL: <https://github.com/akiojin/claude-worktree>
- 自動マージワークフロー: `.github/workflows/auto-merge.yml`

**主要な実装パターン**:

1. **workflow_run トリガー**:

```yaml
on:
  workflow_run:
    workflows: ["Test", "Lint"]
    types: [completed]
```

1. **PR状態チェック**:

```yaml
- name: Get PR status
  run: |
    PR_STATUS=$(gh pr view $PR_NUMBER --json isDraft,mergeable,mergeStateStatus)
```

1. **GraphQL API マージ**:

```yaml
gh api graphql -f query='
  mutation($pr:ID!) {
    mergePullRequest(input:{pullRequestId:$pr, mergeMethod:MERGE}) {
      pullRequest { number }
    }
  }
' -f pr="$PR_ID"
```

**理由**:

- シンプルで信頼性の高い実装
- GitHub標準APIを使用（追加の依存関係不要）
- 実績のある実装パターン

**検討した代替案**:

- GitHub Apps + Webhooks: 複雑すぎる、オーバーエンジニアリング
- サードパーティアクション (actions/merge-pr): 依存関係増加、カスタマイズ性低い

## workflow_run トリガーの仕様

### 決定: workflow_run.conclusion == 'success' でフィルタリング

**重要な制約**:

1. **トリガー条件**:
   - `workflow_run.conclusion`: success, failure, neutral, cancelled, skipped, timed_out
   - `workflow_run.event`: pull_request, push, workflow_dispatch など
   - トリガー元ワークフロー名は正確に一致する必要がある

2. **実行コンテキスト**:
   - workflow_runは **デフォルトブランチ** のワークフローファイルを実行
   - PRブランチのワークフローファイルは使用されない（セキュリティ上の理由）
   - `github.event.workflow_run.head_branch` でPRのブランチ名を取得

3. **並列実行**:
   - 複数のワークフローが完了した場合、それぞれ独立してworkflow_runが起動
   - 同じPRに対して複数回実行される可能性がある

**理由**:

- セキュリティ: PRからの悪意あるコード実行を防ぐ
- 信頼性: デフォルトブランチのワークフローは検証済み

**検討した代替案**:

- pull_request_target: セキュリティリスクが高い
- pull_request + auto-merge: 権限不足（GITHUB_TOKEN制限）

## GraphQL API マージ実装

### 決定: mergePullRequest mutation + MERGE method

**実装詳細**:

1. **PR IDの取得**:

```bash
PR_NUMBER=$(gh pr list --head $BRANCH_NAME --json number --jq '.[0].number')
PR_ID=$(gh pr view $PR_NUMBER --json id --jq '.id')
```

1. **マージ実行**:

```bash
gh api graphql -f query='
  mutation($pr:ID!) {
    mergePullRequest(input:{
      pullRequestId: $pr,
      mergeMethod: MERGE,
      commitHeadline: "Merge pull request #'$PR_NUMBER' from '$BRANCH_NAME'",
      commitBody: "Auto-merged by GitHub Actions"
    }) {
      pullRequest {
        number
        merged
      }
    }
  }
' -f pr="$PR_ID"
```

1. **エラーハンドリング**:
   - コンフリクト: `mergeable != MERGEABLE` で事前チェック
   - 権限エラー: `permissions: contents: write, pull-requests: write` 必須
   - ドラフトPR: `isDraft == true` で事前チェック

**理由**:

- MERGE method: コミット履歴保持、TDDサイクル可視化
- GraphQL: REST APIより柔軟、エラー情報が詳細
- gh CLI: 認証不要（GITHUB_TOKEN自動使用）

**検討した代替案**:

- REST API (`POST /repos/{owner}/{repo}/pulls/{pull_number}/merge`): GraphQLより冗長
- Octokit (JavaScript): 依存関係増加、Bash実装で十分

## commitlint設定

### 決定: @commitlint/config-conventional + 日本語サポート

**設定内容** (`.commitlintrc.json`):

```json
{
  "extends": ["@commitlint/config-conventional"],
  "rules": {
    "type-enum": [
      2,
      "always",
      [
        "feat", "fix", "docs", "style",
        "refactor", "test", "chore", "revert"
      ]
    ],
    "subject-case": [0],
    "body-max-line-length": [2, "always", 200]
  }
}
```

**理由**:

- Conventional Commits: 業界標準、自動changelog生成可能
- `subject-case: [0]`: 日本語対応（大文字小文字チェック無効化）
- `body-max-line-length: 200`: 日本語の長文対応

**検討した代替案**:

- カスタム設定: 複雑すぎる、標準から逸脱
- commitizen: インタラクティブツール不要（自動化優先）

## 既存ci.ymlとの統合

### 決定: quality-checks.ymlに統合、ci.ymlは削除

**現在のci.yml構成**:

- rust test (ubuntu-latest, windows-latest)
- rust lint (cargo fmt, cargo clippy)
- coverage (cargo-llvm-cov)

**統合戦略**:

1. **Phase 1: 並行実行**:
   - ci.ymlとquality-checks.ymlを両方維持
   - 動作確認期間（1-2週間）

2. **Phase 2: 移行**:
   - quality-checks.ymlにcoverageジョブ追加
   - ci.ymlを削除

3. **Phase 3: 最適化**:
   - 重複ジョブの統合
   - matrix戦略の最適化

**理由**:

- 段階的移行: リスク最小化
- 後方互換性: 既存のPRに影響しない
- テスト容易性: 問題発生時のロールバック可能

**検討した代替案**:

- ci.ymlをquality-checks.ymlにリネーム: git履歴が失われる
- 両方を永続的に維持: 冗長、メンテナンスコスト増加

## 技術的決定サマリー

| 項目 | 決定 | 理由 |
|------|------|------|
| トリガー | workflow_run | セキュリティ、信頼性 |
| マージ方法 | GraphQL API + MERGE | 履歴保持、柔軟性 |
| commitlint | Conventional Commits | 業界標準、自動化対応 |
| 統合戦略 | 段階的移行 | リスク最小化 |
| テスト | 実PR使用 | 実依存関係、憲章準拠 |

## 未解決の問題

なし。すべての技術的不明点は解決済み。

---

**リサーチ完了日**: 2025-10-30
