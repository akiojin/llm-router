---
description: リリースPRを自動生成し、CI成功で自動マージ→タグ/Release→配信まで流すワークフローをトリガーします。
tags: [project]
---

# リリースコマンド

`create-release.yml` を実行して release-please の **リリースPR（バージョンアップ込み）** を作成します。PR は CI が通ると自動マージされ、main でタグと GitHub Release が作成され、タグ push をトリガーに配信・バックマージが走ります。ローカルでのブランチ操作は不要です。

## 使い方

```bash
scripts/create-release-branch.sh
```

## 注意

- GitHub CLI で認証済みであること（`gh auth login`）。
- リリース対象の変更が main に含まれていることを確認してから実行してください（release PR は main ベースで作られます）。
