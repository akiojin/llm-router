# クイックスタート: 自動マージ機能（更新版）

**機能ID**: `SPEC-47c6f44c`  
**最終更新日**: 2025-11-07

> ⚠️ **重要**: プロジェクトのカスタム運用ルールにより、開発者が任意でブランチ／Worktreeを作成したり、作業ディレクトリを変更したりすることはできません。本ドキュメントは、自動マージ機能の検証フローと期待される結果を共有する目的で提供しています。実際のブランチ操作やPR作成はすべてリポジトリメンテナが実施します。

## 概要

- 自動マージ機能は `pull_request_target` で動作する `Auto Merge` ワークフローによってGitHubの自動マージを全PRで有効化し、Requiredチェックが通過したタイミングでGitHub自身がMERGEコミットを作成します。
- 開発者は現在の作業環境を変更せず、CIの実行結果を確認・フィードバックする役割を担います。
- テストデータや検証用PRはメンテナが用意したテンプレートを使用し、自動的にクリーンアップされます。

## 運用前提

- メンテナは検証が必要になったタイミングで専用のGitHub Actionsワークフローを手動起動し、テスト用PRを生成します（リポジトリルートの`.github/workflows/quality-checks.yml`／`auto-merge.yml`を利用）。
- テストPRには「Auto Merge QA」というラベルが自動で付与され、完了後にメンテナがクローズ／削除します。
- 開発者は GitHub Actions ダッシュボードで結果を閲覧し、必要に応じてログを共有します。

## テストシナリオ

### シナリオ1: 正常系（全チェック合格 → 自動マージ）

1. **メンテナ**: テンプレートPR生成用ワークフローを起動し、正常系シナリオを選択する。
2. **CI**: `Quality Checks` ワークフローが起動し、tasks-check／rust-test／rust-lint／commitlint／markdownlint が順次成功する。
3. **CI**: PRがReady for reviewになるタイミングで `Auto Merge` ワークフローが起動し、GitHubの「Auto-merge (MERGE)」が自動で有効化される。
4. **開発者**: Requiredチェックが全て完了した後、PRタイムラインで「Auto-merged by GitHub」を確認し、Slackの品質チャンネルへ完了報告を行う。

### シナリオ2: 異常系（未完了タスク → 自動マージスキップ）

1. **メンテナ**: 検証ワークフローを「未完了タスク」モードで実行し、tasks.mdに未完了タスクを含むPRを生成する。
2. **CI**: `Quality Checks` ワークフロー内の `tasks-check` ジョブが失敗し、Requiredチェックが未完了のためGitHubが自動マージを保留する（PR画面には「Auto-merge: Waiting on required checks」が表示される）。
3. **開発者**: Actionsログで `tasks-check` の失敗内容を確認し、対応タスクをIssueに記録する。

### シナリオ3: ドラフトPR（品質チェック実行 → 自動マージスキップ）

1. **メンテナ**: ドラフト状態のPRを生成するモードでワークフローを起動。
2. **CI**: `Quality Checks` は実行されるが、PRがドラフトのため `Auto Merge` ワークフローは「PR is draft」と記録して自動マージを有効化しない。
3. **開発者**: PRのステータスが「Draft」であることと、`Auto Merge` ワークフローに「PR is a draft」と記録されていることを確認する。
4. **メンテナ**: 必要に応じてドラフト解除後に再度ワークフローを起動し、マージ動作を確認する。

### シナリオ4: コミット規約違反（commitlint失敗）

1. **メンテナ**: commitlintに意図的に違反するメッセージを含んだテストPRを生成する。
2. **CI**: `Quality Checks` の `commitlint` ジョブが失敗する。
3. **開発者**: ログを確認し、適切なコミットメッセージ例をPull Requestコメントで共有する。

### シナリオ5: コンフリクト検知（Auto Merge スキップ）

1. **メンテナ**: `main` と競合する変更を含むテストPRを生成。
2. **CI**: `Quality Checks` は成功するが、`Auto Merge` で「PR mergeable state is CONFLICTING」と表示され、GitHubの自動マージが有効化されない。
3. **開発者**: コンフリクト箇所の検討結果をメンテナに共有し、解消方針をIssueに記録する。

## トラブルシューティング

### Auto Mergeワークフローが起動しない

- `auto-merge.yml` が `main` ブランチに存在するかを確認。
- テストPRに `Auto Merge QA` ラベルが自動付与されているかを確認（付与されない場合はメンテナに再実行を依頼）。

### commitlintが失敗した場合

- 違反したコミットメッセージを確認し、Conventional Commitsフォーマットの例をPRコメントに残す。
- メンテナがテストPRを再生成するまで待機する。開発者はローカルで`git commit --amend`等を実行しない。

### tasks-checkが失敗した場合

- Actionsログの失敗箇所を確認し、未完了タスクの抜け漏れをIssueに転記する。
- 対応方針を整理し、メンテナに再検証のタイミングを依頼する。

## 検証チェックリスト

- [ ] 正常系: `Auto Merge` が自動的に有効化され、Requiredチェック合格後にGitHubが自動マージする
- [ ] 未完了タスク: `tasks-check` 失敗でRequiredチェックが満たされず、PR画面に「Auto-merge: Waiting on required checks」と表示される
- [ ] コミット規約違反: `commitlint` が失敗し、メンテナ再実行待ちになる
- [ ] ドラフトPR: `Auto Merge` がドラフト状態を検知してスキップする
- [ ] コンフリクト: `Auto Merge` が「PR mergeable state is CONFLICTING」と出力し、手動解消が必要になる
- [ ] 検証後: メンテナがPR/ブランチをクリーンアップし、ログが共有される

## 次のステップ

1. Actions実行ログと結果サマリを `docs/qa/auto-merge-report.md` に追記する（必要に応じて新規作成可）。
2. 追加のテストシナリオが必要な場合は `specs/SPEC-47c6f44c/tasks.md` にタスクとして記載し、メンテナに依頼する。
3. 自動マージの挙動に変更が入った場合は、本クイックスタートの更新日と変更内容を明記する。
