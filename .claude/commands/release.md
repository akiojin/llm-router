---
description: developからrelease/vX.Y.Zブランチを作成し、リリースフローを開始します。
tags: [project]
---

# リリースコマンド

`release/vX.Y.Z` ブランチを自動作成してリリース用 GitHub Actions をトリガーします。

## 実行内容

1. semantic-release のドライランを実行し次バージョンを判定
2. `release/vX.Y.Z` ブランチを作成してリモートへ push
3. GitHub Actions が以下を自動実行：
   - **releaseブランチ**: semantic-releaseでバージョン決定・CHANGELOG更新・タグ作成・GitHub Release作成、完了後に release ブランチを main へ取り込み、develop へバックマージし、ブランチを削除
   - **mainブランチ**: publish.yml が `release-binaries.yml` を呼び出し、Linux/macOS/Windows 向けのバイナリをビルドして GitHub Release に添付

## 前提条件

- GitHub CLIが認証済みであること（`gh auth login`）
- コミットがすべてConventional Commits形式であること
- semantic-releaseがバージョンアップを判定できるコミットが存在すること

## スクリプト実行

リリースブランチを作成するには、以下を実行します：

```bash
scripts/create-release-branch.sh
```

スクリプトは GitHub Actions の `create-release.yml` を起動し、リモートで次の処理を実行します：

1. semantic-release のドライラン
2. 次バージョン番号の判定
3. `release/vX.Y.Z` ブランチの作成と push

これにより release.yml → publish.yml → release-binaries.yml が自動的に進みます。準備ができたら `/release` を実行してください。
