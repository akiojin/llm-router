# データモデル: エージェント自己登録システム

**SPEC-ID**: SPEC-94621a1f
**日付**: 2025-10-30（実装完了日）
**ステータス**: ✅ 実装済み

## エンティティ

### Agent

エージェント情報を表すメインエンティティ

**ファイル**: `common/src/types.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Agent {
    /// エージェントの一意識別子
    pub id: Uuid,

    /// ホスト名
    pub hostname: String,

    /// IPアドレス
    pub ip_address: String,

    /// Ollamaポート番号
    pub port: u16,

    /// Ollamaバージョン
    pub ollama_version: String,

    /// 現在のステータス
    pub status: AgentStatus,

    /// 最後のハートビート受信時刻
    pub last_heartbeat: DateTime<Utc>,

    /// 登録時刻
    pub registered_at: DateTime<Utc>,
}
```

**検証ルール**:
- `id`: UUIDv4形式
- `hostname`: 空文字列不可
- `ip_address`: 有効なIPv4/IPv6アドレス
- `port`: 1-65535
- `ollama_version`: 空文字列不可
- `last_heartbeat <= Utc::now()`
- `registered_at <= Utc::now()`

### AgentStatus

エージェントの稼働状態

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    /// オンライン（ハートビート受信中）
    Online,

    /// オフライン（タイムアウト）
    Offline,
}
```

**状態遷移**:
```
        登録
         ↓
    ┌─────────┐
    │ Online  │ ←── ハートビート受信
    └────┬────┘
         │
         │ 60秒タイムアウト
         ↓
    ┌─────────┐
    │ Offline │
    └────┬────┘
         │
         │ ハートビート再開
         ↓
    ┌─────────┐
    │ Online  │
    └─────────┘
```

## プロトコル型

### RegisterRequest

エージェント登録リクエスト

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub hostname: String,
    pub ip_address: String,
    pub port: u16,
    pub ollama_version: String,
}
```

### RegisterResponse

エージェント登録レスポンス

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub status: RegisterStatus,
    pub agent_id: Option<Uuid>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RegisterStatus {
    Success,
    Failure,
}
```

### HeartbeatRequest

ハートビート送信リクエスト

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub agent_id: Uuid,
}
```

## ストレージスキーマ

**ファイル**: `~/.ollama-coordinator/agents.json`

```json
[
  {
    "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "hostname": "server-01",
    "ip_address": "192.168.1.10",
    "port": 11434,
    "ollama_version": "0.1.23",
    "status": "Online",
    "last_heartbeat": "2025-10-30T12:00:00Z",
    "registered_at": "2025-10-30T10:00:00Z"
  }
]
```

## エンティティ関係図

```
┌──────────────────┐
│ RegisterRequest  │
└────────┬─────────┘
         │
         ↓ register()
┌─────────────────────────┐
│ Agent                   │
├─────────────────────────┤
│ id: Uuid                │
│ hostname: String        │
│ ip_address: String      │
│ port: u16               │
│ ollama_version: String  │
│ status: AgentStatus ◄───┼──┐
│ last_heartbeat: DateTime│  │
│ registered_at: DateTime │  │
└─────────────────────────┘  │
                             │
            ┌────────────────┴──────────┐
            │ AgentStatus (enum)        │
            ├───────────────────────────┤
            │ • Online                  │
            │ • Offline                 │
            └───────────────────────────┘
```

## データフロー

### 1. エージェント登録
```
RegisterRequest
    → AgentRegistry::register()
    → Agent 生成（UUID割り当て、status=Online）
    → メモリ保存（HashMap）
    → ファイル永続化（agents.json）
    → RegisterResponse
```

### 2. ハートビート
```
HeartbeatRequest
    → AgentRegistry::heartbeat()
    → last_heartbeat 更新
    → status = Online
    → ファイル永続化
```

### 3. タイムアウト検出
```
定期チェック（SPEC-443acc8c）
    → Utc::now() - last_heartbeat > 60秒
    → status = Offline
    → ファイル永続化
```

## 実装メモ

- **メモリ管理**: `HashMap<Uuid, Agent>` をArc<RwLock>でラップ
- **永続化**: 非同期ファイルI/O（`tokio::fs`）
- **並行制御**: RwLock で読み取り並行、書き込み排他
- **JSON**: `serde_json` でシリアライズ/デシリアライズ
