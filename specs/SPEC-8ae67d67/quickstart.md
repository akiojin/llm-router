# クイックスタート: モデル自動配布機能

**機能ID**: `SPEC-8ae67d67`
**最終更新**: 2025-11-14

このガイドでは、ルーター主導のモデル自動配布機能の3つの主要シナリオ
について、実際の操作手順を説明します。

---

## 前提条件

- Coordinatorが起動していること (`coordinator` バイナリ)
- 1台以上のAgentが稼働していること (`agent` バイナリ)
- ノードがGPUを搭載していること
- CoordinatorとAgent間でネットワーク通信が可能であること

---

## シナリオ1: ノード登録時の自動モデル配布

### 概要

新しいノードを登録すると、GPUメモリサイズに応じて最適なモデルが
自動的に選択され、ダウンロードが開始されます。

### GPU メモリとモデルの対応表

| GPU メモリ | 自動選択モデル |
|-----------|--------------|
| 16GB以上 | gpt-oss:20b |
| 8GB〜16GB | gpt-oss:7b |
| 4.5GB〜8GB | gpt-oss:3b |
| 4.5GB未満 | gpt-oss:1b |

### 操作手順

#### 1. ノードを起動

```bash
# Agent側で実行
./agent \
  --coordinator-url http://localhost:8080 \
  --runtime-port 11434 \
  --agent-api-port 11435
```

#### 2. 自動登録の確認

ノード起動時に、以下のログが表示されます：

```
INFO Agent registration successful: agent_id=...
INFO Auto-distribution started: model=gpt-oss:20b, task_id=...
INFO Model pull started: model=gpt-oss:20b
```

#### 3. 進捗の確認

CoordinatorのAPIで進捗を確認できます：

```bash
# タスクIDを使用して進捗を取得
curl http://localhost:8080/api/tasks/{task_id}
```

レスポンス例:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "agent_id": "123e4567-e89b-12d3-a456-426614174000",
  "model_name": "gpt-oss:20b",
  "status": "downloading",
  "progress": 0.45,
  "download_speed_bps": 10485760,
  "created_at": "2025-11-14T10:00:00Z",
  "updated_at": "2025-11-14T10:05:30Z"
}
```

#### 4. 完了の確認

ダウンロード完了時のログ:

```
INFO Task completed: task_id=550e8400-e29b-41d4-a716-446655440000
INFO Model pull completed: model=gpt-oss:20b, task_id=...
```

---

## シナリオ2: 手動でのモデル配布

### 概要

管理者が明示的に特定のモデルを特定のノード（または全ノード）に
配布します。新しいモデルのテストや、特定タスク向けモデルの配布に使用します。

### 操作手順

#### 1. 利用可能なモデル一覧を取得

```bash
curl http://localhost:8080/api/models/available
```

レスポンス例:

```json
{
  "models": [
    {
      "name": "llama3.2:3b",
      "display_name": "Llama 3.2 (3B)",
      "size_gb": 2.0,
      "description": "Meta's Llama 3.2 model, 3 billion parameters"
    },
    {
      "name": "mistral:7b",
      "display_name": "Mistral (7B)",
      "size_gb": 4.1,
      "description": "Mistral AI's 7B parameter model"
    }
  ],
  "source": "runtime_library"
}
```

#### 2. 特定ノードへのモデル配布

```bash
curl -X POST http://localhost:8080/api/agents/{agent_id}/models/pull \
  -H "Content-Type: application/json" \
  -d '{
    "model_name": "llama3.2:3b"
  }'
```

レスポンス例:

```json
{
  "task_id": "660e9400-f39c-42e4-b827-556766550111"
}
```

#### 3. 全ノードへの一括配布

```bash
curl -X POST http://localhost:8080/api/models/distribute \
  -H "Content-Type: application/json" \
  -d '{
    "model_name": "mistral:7b",
    "target": "all"
  }'
```

レスポンス例:

```json
{
  "task_ids": [
    "770ea500-g49d-43f5-c938-667877661222",
    "880fb611-h59e-54g6-d049-778988772333",
    "990gc722-i69f-65h7-e150-889099883444"
  ]
}
```

#### 4. 特定ノード群への配布

```bash
curl -X POST http://localhost:8080/api/models/distribute \
  -H "Content-Type: application/json" \
  -d '{
    "model_name": "phi3:mini",
    "target": "specific",
    "agent_ids": [
      "123e4567-e89b-12d3-a456-426614174000",
      "234f5678-f90c-23e4-b567-537725285111"
    ]
  }'
```

---

## シナリオ3: モデル情報の可視化

### 概要

システム全体のモデル配布状況を確認し、どのノードにどのモデルが
インストールされているかを把握します。

### 操作手順

#### 1. 利用可能なモデル一覧の確認

```bash
curl http://localhost:8080/api/models/available
```

- LLM runtime公式ライブラリから取得したモデル一覧が表示されます
- モデル名、表示名、サイズ、説明が含まれます

#### 2. 特定ノードのインストール済みモデルを確認

```bash
curl http://localhost:8080/api/agents/{agent_id}/models
```

レスポンス例:

```json
[
  {
    "name": "gpt-oss:20b",
    "size_gb": 12.5,
    "installed_at": "2025-11-14T10:00:00Z"
  },
  {
    "name": "llama3.2:3b",
    "size_gb": 2.0,
    "installed_at": "2025-11-14T11:30:00Z"
  }
]
```

#### 3. 全ノードの状況をマトリクス形式で確認

```bash
# ノード一覧を取得
curl http://localhost:8080/api/agents

# 各ノードのモデルを確認
for agent_id in $(curl -s http://localhost:8080/api/agents | jq -r '.[].id'); do
  echo "Agent: $agent_id"
  curl -s http://localhost:8080/api/agents/$agent_id/models | jq '.[] | .name'
done
```

出力例:

```
Agent: 123e4567-e89b-12d3-a456-426614174000
"gpt-oss:20b"
"llama3.2:3b"

Agent: 234f5678-f90c-23e4-b567-537725285111
"gpt-oss:7b"
"mistral:7b"
```

---

## エラーハンドリング

### オフラインノードへの配布試行

```bash
curl -X POST http://localhost:8080/api/agents/{offline_agent_id}/models/pull \
  -H "Content-Type: application/json" \
  -d '{"model_name": "llama3.2"}'
```

エラーレスポンス:

```json
{
  "error": "ノード {agent_id} はオフラインです"
}
```

**HTTP ステータス**: 503 Service Unavailable

### 無効なモデル名の指定

```bash
curl -X POST http://localhost:8080/api/models/distribute \
  -H "Content-Type: application/json" \
  -d '{
    "model_name": "Invalid@Model#Name",
    "target": "all"
  }'
```

エラーレスポンス:

```json
{
  "error": "無効なモデル名: Invalid@Model#Name"
}
```

**HTTP ステータス**: 400 Bad Request

### ディスク容量不足（将来実装予定）

```json
{
  "error": "ストレージ容量不足: 必要容量 12.5GB、空き容量 8.2GB"
}
```

**HTTP ステータス**: 507 Insufficient Storage

---

## トラブルシューティング

### 問題: ノード登録時にモデルダウンロードが開始されない

**原因**:

- GPUメモリ情報が正しく検出されていない
- ノード側のAPI通信が失敗している

**解決方法**:

1. ノードのログで GPU 検出状況を確認:

   ```
   INFO GPU detected: model=NVIDIA GeForce RTX 3090, memory=24GB
   ```

2. ネットワーク疎通を確認:

   ```bash
   curl http://{agent_ip}:11435/health
   ```

3. Coordinatorのログでエラーを確認:

   ```
   ERROR Failed to send pull request to agent {agent_id}: connection refused
   ```

### 問題: ダウンロード進捗が0%から進まない

**原因**:

- ノード側でLLM runtimeサービスが起動していない
- ネットワーク帯域が不足している

**解決方法**:

1. LLM runtimeサービスの起動を確認:

   ```bash
   systemctl status runtime
   # または
   ps aux | grep runtime
   ```

2. ダウンロード速度を確認:

   ```bash
   curl http://localhost:8080/api/tasks/{task_id} | jq '.download_speed_bps'
   ```

---

## 次のステップ

- [仕様書全文](spec.md) - 詳細な要件とエッジケース
- [実装計画](plan.md) - 技術設計とアーキテクチャ
- [タスクリスト](tasks.md) - 実装タスクの詳細

---

**🤖 Generated with [Claude Code](https://claude.com/claude-code)**
