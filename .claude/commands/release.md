---
description: リリース用GitHub Actionsワークフローをトリガーします。
tags: [project]
---

# リリースコマンド

単にリリース用 GitHub Actions (`create-release.yml`) を叩くだけのシンプルなコマンドです。実行するとリモート側で release ブランチの作成・publish ワークフローまで自動で進むので、ローカルでブランチ操作を行う必要はありません。

## 使い方

```bash
scripts/create-release-branch.sh
```

## 注意

- GitHub CLI で認証済みであること（`gh auth login`）。
- リリース対象コミットが既に main へ取り込まれていることを確認してから実行してください。
