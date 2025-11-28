# 共通ログシステム タスク一覧

## Setup

- [x] SPEC-799b8e2bディレクトリ作成
- [x] spec.md作成
- [x] plan.md作成
- [x] tasks.md作成

## Router側

### Test (TDD Red)

- [x] ログファイル日付ローテーションのテスト作成
- [x] 古いログファイル削除のテスト作成
- [x] ログフォーマット（category含む）のテスト作成
- [x] 環境変数設定のテスト作成

### Core (TDD Green)

- [x] [P] ログディレクトリを`~/.llm-router/logs/`に変更
- [x] [P] ファイル名を`llm-router.jsonl.YYYY-MM-DD`に変更
- [x] 日付ベースローテーション実装
- [x] 7日超の古いファイル削除実装
- [x] 新環境変数サポート（LLM_LOG_DIR, LLM_LOG_LEVEL, LLM_LOG_RETENTION_DAYS）
- [x] categoryフィールド出力対応
- [x] [P] stdout出力追加（人間が読みやすい形式）

## Node側

### Test (TDD Red)

- [x] ログファイル日付ローテーションのテスト作成
- [x] 古いログファイル削除のテスト作成
- [x] ログフォーマット（category含む）のテスト作成
- [x] 環境変数設定のテスト作成

### Core (TDD Green)

- [x] [P] ログディレクトリを`~/.llm-router/logs/`に変更
- [x] [P] ファイル名を`llm-node.jsonl.YYYY-MM-DD`に変更
- [x] [P] stdout出力追加（人間が読みやすい形式）
- [x] 日付ベースローテーション実装（daily_file_sink）
- [x] 7日超の古いファイル削除実装
- [x] 新環境変数サポート
- [x] categoryフィールド出力対応

## Integration

- [ ] Router/Node同時起動でログ出力確認
- [ ] /api/logsエンドポイント動作確認

## Polish

- [ ] コードレビュー
- [ ] ドキュメント更新（CLAUDE.md）
- [x] 品質チェック実行
