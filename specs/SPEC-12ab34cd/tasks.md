# タスク: 自動マージテスト (全チェック合格)

**機能ID**: `SPEC-12ab34cd` | **テスト目的**: T011 - 全チェック合格後の自動マージ検証

## テストタスク

- [x] **T001** テストタスク1 - ダミータスク
- [x] **T002** テストタスク2 - ダミータスク
- [x] **T003** テストタスク3 - ダミータスク

## 期待される動作

1. このPRが作成されると、GitHub Actions「Quality Checks」が実行される
2. すべてのチェック（tasks-check, rust-test, rust-lint, commitlint, markdownlint）が成功する
3. GitHub Actions「Auto Merge」が起動する
4. PRが自動的にmainブランチにマージされる

## 検証項目

1. quality-checksワークフローが成功
2. auto-mergeワークフローが起動
3. PRがマージされた
