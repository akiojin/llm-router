# Required Status Checks 更新手順

ワークフロー構造変更に伴うブランチ保護ルールの更新が必要です。

## 変更内容

### 旧構成 (quality-checks.yml)
- `tasks-check`
- `rust-test (ubuntu-latest, stable)`
- `rust-test (windows-latest, stable)`
- `rust-lint`
- `openai-proxy-tests`
- `commitlint`
- `markdownlint`

### 新構成 (lint.yml + test.yml)
- `tasks-check`
- `rust-test (ubuntu-latest)` ← matrix名変更
- `rust-test (windows-latest)` ← matrix名変更
- `rust-lint`
- `openai-proxy-tests`
- `commitlint`
- `markdownlint`
- `hook-tests` ← **新規追加**

## 更新手順

### 1. GitHub UIで手動更新（推奨）

1. リポジトリページへ移動
2. **Settings** → **Branches**
3. `develop` ブランチの **Edit** ボタンをクリック
4. **Require status checks to pass before merging** セクションで以下を更新：

   **削除**:
   - `rust-test (ubuntu-latest, stable)`
   - `rust-test (windows-latest, stable)`

   **追加**:
   - `rust-test (ubuntu-latest)`
   - `rust-test (windows-latest)`
   - `hook-tests`

5. **Save changes**

### 2. GitHub CLIで更新（自動化）

```bash
# 現在の設定を確認
gh api repos/akiojin/ollama-coordinator/branches/develop/protection/required_status_checks

# 新しい設定で更新
gh api \
  --method PATCH \
  repos/akiojin/ollama-coordinator/branches/develop/protection \
  -H "Accept: application/vnd.github+json" \
  --field 'required_status_checks[strict]=true' \
  --field 'required_status_checks[contexts][]=tasks-check' \
  --field 'required_status_checks[contexts][]=rust-test (ubuntu-latest)' \
  --field 'required_status_checks[contexts][]=rust-test (windows-latest)' \
  --field 'required_status_checks[contexts][]=rust-lint' \
  --field 'required_status_checks[contexts][]=openai-proxy-tests' \
  --field 'required_status_checks[contexts][]=commitlint' \
  --field 'required_status_checks[contexts][]=markdownlint' \
  --field 'required_status_checks[contexts][]=hook-tests'
```

## 影響範囲

- **develop** ブランチ: 必須
- **main** ブランチ: 必要に応じて同様の更新を適用

## 確認方法

PR作成後、以下のチェックが正しく実行されることを確認:

```bash
gh pr checks <PR番号>
```

期待されるチェック一覧:
- ✓ tasks-check
- ✓ rust-test (ubuntu-latest)
- ✓ rust-test (windows-latest)
- ✓ rust-lint
- ✓ openai-proxy-tests
- ✓ commitlint
- ✓ markdownlint
- ✓ hook-tests

## 注意事項

- 既存のPR（#59など）では古いチェック名が残る可能性があります
- 新しいPRを作成すると新しいチェック名が適用されます
- ブランチ保護ルール更新後は、すべてのチェックが必須になります

## トラブルシューティング

### 古いチェック名が残っている場合

PRを再実行（re-run all jobs）することで新しいチェック名が適用されます:

```bash
gh pr checks <PR番号> --watch
```

### Required checksが見つからない場合

1. ワークフローが正常に実行されているか確認
2. ジョブ名（job ID）が正しく設定されているか確認
3. ブランチ保護ルールの設定を再確認

## 参考

- [GitHub Docs: Required status checks](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches#require-status-checks-before-merging)
- PR #59: ワークフロー構造変更
