---
description: developからrelease/vX.Y.Zブランチを作成し、リリースフローを開始します。
tags: [project]
---

# リリースコマンド

GitHub Actions の `create-release.yml` ワークフローを起動し、`akiojin/unity-mcp-server` と同じ release ブランチ方式で正式リリースを開始します。追加の PR 操作や手動マージは不要です。

## フロー概要

1. `/release` で `scripts/create-release-branch.sh` を実行し、`gh workflow run create-release.yml --ref develop` を呼び出す。
2. ワークフローが `develop` で semantic-release のドライランを実行し、次のバージョンを決定して `release/vX.Y.Z` ブランチを作成・push。
3. `release/vX.Y.Z` の push をトリガーに `.github/workflows/release.yml` が起動し、semantic-release 本番、CHANGELOG/Cargo.toml 更新、タグ作成、main への自動マージ、develop へのバックマージ、release ブランチ削除を行う。
4. main への push を受けて `.github/workflows/publish.yml` が `release-binaries.yml` を呼び出し、各プラットフォームのバイナリを GitHub Release に添付する。

## 前提条件

- 現在のブランチは `develop`
- GitHub CLI がインストール・認証済み（`gh auth status`）
- develop に Conventional Commits 準拠の変更が揃っている

## 実行方法

```bash
scripts/create-release-branch.sh
```

もしくは Claude Code で `/release` を実行すると、同じスクリプトが呼び出されます。スクリプトは最新の `develop` を前提に `gh workflow run create-release.yml --ref develop` を実行し、リリースブランチ作成と後続フローを自動化します。

## 進捗確認

```bash
gh run watch $(gh run list --workflow=create-release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch $(gh run list --workflow=release.yml --limit 1 --json databaseId --jq '.[0].databaseId')
gh run watch $(gh run list --workflow=publish.yml --limit 1 --json databaseId --jq '.[0].databaseId')
```

## トラブルシューティング

| 症状 | 対応 |
| --- | --- |
| `GitHub CLI is not authenticated` | `gh auth login` を実行してトークンを更新する |
| `release/vX.Y.Z already exists` | 直前の release.yml が進行中。完了後に再実行する |
| `Not authorized to run workflow` | `gh auth refresh -h github.com -s workflow` で `workflow` スコープを再付与 |

準備ができたら `/release` を実行してください。
