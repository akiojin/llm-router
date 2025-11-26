---
description: develop→mainへのマージ→リリースPR作成→タグ/Release→配信までの一連のリリースフローをトリガーします。
tags: [project]
---

# リリースコマンド

`prepare-release.yml` を実行して develop → main へのマージを行い、その後 release-please が
リリースPR（バージョン・CHANGELOG更新込み）を作成します。
リリースPRがマージされるとタグと GitHub Release が作成され、タグ push をトリガーに配信が走ります。

## フロー

```text
/release 実行
    ↓
① develop → main マージ (prepare-release.yml)
    ↓
② release-please がリリースPR作成 (release.yml)
    ↓
③ リリースPRマージ → タグ作成 → 配布 (publish.yml)
```

## 使い方

GitHub CLIで直接ワークフローを実行します：

```bash
gh workflow run prepare-release.yml
```

## 注意

- GitHub CLI で認証済みであること（`gh auth login`）
- リリース対象の変更が develop に含まれていることを確認してから実行してください
- main ブランチへの直接プッシュは禁止されています（PR必須）
