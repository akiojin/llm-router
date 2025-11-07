feature → develop → main の「方法A」リリースプロセスを起動します。

## フロー概要

1. `develop` で `/release` を実行すると `scripts/create-release-pr.sh` が走り、最新の `develop` を取得したうえで `develop → main` のリリースPRを自動生成します。
2. PRには Required チェック（品質検証、specタスク、commitlint 等）が自動付与され、全てGreenになった時点でメンテナがマージします。
3. `main` へのマージをトリガーに `.github/workflows/release.yml` が semantic-release を起動し、以下をGitHub Actions特権で実行します。
   - Conventional Commitsから次バージョン決定
   - `package.json` / `package-lock.json` / `CHANGELOG.md` を更新して `main` に直接コミット
   - タグ（例: `v1.2.0`）と GitHub Release 作成
   - 必要に応じて npm publish（`.releaserc.json` の `npmPublish` を `true` にし `NPM_TOKEN` を登録）
   - 成功後に `main` → `develop` を自動マージ（衝突時は `sync/main-to-develop-<timestamp>` PRを発行）

## 前提条件

- 現在のブランチ: `develop`
- 作業ツリーがクリーンであること（`git status` が clean）
- GitHub CLI `gh` で認証済み（`gh auth status`）
- リモート `origin` に `develop` / `main` が存在すること

## コマンド実行手順

```bash
# develop ブランチ上で
./scripts/create-release-pr.sh
```

もしくは Claude Code から：

```
/release
```

スクリプトが行う処理:

1. `develop` にいるか検証（それ以外なら即停止）
2. `git pull origin develop` で最新化
3. `gh pr create --base main --head develop --title "Release: <YYYY-MM-DD>"` を実行し、説明付きのPRを作成
4. 成功時に「Release PR created successfully」を表示

## トラブルシューティング

### develop でないブランチにいる
`git switch develop` の後に再度 `/release` を実行してください。

### gh CLI で認証エラーが出る
`gh auth login` で PAT もしくはブラウザ連携を済ませてから再実行します。

### 既存の develop→main PR が残っている
スクリプトは既存PRを上書きしないため、まず既存PRをクローズ/マージしてから再実行してください。

---

準備が整っていれば、このプロンプトを確認後に `/release` を実行してください。
