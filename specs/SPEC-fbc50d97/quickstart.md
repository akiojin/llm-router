# クイックスタート: リクエスト/レスポンス履歴保存機能

**機能ID**: SPEC-fbc50d97
**日付**: 2025-11-03

## 概要

この機能は、ルーターが受信するリクエストとノードから返される
レスポンスを自動的に保存し、Webダッシュボードで確認できるようにします。

## 前提条件

- Rust 1.75+ がインストールされていること
- LLM Router が起動していること
- 1つ以上のノードが登録されていること

## 5分で試す

### 1. ルーターを起動

```bash
cargo run --bin coordinator
```

デフォルトで `http://localhost:8080` で起動します。

### 2. テストリクエストを送信

```bash
# Chat リクエスト
curl -X POST http://localhost:8080/api/chat \
  -H "Content-Type: application/json" \
  -d '{
    "model": "llama2",
    "messages": [
      {"role": "user", "content": "Hello, how are you?"}
    ],
    "stream": false
  }'

# Generate リクエスト
curl -X POST http://localhost:8080/api/generate \
  -H "Content-Type: application/json" \
  -d '{
    "model": "codellama",
    "prompt": "Write a hello world in Rust",
    "stream": false
  }'
```

### 3. ダッシュボードで履歴を確認

ブラウザで `http://localhost:8080` にアクセスし、「リクエスト履歴」タブを開きます。

**確認項目**:
- リクエストが時系列順に表示されている
- 各レコードに時刻、モデル名、ノード名、処理時間、ステータスが表示されている
- レコードをクリックすると詳細モーダルが開く

### 4. APIで履歴を取得

```bash
# 一覧取得
curl http://localhost:8080/api/dashboard/request-responses

# フィルタリング（モデル名指定）
curl "http://localhost:8080/api/dashboard/request-responses?model=llama2"

# 特定レコードの詳細取得
curl http://localhost:8080/api/dashboard/request-responses/{id}

# JSON形式でエクスポート
curl "http://localhost:8080/api/dashboard/request-responses/export?format=json" \
  -o history.json

# CSV形式でエクスポート
curl "http://localhost:8080/api/dashboard/request-responses/export?format=csv" \
  -o history.csv
```

### 5. 保存ファイルを確認

```bash
cat ~/.llm-router/request_history.json | jq
```

レコードがJSON配列形式で保存されていることを確認できます。

## ユーザーストーリー検証

### ストーリー1: リクエスト履歴の確認 (P1)

**目的**: オペレーターがリクエストの処理状況を監視できること

**検証手順**:
1. ルーターを起動
2. 複数のリクエストを送信（成功・失敗を含む）
3. ダッシュボードの履歴タブを開く

**期待結果**:
- ✅ 時系列順にリクエスト一覧が表示される
- ✅ 成功/失敗のステータスが視覚的に区別できる（色分け）
- ✅ どのノードが処理したかが表示される

**検証コマンド**:
```bash
# 成功リクエスト
curl -X POST http://localhost:8080/api/chat \
  -H "Content-Type: application/json" \
  -d '{"model": "llama2", "messages": [{"role": "user", "content": "test"}], "stream": false}'

# 失敗リクエスト（存在しないノード）
# → システムで自動的にエラーとなる場合があります

# ダッシュボードで確認
open http://localhost:8080
```

---

### ストーリー2: エラーの詳細調査 (P2)

**目的**: 開発者がエラーの詳細を確認してデバッグできること

**検証手順**:
1. エラーが発生するリクエストを送信
2. ダッシュボードでエラーレコードをクリック
3. 詳細モーダルが開く

**期待結果**:
- ✅ リクエスト本文（プロンプト、パラメータ）が表示される
- ✅ エラーメッセージが表示される
- ✅ JSON形式で見やすく表示される（シンタックスハイライト）

**検証コマンド**:
```bash
# APIで詳細取得
RECORD_ID=$(curl -s http://localhost:8080/api/dashboard/request-responses | jq -r '.records[0].id')
curl http://localhost:8080/api/dashboard/request-responses/$RECORD_ID | jq
```

---

### ストーリー3: 履歴の検索とフィルタリング (P3)

**目的**: 管理者が特定条件で履歴を絞り込めること

**検証手順**:
1. 複数のモデルでリクエストを送信
2. ダッシュボードでフィルタを適用
3. 条件に一致するレコードのみ表示される

**期待結果**:
- ✅ モデル名でフィルタできる
- ✅ ノードでフィルタできる
- ✅ ステータス（成功/失敗）でフィルタできる
- ✅ 日時範囲でフィルタできる

**検証コマンド**:
```bash
# モデル名でフィルタ
curl "http://localhost:8080/api/dashboard/request-responses?model=llama2"

# ステータスでフィルタ
curl "http://localhost:8080/api/dashboard/request-responses?status=success"

# 日時範囲でフィルタ
curl "http://localhost:8080/api/dashboard/request-responses?start_time=2025-11-03T00:00:00Z&end_time=2025-11-03T23:59:59Z"
```

---

### ストーリー4: 履歴データのエクスポート (P3)

**目的**: 管理者が履歴を外部ツールで分析できること

**検証手順**:
1. ダッシュボードでエクスポートボタンをクリック
2. JSON または CSV 形式を選択
3. ファイルがダウンロードされる

**期待結果**:
- ✅ JSON形式でエクスポートできる
- ✅ CSV形式でエクスポートできる
- ✅ フィルタ条件が適用される

**検証コマンド**:
```bash
# JSON エクスポート
curl "http://localhost:8080/api/dashboard/request-responses/export?format=json" \
  -o history.json
cat history.json | jq

# CSV エクスポート
curl "http://localhost:8080/api/dashboard/request-responses/export?format=csv" \
  -o history.csv
head history.csv
```

---

## トラブルシューティング

### 履歴が表示されない

**原因**: リクエストが送信されていない、または保存に失敗

**確認方法**:
```bash
# ファイルの存在確認
ls -lh ~/.llm-router/request_history.json

# ファイルの内容確認
cat ~/.llm-router/request_history.json | jq
```

**対処方法**:
- ルーターのログを確認（`tracing` で出力）
- ストレージディレクトリの書き込み権限を確認

---

### エクスポートが失敗する

**原因**: 不正なクエリパラメータ

**確認方法**:
```bash
# エラーレスポンスを確認
curl -v "http://localhost:8080/api/dashboard/request-responses/export?format=invalid"
```

**対処方法**:
- `format` パラメータは `json` または `csv` のみ
- 日時形式は ISO8601 形式（例: `2025-11-03T10:30:00Z`）

---

### ファイルが肥大化する

**原因**: クリーンアップタスクが動作していない

**確認方法**:
```bash
# ファイルサイズ確認
du -h ~/.llm-router/request_history.json
```

**対処方法**:
- ルーターを再起動（起動時にクリーンアップ実行）
- 手動でファイルを編集（古いレコードを削除）

---

## パフォーマンステスト

### ベンチマーク実行

```bash
# 100リクエストを送信
for i in {1..100}; do
  curl -X POST http://localhost:8080/api/chat \
    -H "Content-Type: application/json" \
    -d '{"model": "llama2", "messages": [{"role": "user", "content": "test"}], "stream": false}' &
done
wait

# レスポンスタイム確認
time curl http://localhost:8080/api/dashboard/request-responses
```

**期待パフォーマンス**:
- プロキシオーバーヘッド: < 5ms
- ダッシュボード初期表示: < 1秒
- 10,000件のフィルタリング: < 300ms

---

## 次のステップ

1. **統合テスト実行**:
   ```bash
   cargo test --test integration_test
   ```

2. **E2Eテスト実行**:
   ```bash
   cargo test --test e2e_test
   ```

3. **すべてのテスト実行**:
   ```bash
   cargo test
   ```

4. **品質チェック**:
   ```bash
   make quality-checks
   ```

---

## 参考資料

- [機能仕様書](./spec.md): ビジネス要件とユーザーストーリー
- [実装計画](./plan.md): 技術設計とアーキテクチャ
- [データモデル](./data-model.md): RequestResponseRecord の詳細
- [API契約](./contracts/dashboard-history-api.json): OpenAPI仕様
