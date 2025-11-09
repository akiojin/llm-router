# Quickstart Guide: Worktree環境での作業境界強制システム

このガイドでは、Claude Code PreToolUse Hookによる作業境界強制システムの動作確認と
テスト実行手順を説明します。

## 前提条件

- Git 2.5以降（git worktree機能サポート）
- jq（JSON処理ライブラリ）
- Bash 4.0以降
- Claude Code 1.0以降（PreToolUse Hook API対応版）
- Node.js（Bats-coreテストフレームワーク用）

## Hook設定の確認手順

### 1. Hookスクリプトの存在確認

```bash
# Hookスクリプトファイルが存在することを確認
ls -la .claude/hooks/

# 期待される出力:
# -rwxr-xr-x block-cd-command.sh
# -rwxr-xr-x block-git-branch-ops.sh
```

### 2. 設定ファイルの確認

```bash
# settings.jsonにHook設定が記述されていることを確認
cat .claude/settings.json | jq '.hooks.PreToolUse'

# 期待される出力: 2つのhookが登録されている
# [
#   {
#     "matcher": "Bash",
#     "hooks": [
#       {"type": "command", "command": ".claude/hooks/block-git-branch-ops.sh"},
#       {"type": "command", "command": ".claude/hooks/block-cd-command.sh"}
#     ]
#   }
# ]
```

### 3. 実行権限の確認

```bash
# Hookスクリプトに実行権限が付与されていることを確認
[ -x .claude/hooks/block-git-branch-ops.sh ] && echo "✅ block-git-branch-ops.sh: executable"
[ -x .claude/hooks/block-cd-command.sh ] && echo "✅ block-cd-command.sh: executable"
```

## 手動テスト実行例

### Git ブランチ操作のブロック確認

#### ✅ 許可される操作（読み取り専用）

```bash
# ブランチ一覧の表示（許可される）
echo '{"tool_name":"Bash","tool_input":{"command":"git branch"}}' | \
  .claude/hooks/block-git-branch-ops.sh
# 期待: exit 0、出力なし
```

#### ❌ ブロックされる操作

```bash
# ブランチ切り替え（ブロックされる）
echo '{"tool_name":"Bash","tool_input":{"command":"git checkout main"}}' | \
  .claude/hooks/block-git-branch-ops.sh
# 期待: exit 2、JSON with "decision": "block"

# 新しいWorktree作成（ブロックされる）
echo '{"tool_name":"Bash","tool_input":{"command":"git worktree add /tmp/test"}}' | \
  .claude/hooks/block-git-branch-ops.sh
# 期待: exit 2、JSON with "decision": "block"
```

### ディレクトリ移動のブロック確認

#### ✅ 許可される操作（Worktree内）

```bash
# Worktree内のディレクトリ移動（許可される）
echo '{"tool_name":"Bash","tool_input":{"command":"cd src"}}' | \
  .claude/hooks/block-cd-command.sh
# 期待: exit 0、出力なし

# カレントディレクトリ（許可される）
echo '{"tool_name":"Bash","tool_input":{"command":"cd ."}}' | \
  .claude/hooks/block-cd-command.sh
# 期待: exit 0、出力なし
```

#### ❌ ブロックされる操作（Worktree外）

```bash
# ルートディレクトリへの移動（ブロックされる）
echo '{"tool_name":"Bash","tool_input":{"command":"cd /"}}' | \
  .claude/hooks/block-cd-command.sh
# 期待: exit 2、JSON with "decision": "block"

# ホームディレクトリへの移動（ブロックされる）
echo '{"tool_name":"Bash","tool_input":{"command":"cd ~"}}' | \
  .claude/hooks/block-cd-command.sh
# 期待: exit 2、JSON with "decision": "block"

# Worktree外への移動（ブロックされる）
echo '{"tool_name":"Bash","tool_input":{"command":"cd ../.."}}' | \
  .claude/hooks/block-cd-command.sh
# 期待: exit 2、JSON with "decision": "block"
```

## 自動テストスイート実行手順

### Bats-coreのインストール確認

```bash
# Bats-coreがインストールされていることを確認
npx bats --version
# 期待される出力: Bats 1.13.0 (またはそれ以降)
```

### 全テストの実行

```bash
# すべてのHookテストを実行
npx bats tests/hooks/test-block-git-branch-ops.bats tests/hooks/test-block-cd-command.bats

# 期待される出力:
# 1..13
# ok 1 git branch without arguments is allowed
# ok 2 git branch --list is allowed
# ok 3 git checkout is blocked
# ok 4 git switch is blocked
# ok 5 git worktree add is blocked
# ok 6 git branch -d is blocked
# ok 7 compound command with git checkout is blocked
# ok 8 cd . is allowed
# ok 9 cd src is allowed (within worktree)
# ok 10 cd / is blocked
# ok 11 cd ~ is blocked
# ok 12 cd /ollama-coordinator is blocked (outside worktree)
# ok 13 cd ../.. is blocked (parent directory)
```

### 個別テストスイートの実行

```bash
# Git操作のテストのみ実行
npx bats tests/hooks/test-block-git-branch-ops.bats

# cd操作のテストのみ実行
npx bats tests/hooks/test-block-cd-command.bats
```

### Makefileターゲットの使用

```bash
# Makefileターゲットを使用（T009完了後）
make test-hooks

# Quality checks全体を実行（Hookテストを含む）
make quality-checks
```

## トラブルシューティング

### 問題1: Hookスクリプトが実行されない

**症状**: コマンドがブロックされずに実行される

**確認事項**:

1. Claude Codeのバージョンが1.0以降であることを確認
   ```bash
   claude --version
   ```

2. settings.jsonのHook設定を確認
   ```bash
   cat .claude/settings.json | jq '.hooks.PreToolUse'
   ```

3. Hookスクリプトの実行権限を確認
   ```bash
   ls -l .claude/hooks/
   ```

**解決方法**:

```bash
# 実行権限を付与
chmod +x .claude/hooks/block-git-branch-ops.sh
chmod +x .claude/hooks/block-cd-command.sh

# Claude Codeを再起動
```

### 問題2: テストが失敗する

**症状**: `npx bats` でテストが失敗する

**確認事項**:

1. jqがインストールされているか確認
   ```bash
   which jq
   jq --version
   ```

2. Gitリポジトリ内で実行しているか確認
   ```bash
   git rev-parse --show-toplevel
   ```

3. Python3がインストールされているか確認（高度なトークン解析用）
   ```bash
   which python3
   python3 --version
   ```

**解決方法**:

```bash
# jqのインストール（Ubuntu/Debian）
sudo apt-get install jq

# jqのインストール（macOS）
brew install jq

# Python3のインストール（オプション）
sudo apt-get install python3  # Ubuntu/Debian
brew install python3           # macOS
```

### 問題3: JSON parse errorが発生する

**症状**: テスト実行時に "parse error: Invalid numeric literal" が表示される

**原因**: Hook出力にstderrとstdoutが混在している

**解決方法**:

この問題は既にテストスクリプトで対処済みです。`get_decision()` 関数が
JSON部分のみを抽出するようになっています。

古いバージョンのテストスクリプトを使用している場合は、最新版に更新してください。

### 問題4: Worktree外へのアクセスがブロックされすぎる

**症状**: 正当なWorktree内の操作がブロックされる

**確認事項**:

1. Worktreeルートパスを確認
   ```bash
   git rev-parse --show-toplevel
   ```

2. 現在のディレクトリがWorktree内か確認
   ```bash
   pwd
   realpath .
   ```

3. シンボリックリンクの解決を確認
   ```bash
   # Hookスクリプトはrealpathで解決しています
   realpath /path/to/check
   ```

**解決方法**:

Hookスクリプトは `realpath` を使用してシンボリックリンクを解決します。
シンボリックリンク経由でWorktree外にアクセスしようとしている場合は、
絶対パスを使用してコマンドを実行してください。

## パフォーマンス確認

Hookスクリプトの実行時間が100ms未満であることを確認:

```bash
# block-git-branch-ops.shのパフォーマンス測定
time echo '{"tool_name":"Bash","tool_input":{"command":"git branch"}}' | \
  .claude/hooks/block-git-branch-ops.sh

# block-cd-command.shのパフォーマンス測定
time echo '{"tool_name":"Bash","tool_input":{"command":"cd ."}}' | \
  .claude/hooks/block-cd-command.sh
```

期待される実行時間: < 100ms（通常は10-50ms程度）

## 次のステップ

1. **CI/CD統合**: GitHub Actionsワークフローでテストを自動実行（T007-T008）
2. **Makefileターゲット**: `make test-hooks` でテストを実行（T009）
3. **パフォーマンステスト**: ベンチマークスクリプトで性能を測定（T010）
4. **ドキュメント更新**: README.mdとCLAUDE.mdに機能説明を追加（T011-T012）

## サポート

問題が解決しない場合は、以下を確認してください:

- [SPEC-dc648675/spec.md](spec.md): 機能仕様書
- [SPEC-dc648675/plan.md](plan.md): 実装計画
- [GitHub Issues](https://github.com/akiojin/ollama-coordinator/issues): 既知の問題
