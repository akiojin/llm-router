# クイックスタート: モデルファイル破損時の自動修復機能

**機能ID**: `SPEC-3df1b977`

## 概要

モデルファイルが破損している場合、システムが自動的に再ダウンロードして
リクエストを処理する機能です。

## 前提条件

- llm-router ノードが起動していること
- ルーターに接続していること
- インターネット接続があること

## 基本的な動作

### 1. 通常のリクエスト

```bash
curl -X POST http://localhost:8081/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-oss:7b",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

### 2. 破損モデルでのリクエスト

モデルファイルが破損している場合でも、同じリクエストを送信すると:

1. システムが破損を検出
2. 自動的にモデルを再ダウンロード
3. リクエストが正常に処理される

レスポンス時間は通常より長くなりますが、ユーザー操作は不要です。

## 環境変数設定

| 変数名 | デフォルト | 説明 |
|--------|-----------|------|
| `LLM_AUTO_REPAIR` | `true` | 自動修復の有効/無効 |
| `LLM_REPAIR_TIMEOUT_SECS` | `300` | 修復タイムアウト（秒） |

### 自動修復を無効にする

```bash
export LLM_AUTO_REPAIR=false
```

### タイムアウトを変更する

```bash
export LLM_REPAIR_TIMEOUT_SECS=600  # 10分
```

## エラーハンドリング

### 修復失敗時

```json
{
  "error": {
    "message": "Model repair failed: network error",
    "type": "repair_failed",
    "code": "model_repair_error"
  }
}
```

**対処法**: ネットワーク接続を確認し、再試行してください。

### ストレージ不足時

```json
{
  "error": {
    "message": "Insufficient storage for model repair",
    "type": "storage_full",
    "code": "insufficient_storage"
  }
}
```

**対処法**: ディスク容量を確保してください。

### タイムアウト時

```json
{
  "error": {
    "message": "Model repair timed out",
    "type": "repair_timeout",
    "code": "gateway_timeout"
  }
}
```

**対処法**: `LLM_REPAIR_TIMEOUT_SECS`を増やすか、手動でモデルをダウンロードしてください。

## テストシナリオ

### シナリオ1: 破損モデルの自動修復

```bash
# 1. モデルファイルを破損させる（テスト用）
echo "corrupted" > ~/.runtime/models/blobs/sha256-xxxx

# 2. リクエスト送信
curl -X POST http://localhost:8081/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model": "gpt-oss:7b", "messages": [{"role": "user", "content": "test"}]}'

# 3. 自動修復後、正常なレスポンスが返る
```

### シナリオ2: 同時リクエストの処理

```bash
# 複数の同時リクエストを送信
for i in {1..5}; do
  curl -X POST http://localhost:8081/v1/chat/completions \
    -H "Content-Type: application/json" \
    -d '{"model": "gpt-oss:7b", "messages": [{"role": "user", "content": "test"}]}' &
done
wait

# すべてのリクエストが修復完了後に処理される
# 修復は1回のみ実行される
```

## ログ確認

```bash
# 修復ログを確認
grep "auto-repair" /var/log/llm-node.log
```

出力例:

```
[INFO] Starting auto-repair for model: gpt-oss:7b
[INFO] Downloading model: gpt-oss:7b (50%)
[INFO] Auto-repair completed: gpt-oss:7b (elapsed: 120s)
```
