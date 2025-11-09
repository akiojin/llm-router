# Required Status Checks 更新手順

ワークフロー構造変更に伴うブランチ保護ルールの更新が必要です。

## 重要な注意事項

**Required Checksでは、ジョブIDではなく、各ジョブの`name:`フィールドで指定された名前を使用します。**

例:
```yaml
jobs:
  rust-lint:              # ← ジョブID（Required Checksでは使わない）
    name: Rust Format & Clippy  # ← この名前を使用
```

## 変更内容

### 旧構成 (quality-checks.yml)
実際のチェック名（古い構成）:
- `tasks-check`
- `rust-test (ubuntu-latest, stable)`
- `rust-test (windows-latest, stable)`
- `rust-lint`
- `openai-proxy-tests`
- `commitlint`
- `markdownlint`

### 新構成 (lint.yml + test.yml)
実際のチェック名（新しい構成）:
- `Commit Message Lint` (旧: `commitlint`)
- `Markdown Lint` (旧: `markdownlint`)
- `Rust Format & Clippy` (旧: `rust-lint`)
- `Rust Tests (ubuntu-latest)` (旧: `rust-test (ubuntu-latest, stable)`)
- `Rust Tests (windows-latest)` (旧: `rust-test (windows-latest, stable)`)
- `Tasks Completion Check` (オプション)
- `OpenAI API Compatibility Tests` (オプション)
- `Hook Tests` (オプション)
- `Test Claude Code PreToolUse Hooks` (オプション)

## 必須チェックリスト（現在の設定）

developブランチで必須にしているチェック:
- ✓ `Commit Message Lint`
- ✓ `Markdown Lint`
- ✓ `Rust Format & Clippy`
- ✓ `Rust Tests (ubuntu-latest)`
- ✓ `Rust Tests (windows-latest)`

## 更新手順

### 1. PRで実際のチェック名を確認

```bash
# PRで実行されているチェック名を取得
gh pr checks <PR番号> --json name --jq '.[].name' | sort -u
```

### 2. GitHub UIで手動更新（推奨）

1. リポジトリページへ移動
2. **Settings** → **Branches**
3. `develop` ブランチの **Edit** ボタンをクリック
4. **Require status checks to pass before merging** セクションで検索して追加：
   - `Commit Message Lint`
   - `Markdown Lint`
   - `Rust Format & Clippy`
   - `Rust Tests (ubuntu-latest)`
   - `Rust Tests (windows-latest)`

5. **Save changes**

### 3. GitHub CLIで更新（自動化）

```bash
# 現在の設定を確認
gh api repos/akiojin/ollama-coordinator/branches/develop/protection/required_status_checks

# 正しいチェック名で更新
gh api \
  --method PUT \
  repos/akiojin/ollama-coordinator/branches/develop/protection/required_status_checks/contexts \
  -H "Accept: application/vnd.github+json" \
  --raw-field 'contexts[]=Commit Message Lint' \
  --raw-field 'contexts[]=Markdown Lint' \
  --raw-field 'contexts[]=Rust Format & Clippy' \
  --raw-field 'contexts[]=Rust Tests (ubuntu-latest)' \
  --raw-field 'contexts[]=Rust Tests (windows-latest)'
```

## ワークフローのname設定確認

各ワークフローファイルで`name:`フィールドを確認:

### lint.yml
```yaml
jobs:
  rust-lint:
    name: Rust Format & Clippy  # ← Required Checksで使用
  markdownlint:
    name: Markdown Lint
  commitlint:
    name: Commit Message Lint
  hook-tests:
    name: Hook Tests
```

### test.yml
```yaml
jobs:
  tasks-check:
    name: Tasks Completion Check
  rust-test:
    name: Rust Tests (${{ matrix.os }})  # ubuntu-latest / windows-latest
  openai-proxy-tests:
    name: OpenAI API Compatibility Tests
```

## 影響範囲

- **develop** ブランチ: 更新済み
- **main** ブランチ: 必要に応じて同様の更新を適用

## 確認方法

PR作成後、以下のコマンドでチェックが正しく実行されることを確認:

```bash
gh pr checks <PR番号>
```

## トラブルシューティング

### エラー: "Required checks not found"

**原因**: ジョブIDとジョブの`name:`が一致していない

**解決方法**:
1. 実際のPRチェック結果を確認
2. ワークフローファイルの`name:`フィールドを確認
3. Required Checksを実際のチェック名で設定

### 古いチェック名が残っている場合

PRを再実行（re-run all jobs）:
```bash
gh pr checks <PR番号> --watch
```

### チェック名にスペースが含まれる場合

GitHub CLIでは`--raw-field`を使用してスペースを含む名前を正しく渡します:
```bash
--raw-field 'contexts[]=Rust Format & Clippy'
```

## 参考

- [GitHub Docs: Required status checks](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches#require-status-checks-before-merging)
- PR #59: ワークフロー構造変更
- **重要**: ジョブIDではなく、ジョブの`name:`フィールドを使用すること
