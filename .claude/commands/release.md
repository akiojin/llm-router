---
description: developからrelease/vX.Y.Zブランチを作成し、リリースフローを開始します。
tags: [project]
---

# リリースコマンド

developブランチから`release/vX.Y.Z`ブランチを自動作成し、リリースフローを開始します。

## 実行内容

1. 現在のブランチがdevelopであることを確認
2. developブランチを最新に更新（`git pull`）
3. semantic-releaseのドライランで次バージョンを判定
4. `release/vX.Y.Z`ブランチをdevelopから作成
5. リモートにpush
6. GitHub Actionsが以下を自動実行：
   - **releaseブランチ**: semantic-releaseでバージョン決定・CHANGELOG更新・タグ作成・GitHub Release作成、完了後に release ブランチを main へ取り込み、develop へバックマージし、ブランチを削除
   - **mainブランチ**: publish.yml が `release-binaries.yml` を呼び出し、Linux/macOS/Windows 向けのバイナリをビルドして GitHub Release に添付

## 前提条件

- developブランチにいること
- GitHub CLIが認証済みであること（`gh auth login`）
- コミットがすべてConventional Commits形式であること
- semantic-releaseがバージョンアップを判定できるコミットが存在すること

## スクリプト実行

リリースブランチを作成するには、以下を実行します：

```bash
scripts/create-release-branch.sh
```

スクリプトはGitHub Actions の `create-release.yml` を起動し、リモートで以下を実行します：

1. developブランチでsemantic-releaseのドライラン
2. 次バージョン番号の判定
3. `release/vX.Y.Z`ブランチを作成してpush

これにより release.yml → publish.yml → release-binaries.yml が自動的に進みます。準備ができたら `/release` を実行してください。
