# クイックスタートガイド: 完全自動化リリースシステム

**機能ID**: `SPEC-ee2aa3ef` | **日付**: 2025-11-05 | **仕様**: [spec.md](./spec.md)

このガイドでは、完全自動化リリースシステムの使い方を実践的に説明します。

## 前提条件

### 必須環境

- [x] **GitHub CLI (gh)** がインストール済み
  ```bash
  gh --version  # v2.0.0以上
  ```

- [x] **Git** が設定済み
  ```bash
  git config user.name
  git config user.email
  ```

- [x] **Node.js 20+** がインストール済み（semantic-release実行用）
  ```bash
  node --version  # v20.0.0以上
  ```

- [x] **Rust toolchain** がインストール済み
  ```bash
  rustc --version  # 1.75以上
  ```

### リポジトリ状態

- [x] **developブランチ** が存在する
  ```bash
  git branch -r | grep origin/develop
  ```

  **developブランチが存在しない場合の作成手順**（メンテナ実施）:

  ```bash
  # 1. mainブランチから分岐してdevelopを作成
  git checkout main
  git pull origin main
  git checkout -b develop

  # 2. リモートにプッシュ
  git push -u origin develop

  # 3. ブランチ保護設定（GitHub Web UI）
  #    Settings → Branches → Add branch protection rule
  #    - Branch name pattern: develop
  #    - ✅ Require a pull request before merging
  #    - ✅ Require status checks to pass before merging
  #      - quality-checks

  # 4. 確認
  git branch -r | grep origin/develop
  ```

- [x] **mainブランチ** が存在する（通常は既存）
  ```bash
  git branch -r | grep origin/main
  ```

### 権限確認

- [x] GitHub リポジトリへの **push 権限**
- [x] **PR作成権限**
- [x] **GitHub Actions** が有効

## シナリオ1: 日常開発とアルファ版リリース

**所要時間**: 5分以内でアルファ版リリース完了

### 1. 機能ブランチで開発

```bash
# 現在のブランチを確認
git branch --show-current
# 例: feature/new-feature

# 開発作業を実施
# ... コード編集 ...

# ローカル品質チェック（必須）
make quality-checks

# コミット（Conventional Commits形式）
git commit -m "feat(core): 新機能Xを追加"
git push origin feature/new-feature
```

### 2. develop へのPR作成

```bash
# GitHub上でPR作成（mainではなくdevelopベースで）
gh pr create --base develop --head feature/new-feature \
  --title "feat: 新機能X追加" \
  --body "新機能Xの実装

## 変更内容
- 機能Aを追加
- テストBを追加

## テスト
- [x] make quality-checks 合格
"
```

または GitHub Web UIで：

1. "New pull request" をクリック
2. **base: develop** を選択（重要）
3. **compare: feature/new-feature** を選択
4. PR作成

### 3. 自動品質チェックとマージ

```bash
# PR作成後、GitHub Actionsが自動実行:
# ✅ cargo fmt --check
# ✅ cargo clippy
# ✅ cargo test
# ✅ commitlint
# ✅ markdownlint

# 全チェック合格後、auto-mergeが自動実行
# → developブランチへマージ
```

### 4. アルファ版リリース確認

```bash
# developへのマージ後、semantic-releaseが自動実行
# → v1.2.3-alpha.1 形式のタグ作成
# → GitHub Releaseページに公開

# リリースを確認
gh release list
# v1.2.3-alpha.1  Latest  2025-11-05 (prerelease)

# CHANGELOG確認
git pull origin develop
cat CHANGELOG.md
```

**期待結果**:

- ✅ アルファ版リリース（v1.2.3-alpha.N）が作成される
- ✅ CHANGELOG.md が自動更新される
- ✅ Cargo.toml のバージョンが更新される
- ❌ バイナリは添付されない（developブランチでは省略）

## シナリオ2: 正式リリースの開始

**所要時間**: 30分以内（バイナリビルド含む）

### 1. リリース準備の確認

```bash
# developブランチの状態確認
git checkout develop
git pull origin develop

# 最新のアルファ版を確認
gh release list

# CHANGELOG確認
cat CHANGELOG.md
```

### 2. リリースPR作成（/releaseコマンド使用）

**Claude Codeを使用する場合**:

```
/release
```

**または直接スクリプト実行**:

```bash
# develop → main PR を自動作成
./scripts/release/create-release-pr.sh

# 実行内容:
# ✅ 前提条件チェック（gh, develop, main）
# ✅ developとmainをorigin同期
# ✅ 既存PR確認（重複防止）
# ✅ PRテンプレート生成（コミット一覧、CHANGELOG抽出）
# ✅ PR作成（release, auto-merge ラベル付与）

# 出力例:
# ✅ develop → main PR を作成しました！
# 📎 PR URL: https://github.com/owner/repo/pull/123
#
# 次のステップ:
#   1. PR を確認してください
#   2. 品質チェック（GitHub Actions）の合格を待ちます
#   3. 全チェック合格後、自動マージされます
#   4. mainマージ後、正式版リリース（v1.3.0）が自動作成されます
```

### 3. PR確認と自動マージ

```bash
# GitHub Web UIでPRを確認
# - コミット一覧
# - CHANGELOG差分
# - バージョン種別（パッチ/マイナー/メジャー）

# GitHub Actionsが品質チェックを実行（5-10分）
# ✅ cargo test（複数OS）
# ✅ cargo fmt / clippy
# ✅ commitlint
# ✅ markdownlint

# 全チェック合格後、auto-mergeが自動実行
# → mainブランチへマージ
```

### 4. 正式版リリース確認

```bash
# mainマージ後、semantic-releaseとバイナリビルドが実行（20-30分）
# 1. semantic-release: バージョン計算、タグ作成
# 2. release-binaries: 5プラットフォームのバイナリビルド
#    - Linux x86_64
#    - Windows x86_64
#    - macOS x86_64
#    - macOS ARM64
# 3. GitHub Releaseに全成果物を添付

# リリースを確認
gh release view v1.3.0

# バイナリ確認
gh release download v1.3.0 --pattern "*.tar.gz"
gh release download v1.3.0 --pattern "*.zip"

# CHANGELOG確認
git checkout main
git pull origin main
cat CHANGELOG.md
```

**期待結果**:

- ✅ 正式版リリース（v1.3.0形式）が作成される
- ✅ 5プラットフォームのバイナリが添付される
- ✅ CHANGELOG.md が更新される
- ✅ Cargo.toml のバージョンが更新される

## シナリオ3: 緊急修正のリリース

**所要時間**: 10分以内（パッチ版リリース）

### 1. ホットフィックスブランチ作成（/hotfixコマンド使用）

**Claude Codeを使用する場合**:

```
/hotfix
```

**または直接スクリプト実行**:

**パターンA: Issue番号を指定**

```bash
./scripts/release/create-hotfix.sh 456

# 実行内容:
# ✅ 前提条件チェック（main存在、クリーンな作業ツリー）
# ✅ hotfix/456 ブランチ作成（mainから分岐）
# ✅ ブランチ切り替え

# 出力例:
# ✅ ホットフィックスブランチを作成しました！
# 📌 ブランチ: hotfix/456
#
# 次のステップ:
#   1. 緊急修正を実装してコミット
#      git commit -m "fix: 緊急修正の説明"
#   2. ローカル品質チェックを実行
#      make quality-checks
#   3. リモートにプッシュ
#      git push -u origin hotfix/456
#   4. main へのPR作成
#      gh pr create --base main --head hotfix/456
```

**パターンB: 説明を指定**

```bash
./scripts/release/create-hotfix.sh "critical-auth-bug"

# hotfix/critical-auth-bug ブランチ作成
```

**パターンC: 対話式**

```bash
./scripts/release/create-hotfix.sh

# プロンプト表示:
# Issue番号またはブランチ名を入力してください (例: 123, auth-fix):
# [入力] → 789
# hotfix/789 ブランチ作成
```

### 2. 修正を実装

```bash
# 現在のブランチ確認
git branch --show-current
# hotfix/456

# 緊急修正を実装
# ... コード編集 ...

# ローカル品質チェック（必須）
make quality-checks

# コミット
git commit -m "fix(core): クリティカルなバグXを修正

closes #456
"

# プッシュ
git push -u origin hotfix/456
```

### 3. main へのPR作成

```bash
# PRを作成（mainベース）
gh pr create --base main --head hotfix/456 \
  --title "fix: クリティカルなバグXを修正" \
  --body "緊急修正

## 問題
- 本番環境でバグXが発生

## 解決策
- Yを修正

## テスト
- [x] make quality-checks 合格

Closes #456
" \
  --label "hotfix,auto-merge"
```

### 4. 自動マージとパッチリリース

```bash
# PR作成後、GitHub Actionsが品質チェック実行
# ✅ 全チェック合格後、auto-mergeが自動実行
# → mainブランチへマージ

# mainマージ後、semantic-releaseが即座に実行
# → v1.2.4 形式のパッチリリース作成
# → バイナリビルド・添付（20-30分）

# リリースを確認
gh release view v1.2.4
```

**期待結果**:

- ✅ パッチ版リリース（v1.2.4形式）が作成される
- ✅ 5プラットフォームのバイナリが添付される
- ✅ mainとdevelopが同期される（自動）

## トラブルシューティング

### Q1. PRが自動マージされない

**原因**: 品質チェックが失敗している

**解決策**:

```bash
# GitHub ActionsのログでエラーDetails確認
gh pr checks

# 失敗したチェックを特定
# 例: cargo clippy失敗 → コードを修正

git commit -m "fix: clippy警告を修正"
git push

# 自動的に再チェックが実行される
```

### Q2. semantic-releaseがバージョンを生成しない

**原因**: Conventional Commits形式でないコミットが含まれる

**解決策**:

```bash
# コミットメッセージを確認
git log origin/main..HEAD --oneline

# 不正なコミット例:
# ❌ "updated code"
# ❌ "WIP: testing"

# 修正方法:
git rebase -i origin/main
# エディタで不正なコミットメッセージを修正:
# feat(core): update code
# chore: add test

git push --force-with-lease
```

### Q3. バイナリがリリースに添付されない

**原因**: developブランチではバイナリビルドをスキップ

**解決策**:

```bash
# developブランチではアルファ版のみ（バイナリなし）
# 正式版リリースが必要な場合:

./scripts/release/create-release-pr.sh
# develop → main PR作成 → mainマージでバイナリ添付
```

### Q4. create-release-pr.sh が "既にPRが存在します" エラー

**原因**: develop → main PR が既に開いている

**解決策**:

```bash
# 既存PRを確認
gh pr list --base main --head develop

# 既存PRをマージまたはクローズしてから再実行
gh pr view 123  # 既存PRの状態確認
```

### Q5. ホットフィックスブランチ作成失敗

**原因**: 作業ツリーがクリーンでない

**解決策**:

```bash
# 未コミット変更を確認
git status

# 変更をコミットまたはstash
git stash

# ホットフィックス作成
./scripts/release/create-hotfix.sh 456

# 後で変更を復元
git stash pop
```

## バージョニングルール

semantic-releaseは以下のルールで自動計算します：

| コミットタイプ | バージョン変化 | 例 |
|---------------|--------------|-----|
| `fix:` | パッチ (+0.0.1) | v1.2.3 → v1.2.4 |
| `feat:` | マイナー (+0.1.0) | v1.2.3 → v1.3.0 |
| `BREAKING CHANGE:` | メジャー (+1.0.0) | v1.2.3 → v2.0.0 |
| `chore:`, `docs:` | リリースなし | - |

**例**:

```bash
# パッチ版リリース
git commit -m "fix(api): エラーハンドリング改善"
# → v1.2.4

# マイナー版リリース
git commit -m "feat(cli): 新コマンド追加"
# → v1.3.0

# メジャー版リリース
git commit -m "feat(core)!: APIを刷新

BREAKING CHANGE: APIエンドポイントを変更"
# → v2.0.0
```

## 品質チェック項目

PRマージ前に以下が自動実行されます：

```bash
# ローカルで事前確認（推奨）
make quality-checks

# 個別実行:
cargo fmt --check           # コードフォーマット
cargo clippy -- -D warnings # Lintチェック
cargo test --workspace      # 全テスト実行
make openai-tests           # OpenAI互換APIテスト
npx markdownlint-cli '**/*.md'  # マークダウンLint

# コミットメッセージ検証
.specify/scripts/checks/check-commits.sh --from origin/main --to HEAD
```

## ベストプラクティス

### 1. コミットメッセージ

✅ **良い例**:

```
feat(core): エージェント登録機能を追加

- GPU情報検証を強化
- エラーメッセージを改善

Closes #123
```

❌ **悪い例**:

```
update code
WIP
fix bug
```

### 2. PR作成タイミング

- **feature → develop**: 機能完成時（アルファ版リリース）
- **develop → main**: 複数機能統合後（正式版リリース）
- **hotfix → main**: 緊急修正時（パッチ版リリース）

### 3. ローカル検証

```bash
# PRを作成する前に必ず実行
make quality-checks

# 失敗した場合は修正してから再実行
# CI失敗を防ぐことで開発効率が向上
```

### 4. ブランチ保護

- **main**: developまたはhotfix/**からのみマージ可能
- **develop**: feature/**からのみマージ可能
- **feature/hotfix**: 自由に作成可能

## 次のステップ

1. **初回セットアップ**: [plan.md](./plan.md) でシステム構成を理解
2. **タスク詳細**: [tasks.md](./tasks.md) で実装済みタスクを確認
3. **設計ドキュメント**: [spec.md](./spec.md) で要件を確認

## サポート

問題が発生した場合：

1. [GitHub Issues](../../issues) で質問
2. `.github/workflows/` のActionsログを確認
3. `scripts/release/` のスクリプトを直接実行してデバッグ

---

*クイックスタートガイド - 実装完了後の遡及的文書化 - 2025-11-05*
