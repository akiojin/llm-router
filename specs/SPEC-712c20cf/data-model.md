# データモデル: 管理ダッシュボード

**機能ID**: `SPEC-712c20cf` | **日付**: 2025-10-31

## 概要

管理ダッシュボード機能で使用するデータモデル定義。既存の`Agent`型を再利用し、新規に`SystemStats`型を追加する。

## エンティティ

### 1. Agent (既存)

**説明**: エージェント情報を表す構造体（既存の`common/src/types.rs`で定義済み）

> 2025-11-01 追記: `loaded_models: Vec<String>` を追加し、エージェントがOllamaにロード済みのモデル一覧を保持する。ダッシュボードの「モデル」列および詳細モーダルで参照する。

**フィールド**:
```rust
pub struct Agent {
    pub id: Uuid,
    pub machine_name: String,
    pub ip_address: String,
    pub ollama_version: String,
    pub status: AgentStatus,
    pub registered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub system_info: SystemInfo,
}

pub enum AgentStatus {
    Online,
    Offline,
}

pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub cpu_cores: u32,
    pub total_memory: u64,
}
```

**検証ルール**:
- `machine_name`: 空文字列禁止
- `ip_address`: 有効なIPv4/IPv6アドレス
- `ollama_version`: セマンティックバージョニング形式

**ダッシュボードでの使用**:
- エージェント一覧表示
- オンライン/オフラインステータス表示
- 稼働時間計算（`registered_at`から現在時刻までの差分）

### 2. SystemStats (新規)

**説明**: システム全体の統計情報

**フィールド**:
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct SystemStats {
    pub total_agents: usize,
    pub online_agents: usize,
    pub offline_agents: usize,
    pub total_requests: u64,      // 将来拡張
    pub avg_response_time_ms: u32, // 将来拡張
    pub errors_count: u64,         // 将来拡張
}
```

**検証ルール**:
- `total_agents >= 0`
- `online_agents + offline_agents == total_agents`
- `total_requests >= 0`
- `avg_response_time_ms >= 0`

**計算方法**:
- `total_agents`: AgentRegistryの全エージェント数
- `online_agents`: `status == AgentStatus::Online`の数
- `offline_agents`: `status == AgentStatus::Offline`の数
- `total_requests`, `avg_response_time_ms`, `errors_count`: 将来拡張（初期実装では0）

### 3. AgentWithUptime (新規レスポンス型)

**説明**: ダッシュボードAPI用のエージェント情報（稼働時間を含む）

**フィールド**:
```rust
#[derive(Debug, Serialize)]
pub struct AgentWithUptime {
    pub id: Uuid,
    pub machine_name: String,
    pub ip_address: String,
    pub status: AgentStatus,
    pub ollama_version: String,
    pub registered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub uptime_seconds: i64,
}
```

**計算方法**:
- `uptime_seconds`: `last_seen - registered_at`（秒単位）

**API変換**:
```rust
impl From<Agent> for AgentWithUptime {
    fn from(agent: Agent) -> Self {
        let uptime_seconds = (agent.last_seen - agent.registered_at).num_seconds();
        Self {
            id: agent.id,
            machine_name: agent.machine_name,
            ip_address: agent.ip_address,
            status: agent.status,
            ollama_version: agent.ollama_version,
            registered_at: agent.registered_at,
            last_seen: agent.last_seen,
            uptime_seconds,
        }
    }
}
```

### 4. AgentMetrics (将来拡張、SPEC-589f2df1依存)

**説明**: エージェントのパフォーマンスメトリクス（将来拡張用）

**フィールド**:
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub agent_id: Uuid,
    pub cpu_usage: f64,           // %
    pub memory_usage: f64,        // %
    pub active_requests: u32,     // 件
    pub avg_response_time_ms: u32,// ms
    pub timestamp: DateTime<Utc>,
}
```

**注**: SPEC-589f2df1（ロードバランシングシステム）でメトリクス収集機能が実装された後に使用可能。

## エンティティ関係図

```
┌─────────────────┐
│     Agent       │ (既存)
│─────────────────│
│ + id            │
│ + machine_name  │
│ + ip_address    │
│ + status        │
│ + ...           │
└─────────────────┘
         │
         │ 1:1 変換
         ▼
┌──────────────────┐
│ AgentWithUptime  │ (新規レスポンス型)
│──────────────────│
│ + id             │
│ + machine_name   │
│ + uptime_seconds │
│ + ...            │
└──────────────────┘

┌──────────────────┐
│   SystemStats    │ (新規)
│──────────────────│
│ + total_agents   │
│ + online_agents  │
│ + ...            │
└──────────────────┘

┌──────────────────┐
│  AgentMetrics    │ (将来拡張)
│──────────────────│
│ + agent_id       │
│ + cpu_usage      │
│ + ...            │
└──────────────────┘
         │
         │ 1:N
         │
         ▼
┌─────────────────┐
│     Agent       │
└─────────────────┘
```

## 状態遷移

### AgentStatus

```
    register
┌──────────────┐
│   (未登録)    │
└──────────────┘
       │
       │ POST /api/agents/register
       ▼
┌──────────────┐
│    Online    │ ◄──────┐
└──────────────┘        │
       │                │ POST /api/agents/:id/heartbeat
       │ timeout        │
       ▼                │
┌──────────────┐        │
│   Offline    │ ───────┘
└──────────────┘
```

## データフロー

### エージェント一覧取得
```
Client ─GET /api/dashboard/agents→ Coordinator
                                        │
                                        │ AgentRegistry.list_all()
                                        ▼
                                    Vec<Agent>
                                        │
                                        │ map(Agent → AgentWithUptime)
                                        ▼
                                  Vec<AgentWithUptime>
                                        │
                                        │ JSON
                                        ▼
Client ◄──────────────────────────── Response
```

### システム統計取得
```
Client ─GET /api/dashboard/stats→ Coordinator
                                       │
                                       │ AgentRegistry.list_all()
                                       ▼
                                   Vec<Agent>
                                       │
                                       │ count(), filter()
                                       ▼
                                   SystemStats
                                       │
                                       │ JSON
                                       ▼
Client ◄─────────────────────────── Response
```

## ファイル配置

```
common/src/
├── types.rs              # Agent, AgentStatus, SystemInfo (既存)
└── dashboard.rs          # AgentWithUptime, SystemStats (新規)

coordinator/src/
├── api/
│   └── dashboard.rs      # ダッシュボードAPI実装
└── registry/
    └── mod.rs            # AgentRegistry (既存)
```

## テストデータ

### サンプルAgent
```json
{
  "id": "123e4567-e89b-12d3-a456-426614174000",
  "machine_name": "server-01",
  "ip_address": "192.168.1.100",
  "status": "Online",
  "ollama_version": "0.1.0",
  "registered_at": "2025-10-31T10:00:00Z",
  "last_seen": "2025-10-31T12:30:00Z",
  "system_info": {
    "os": "Linux",
    "arch": "x86_64",
    "cpu_cores": 8,
    "total_memory": 16777216
  }
}
```

### サンプルSystemStats
```json
{
  "total_agents": 10,
  "online_agents": 8,
  "offline_agents": 2,
  "total_requests": 0,
  "avg_response_time_ms": 0,
  "errors_count": 0
}
```

## 将来拡張

### メトリクス可視化（SPEC-589f2df1実装後）
- `AgentMetrics`の実装
- メトリクス収集API (`POST /api/agents/:id/metrics`)
- メトリクス取得API (`GET /api/dashboard/metrics/:agent_id`)
- リクエスト履歴グラフ用のデータ構造

### リクエスト履歴
- `RequestHistory`構造体
- 時系列データ（1分単位のリクエスト数）
- リングバッファによるメモリ管理（最新1時間分のみ保持）

---
*このデータモデルは plan.md Phase 1 の成果物です*
