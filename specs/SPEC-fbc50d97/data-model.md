# データモデル

**機能**: リクエスト/レスポンス履歴保存機能
**日付**: 2025-11-03

## エンティティ定義

### RequestResponseRecord

リクエスト/レスポンスの1つのトランザクションを表すレコード。

**場所**: `common/src/protocol.rs`

**構造体定義**:
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestResponseRecord {
    /// レコードの一意識別子
    pub id: Uuid,

    /// リクエスト受信時刻
    pub timestamp: DateTime<Utc>,

    /// リクエストタイプ（Chat または Generate）
    pub request_type: RequestType,

    /// 使用されたモデル名（例: "llama2"）
    pub model: String,

    /// 処理したノードのID
    pub agent_id: Uuid,

    /// ノードのマシン名
    pub agent_machine_name: String,

    /// ノードのIPアドレス
    pub agent_ip: IpAddr,

    /// リクエスト本文（JSON形式）
    pub request_body: serde_json::Value,

    /// レスポンス本文（JSON形式、エラー時はNone）
    pub response_body: Option<serde_json::Value>,

    /// 処理時間（ミリ秒）
    pub duration_ms: u64,

    /// レコードのステータス（成功 or エラー）
    pub status: RecordStatus,

    /// レスポンス完了時刻
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RequestType {
    Chat,
    Generate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RecordStatus {
    Success,
    Error { message: String },
}
```

**フィールド詳細**:

| フィールド | 型 | 必須 | 説明 |
| --- | --- | --- | --- |
| `id` | `Uuid` | Yes | レコードの一意識別子、`Uuid::new_v4()` で生成 |
| `timestamp` | `DateTime<Utc>` | Yes | リクエスト受信時刻（UTCタイムゾーン） |
| `request_type` | `RequestType` | Yes | "chat" または "generate" |
| `model` | `String` | Yes | モデル名（例: "llama2", "codellama"） |
| `agent_id` | `Uuid` | Yes | ノードID（Agent構造体のidと一致） |
| `agent_machine_name` | `String` | Yes | ノードのマシン名（表示用） |
| `agent_ip` | `IpAddr` | Yes | ノードのIPアドレス（デバッグ用） |
| `request_body` | `serde_json::Value` | Yes | リクエスト本文全体をJSON Value として保存 |
| `response_body` | `Option<serde_json::Value>` | No | レスポンス本文、エラー時は None |
| `duration_ms` | `u64` | Yes | リクエスト開始から完了までの時間（ミリ秒） |
| `status` | `RecordStatus` | Yes | Success または Error |
| `completed_at` | `DateTime<Utc>` | Yes | レスポンス完了時刻（UTCタイムゾーン） |

**バリデーションルール**:
- `id`: 重複不可（UUIDv4なので実質重複しない）
- `model`: 空文字列不可
- `duration_ms`: 0以上
- `timestamp` <= `completed_at` （時系列整合性）

---

### RequestType (Enum)

リクエストの種類を表す列挙型。

**値**:
- `Chat`: `/api/chat` エンドポイントへのリクエスト
- `Generate`: `/api/generate` エンドポイントへのリクエスト

**シリアライゼーション**:
- JSON: `"chat"` または `"generate"` （小文字）

---

### RecordStatus (Enum)

レコードの処理結果を表す列挙型。

**値**:
- `Success`: 正常に処理完了
- `Error { message: String }`: エラー発生、メッセージを含む

**シリアライゼーション例**:
```json
// 成功時
{ "type": "success" }

// エラー時
{
  "type": "error",
  "message": "Connection timeout to agent"
}
```

---

## 関係性

### Agent ← RequestResponseRecord

**関係タイプ**: 1対多（One-to-Many）

**説明**:
- 1つの Agent は複数の RequestResponseRecord を処理する
- RequestResponseRecord は `agent_id` で Agent を参照する
- 外部キー制約なし（JSONファイルベースのため）

**参照整合性**:
- ノード削除時、既存のレコードは残る（履歴保持のため）
- `agent_machine_name` と `agent_ip` を非正規化して保存（表示用）

**図**:
```
Agent (1) ----< (N) RequestResponseRecord
  id                  agent_id (参照)
  machine_name        agent_machine_name (非正規化)
  ip_address          agent_ip (非正規化)
```

---

## ストレージ形式

### ファイルパス

```
~/.llm-router/request_history.json
```

環境変数 `OLLAMA_ROUTER_DATA_DIR` で変更可能。

### JSON構造

**ファイル内容**:
```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "timestamp": "2025-11-03T10:30:00Z",
    "request_type": "chat",
    "model": "llama2",
    "agent_id": "123e4567-e89b-12d3-a456-426614174000",
    "agent_machine_name": "gpu-server-01",
    "agent_ip": "192.168.1.10",
    "request_body": {
      "model": "llama2",
      "messages": [
        { "role": "user", "content": "Hello" }
      ],
      "stream": false
    },
    "response_body": {
      "message": {
        "role": "assistant",
        "content": "Hello! How can I help you today?"
      },
      "done": true
    },
    "duration_ms": 1234,
    "status": { "type": "success" },
    "completed_at": "2025-11-03T10:30:01Z"
  },
  {
    "id": "660e8400-e29b-41d4-a716-446655440001",
    "timestamp": "2025-11-03T10:31:00Z",
    "request_type": "generate",
    "model": "codellama",
    "agent_id": "123e4567-e89b-12d3-a456-426614174000",
    "agent_machine_name": "gpu-server-01",
    "agent_ip": "192.168.1.10",
    "request_body": {
      "model": "codellama",
      "prompt": "Write a hello world in Rust",
      "stream": false
    },
    "response_body": null,
    "duration_ms": 5678,
    "status": {
      "type": "error",
      "message": "Agent connection timeout"
    },
    "completed_at": "2025-11-03T10:31:05Z"
  }
]
```

**特徴**:
- レコードの配列
- 新しいレコードは配列の末尾に追加
- 古いレコードは定期クリーンアップで削除

---

## 状態遷移

### RequestResponseRecord のライフサイクル

```
1. [作成] プロキシがリクエストを受信
   → timestamp 設定
   → id 生成（UUID）
   → request_body 保存

2. [処理中] ノードへ転送

3. [完了] レスポンス受信
   → response_body 保存
   → duration_ms 計算
   → status 設定（Success または Error）
   → completed_at 設定

4. [保存] 非同期タスクで request_history.json に追記

5. [保持] 7日間保持

6. [削除] クリーンアップタスクが7日より古いレコードを削除
```

**状態図**:
```
[リクエスト受信] → [ノード処理] → [レスポンス完了] → [保存済み] → [7日後削除]
                                            ↓ (エラー時)
                                        [エラー記録] → [保存済み] → [7日後削除]
```

---

## インデックス戦略

JSONファイルベースのため、インデックスは使用しない。

**フィルタリング方法**:
- 全レコードをメモリに読み込み
- Rustのイテレータでフィルタリング
- ページネーションで結果を制限

**パフォーマンス見積もり**:
- 10,000件 × 10KB = 100MB
- 読み込み: < 200ms
- フィルタリング: < 50ms
- 合計: < 300ms（許容範囲）

---

## スキーマバージョニング

**初期バージョン**: v1

**将来の拡張性**:
- フィールド追加は後方互換（`#[serde(default)]` 使用）
- フィールド削除は破壊的変更（非推奨マーク → 削除）
- 型変更は破壊的変更（マイグレーションスクリプト必要）

**マイグレーション戦略**:
- スキーマ変更時はファイル読み込み時にマイグレーション実行
- 古い形式を新しい形式に変換してから保存
- バックアップ作成（`.bak`）

---

## まとめ

**エンティティ数**: 1個（RequestResponseRecord）
**関係性**: Agent との 1対多
**ストレージ**: JSONファイル（シンプル、憲章準拠）
**スケール**: 7日間で10,000+件、100MB程度（問題なし）
