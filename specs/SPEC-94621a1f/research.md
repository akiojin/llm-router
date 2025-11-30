# 技術リサーチ: ノード自己登録システム

**SPEC-ID**: SPEC-94621a1f
**日付**: 2025-10-30（実装完了日）
**ステータス**: ✅ 完了

## 主要技術決定

### 決定1: JSONファイルストレージ

**選択**: `~/.llm-router/agents.json` にノード情報を保存

**理由**:
- シンプル: データベースサーバー不要
- 十分な性能: ~100ノードなら数ms で読み書き
- ポータビリティ: ファイルコピーでバックアップ・移行可能
- デバッグ容易: テキストエディタで確認可能

**代替案検討**:
- **SQLite**: 構造化クエリ可能だが、初期段階では過剰。将来的に検討
- **PostgreSQL**: 完全にオーバースペック、運用コスト高
- **In-memory のみ**: 再起動でデータ消失、非機能要件に不適合

**実装詳細**:
```rust
// ~/.llm-router/agents.json
[
  {
    "id": "uuid",
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

### 決定2: Arc<RwLock<HashMap>> によるメモリ内管理

**選択**: `Arc<RwLock<HashMap<Uuid, Agent>>>` でノード情報をメモリ管理

**理由**:
- 高速: O(1) アクセス
- 並行安全: RwLock で複数リーダー、単一ライター
- 共有可能: Arc で複数スレッド間で安全に共有

**代替案検討**:
- **Mutex<HashMap>**: 読み取りも排他ロック、性能劣る
- **DashMap**: 優れた並行性だが、依存増加
- **Database直接アクセス**: I/O遅延、複雑化

**実装詳細**:
```rust
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<Uuid, Agent>>>,
}

// 読み取り（複数同時可）
let agents = self.agents.read().await;

// 書き込み（排他）
let mut agents = self.agents.write().await;
agents.insert(id, agent);
```

### 決定3: 30秒ハートビート間隔 + 60秒タイムアウト

**選択**: ノードは30秒ごとにハートビート、60秒タイムアウトでOffline

**理由**:
- バランス: ネットワーク負荷低、障害検出も適度に早い
- 2倍マージン: 1回ハートビート失敗してもOfflineにならない（ネットワーク揺らぎ許容）

**代替案検討**:
- **10秒/20秒**: ネットワーク負荷高、過敏
- **60秒/120秒**: 障害検出遅い

**実装詳細**:
- 環境変数: `HEARTBEAT_INTERVAL=30`, `AGENT_TIMEOUT=60`
- ノード側: `tokio::time::interval(Duration::from_secs(30))`
- ルーター側: `last_heartbeat + 60秒 < now` でOffline判定

### 決定4: UUID v4 によるノードID生成

**選択**: `uuid::Uuid::new_v4()` でランダムUUID生成

**理由**:
- 衝突リスク極小: 実質的にユニーク
- 中央管理不要: 各ノードが独立生成可能
- 標準的: Rust `uuid` クレート、安定

**代替案検討**:
- **連番ID**: 中央管理必要、分散生成不可
- **ホスト名**: 衝突可能（同じホスト名のマシン複数）
- **IPアドレス**: DHCPで変わる可能性

## 実装で学んだこと

1. **Axum State共有**: `State<AppState>` で簡単に共有状態アクセス
2. **RwLock の使い分け**: 読み取り頻度高 → RwLock、書き込み頻度高 → Mutex
3. **非同期ファイルI/O**: `tokio::fs` で非同期化、ブロッキングなし
4. **エラー型設計**: `thiserror` で明確なエラー型定義

## 技術負債・将来改善

- [ ] 認証機能なし → APIキーまたはトークンベース認証追加
- [ ] ストレージが1ファイル → 将来的にSQLite検討（1000+ノード時）
- [ ] ヘルスチェックが受動的 → 能動的Ping検討

**優先度**: 低（現状で十分機能）
