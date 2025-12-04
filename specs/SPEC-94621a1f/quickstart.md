# ノード自己登録システム クイックスタート

**SPEC-ID**: SPEC-94621a1f
**ステータス**: ✅ 実装済み

## 前提条件

- Rust 1.75+ がインストールされている
- ルーターが起動している（`cargo run --bin coordinator`）

## ノード登録手順

### 1. ルーター起動

```bash
cargo run --manifest-path coordinator/Cargo.toml --bin coordinator
```

**期待される出力**:
```
INFO coordinator: Starting LLM Router on 0.0.0.0:8080
INFO coordinator: Agent registry initialized
```

### 2. ノード登録（curlで確認）

```bash
curl -X POST http://localhost:8080/api/agents \
  -H "Content-Type: application/json" \
  -d '{
    "hostname": "server-01",
    "ip_address": "192.168.1.10",
    "port": 11434,
    "runtime_version": "0.1.23"
  }'
```

**期待されるレスポンス**:
```json
{
  "status": "Success",
  "agent_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "message": "Agent registered successfully"
}
```

### 3. ノード一覧確認

```bash
curl http://localhost:8080/api/agents
```

**期待されるレスポンス**:
```json
[
  {
    "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "hostname": "server-01",
    "ip_address": "192.168.1.10",
    "port": 11434,
    "runtime_version": "0.1.23",
    "status": "Online",
    "last_heartbeat": "2025-10-30T12:00:00Z",
    "registered_at": "2025-10-30T10:00:00Z"
  }
]
```

### 4. ハートビート送信

```bash
AGENT_ID="a1b2c3d4-e5f6-7890-abcd-ef1234567890"

curl -X POST http://localhost:8080/api/agents/${AGENT_ID}/heartbeat \
  -H "Content-Type: application/json" \
  -d "{\"agent_id\": \"${AGENT_ID}\"}"
```

**期待されるレスポンス**: 204 No Content

## テストシナリオ

### シナリオ1: 基本登録フロー

| ステップ | アクション | 期待される結果 |
|---------|-----------|--------------|
| 1 | ルーター起動 | ポート8080でリッスン |
| 2 | POST /api/agents でノード登録 | 200 OK, agent_id返却 |
| 3 | GET /api/agents でノード一覧取得 | 登録したノードが含まれる |

### シナリオ2: 永続化確認

| ステップ | アクション | 期待される結果 |
|---------|-----------|--------------|
| 1 | ノード登録 | 成功 |
| 2 | `~/.llm-router/agents.json` を確認 | ノード情報が保存されている |
| 3 | ルーター再起動 | - |
| 4 | GET /api/agents | 登録済みノード情報が保持されている |

### シナリオ3: ハートビート＆タイムアウト

| ステップ | アクション | 期待される結果 |
|---------|-----------|--------------|
| 1 | ノード登録 | status = "Online" |
| 2 | 30秒ごとにハートビート送信 | status = "Online" 維持 |
| 3 | ハートビート停止 | - |
| 4 | 60秒待機 | status = "Offline" に変化（SPEC-443acc8c） |

## トラブルシューティング

### ノード登録に失敗

**症状**: POST /api/agents が 500 エラー

**原因と対処**:
1. **ストレージディレクトリが作成できない**
   - `~/.llm-router/` の書き込み権限を確認
   - `mkdir -p ~/.llm-router` で手動作成

2. **JSONパース失敗**
   - リクエストボディのJSONフォーマットを確認
   - Content-Typeヘッダーが `application/json` であることを確認

### ノード一覧が空

**症状**: GET /api/agents が空配列を返す

**原因と対処**:
1. **ノードが登録されていない**
   - POST /api/agents で登録
   - `~/.llm-router/agents.json` の内容を確認

2. **ストレージファイルが読み込めない**
   - ファイル権限を確認
   - ファイルのJSON形式が正しいか確認

## 統合テスト実行

```bash
# 統合テストを実行（作業ディレクトリを変更せずに実行するため --manifest-path を指定）
cargo test --manifest-path coordinator/Cargo.toml --test agent_test
```

**期待される出力**:
```
running 5 tests
test test_agent_registration ... ok
test test_heartbeat ... ok
test test_agent_persistence ... ok
test test_agent_list ... ok
test test_agent_timeout ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

## 次のステップ

1. **SPEC-63acef08**: 統一APIプロキシ実装（ノード選択＆ルーティング）
2. **SPEC-443acc8c**: ヘルスチェックシステム実装（自動Offline検出）
3. **SPEC-712c20cf**: 管理ダッシュボード実装（ノード可視化）
