# 実装計画: ロードバランシングシステム

**機能ID**: `SPEC-589f2df1` | **日付**: 2025-10-30（部分実装） | **仕様**: [spec.md](./spec.md)
**入力**: `/llm-router/specs/SPEC-589f2df1/spec.md`の機能仕様
**ステータス**: 🚧 **部分実装** (Phase 1: ラウンドロビン完了、Phase 2: メトリクスベース未実装)

## 概要

複数のノード間でリクエストを最適に分散するロードバランシング機能。Phase 1（ラウンドロビン）は実装済み、Phase 2（メトリクスベース）は今後の拡張として計画中。

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+
**主要依存関係**: Tokio（非同期）, AtomicUsize（ラウンドロビン）, sysinfo（CPU/メモリ監視、Phase 2で追加）
**ストレージ**: JSONファイル（ノード情報）+ インメモリ（メトリクス、Phase 2で追加）
**テスト**: cargo test
**対象プラットフォーム**: Linuxサーバー
**プロジェクトタイプ**: single（coordinatorクレート内）
**パフォーマンス目標**: ノード選択 < 10ms
**制約**: メトリクス収集がプロキシ処理をブロックしない
**スケール/スコープ**: 1000ノード対応

## 憲章チェック

**シンプルさ**: ✅
- Phase 1: シンプルなラウンドロビン（実装済み）
- Phase 2: メトリクス収集は最小限（未実装）

**アーキテクチャ**: ✅
- Phase 1: AtomicUsizeによるステートレスなインデックス管理

**テスト**: ✅
- TDDサイクル遵守: ✅ ラウンドロビンテストは先行実装

## 実装状況

### ✅ Phase 1: ラウンドロビン方式（実装済み）

**実装内容**:
```rust
pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<Uuid, Agent>>>,
    round_robin_index: AtomicUsize,
}

pub async fn select_agent(&self) -> Option<Agent> {
    let agents = self.agents.read().await;
    let online_agents: Vec<_> = agents.values()
        .filter(|a| a.status == AgentStatus::Online)
        .cloned()
        .collect();

    if online_agents.is_empty() {
        return None;
    }

    let index = self.round_robin_index.fetch_add(1, Ordering::Relaxed);
    Some(online_agents[index % online_agents.len()].clone())
}
```

**実装の主要決定**:
- **AtomicUsizeインデックス**: ロックフリーで高速
- **モジュロ演算**: `index % len` でサイクリックに選択
- **オンラインフィルター**: Offlineノードは除外

### 🚧 Phase 2: メトリクスベース選択（未実装）

**計画内容**:

#### データモデル（追加予定）
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub agent_id: Uuid,
    pub cpu_usage: f32,          // 0.0-100.0
    pub memory_usage: f32,       // 0.0-100.0
    pub active_requests: usize,
    pub avg_response_time: f64,  // milliseconds
    pub timestamp: DateTime<Utc>,
}

pub struct AgentRegistry {
    agents: Arc<RwLock<HashMap<Uuid, Agent>>>,
    metrics: Arc<RwLock<HashMap<Uuid, AgentMetrics>>>, // 追加
    round_robin_index: AtomicUsize,
}
```

#### メトリクスベース選択アルゴリズム（追加予定）
```rust
pub async fn select_agent_by_metrics(&self) -> Option<Agent> {
    let agents = self.agents.read().await;
    let metrics = self.metrics.read().await;

    let online_agents: Vec<_> = agents.values()
        .filter(|a| a.status == AgentStatus::Online)
        .cloned()
        .collect();

    if online_agents.is_empty() {
        return None;
    }

    // 負荷スコア計算: CPU + Memory + Active Requests
    let best_agent = online_agents.iter()
        .min_by_key(|agent| {
            if let Some(m) = metrics.get(&agent.id) {
                let cpu_score = m.cpu_usage as usize;
                let mem_score = m.memory_usage as usize;
                let req_score = m.active_requests * 10;
                cpu_score + mem_score + req_score
            } else {
                usize::MAX // メトリクスなしは最低優先度
            }
        })?;

    Some(best_agent.clone())
}
```

#### メトリクス収集API（追加予定）
```rust
// POST /api/agents/:id/metrics
pub async fn update_metrics(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
    Json(metrics): Json<AgentMetrics>,
) -> Result<StatusCode, AppError> {
    let mut metrics_map = state.registry.metrics.write().await;
    metrics_map.insert(agent_id, metrics);
    Ok(StatusCode::NO_CONTENT)
}
```

## Phase 0: 技術リサーチ（Phase 2用）

**未実施事項**:
- Rust `sysinfo` クレート調査（CPU/メモリ監視）
- メトリクスストレージ方式検討（インメモリ vs Redis）
- 負荷スコアアルゴリズムベストプラクティス

## Phase 1: 設計＆契約（Phase 2用）

**未実施事項**:
- メトリクス収集API契約定義
- AgentMetrics データモデル設計
- 負荷ベース選択アルゴリズム仕様

## Phase 2: タスク分解（Phase 2用）

**推定タスク数**: 約20タスク
**推定実装時間**: 約10時間

**タスク生成戦略**:
- メトリクス収集API実装
- AgentMetricsデータモデル実装
- 負荷ベース選択ロジック実装
- メトリクス永続化（オプション）
- パフォーマンステスト

## 進捗トラッキング

**Phase 1（ラウンドロビン）**:
- [x] Research完了
- [x] Design完了
- [x] Task planning完了
- [x] Tasks実行完了
- [x] 実装完了（PR #1）
- [x] 検証合格

**Phase 2（メトリクスベース）**:
- [ ] Research完了
- [ ] Design完了
- [ ] Task planning完了
- [ ] Tasks実行完了
- [ ] 実装完了
- [ ] 検証合格

---
*憲章 v1.0.0 に基づく - `/memory/constitution.md` 参照*
