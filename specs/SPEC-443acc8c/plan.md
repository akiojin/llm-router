# 実装計画: ヘルスチェックシステム

**機能ID**: `SPEC-443acc8c` | **日付**: 2025-10-30（実装完了日） | **仕様**: [spec.md](./spec.md)
**入力**: `/ollama-coordinator/specs/SPEC-443acc8c/spec.md`の機能仕様
**ステータス**: ✅ **実装済み** (PR #1でマージ済み)

## 概要

エージェントの稼働状況を定期的に監視し、障害を自動検知するヘルスチェックシステム。応答がないエージェントは自動的にリクエスト振り分けから除外され、復旧時に自動的に再登録される。

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+
**主要依存関係**: Tokio（非同期ランタイム）, chrono（タイムスタンプ）
**ストレージ**: JSONファイル（agents.jsonにステータス永続化）
**テスト**: cargo test
**対象プラットフォーム**: Linuxサーバー
**プロジェクトタイプ**: single（coordinatorクレート内）
**パフォーマンス目標**: ヘルスチェック処理がプロキシ処理に影響を与えない
**制約**: バックグラウンド非同期実行
**スケール/スコープ**: 100エージェント対応

## 憲章チェック

**シンプルさ**: ✅
- パッシブヘルスチェック: ✅ ハートビートベース（コーディネーターからポーリングなし）
- 単一データモデル: ✅ Agentモデル再利用、statusフィールド使用
- パターン回避: ✅ シンプルなタイムアウト判定ロジック

**アーキテクチャ**: ✅
- バックグラウンドタスク: ✅ Tokio spawn でタイムアウト監視タスク実行

**テスト**: ✅
- TDDサイクル遵守: ✅ テスト先行実装

**可観測性**: ✅
- ステータス変化ログ: ✅ Online ↔ Offline 遷移をログ出力

## 実装アーキテクチャ

### パッシブヘルスチェック（ハートビートベース）

```rust
// ハートビート受信時にlast_heartbeatを更新、自動的にOnlineに
pub async fn heartbeat(&self, agent_id: Uuid) -> Result<()> {
    let mut agents = self.agents.write().await;
    if let Some(agent) = agents.get_mut(&agent_id) {
        agent.last_heartbeat = Utc::now();
        agent.status = AgentStatus::Online; // 自動復旧
        tracing::info!("Agent {} heartbeat received", agent_id);
    }
    Ok(())
}
```

### バックグラウンドタイムアウト監視

```rust
// 定期的にタイムアウトをチェックし、Offline化
pub async fn start_timeout_monitor(&self, interval: Duration, timeout: Duration) {
    let agents = self.agents.clone();
    tokio::spawn(async move {
        let mut interval_timer = tokio::time::interval(interval);
        loop {
            interval_timer.tick().await;
            let mut agents_lock = agents.write().await;
            let now = Utc::now();
            for agent in agents_lock.values_mut() {
                if agent.status == AgentStatus::Online {
                    let elapsed = now - agent.last_heartbeat;
                    if elapsed > timeout {
                        agent.status = AgentStatus::Offline;
                        tracing::warn!("Agent {} timed out", agent.id);
                    }
                }
            }
        }
    });
}
```

## 実装の主要決定

### 決定1: パッシブヘルスチェック（ハートビートベース）

**選択**: コーディネーターはポーリングせず、エージェントからのハートビート受信のみで判定

**理由**:
- シンプル: 能動的なHTTPリクエスト不要
- スケーラブル: エージェント数が増えてもネットワーク負荷一定
- リソース効率: CPUとネットワーク帯域を節約
- パフォーマンス: プロキシ処理に影響なし

**代替案検討**:
- **Active polling**: コーディネーターからGET /api/healthでポーリング → ネットワーク負荷高、複雑

### 決定2: 60秒タイムアウト

**選択**: 最後のハートビートから60秒経過でOffline化

**理由**:
- 2倍マージン: 30秒ハートビート間隔の2倍で誤検知防止
- ネットワーク揺らぎ許容: 一時的な遅延でOfflineにならない
- 障害検知速度: 1分以内に障害検出（十分に早い）

### 決定3: 環境変数設定可能化

**選択**: `AGENT_TIMEOUT`環境変数でタイムアウト設定可能

**理由**:
- 柔軟性: 環境に応じてチューニング可能
- 再コンパイル不要: 設定変更で即座に反映

## 進捗トラッキング

**フェーズステータス**:
- [x] Phase 0: Research完了
- [x] Phase 1: Design完了
- [x] Phase 2: Task planning完了
- [x] Phase 3: Tasks実行完了
- [x] Phase 4: 実装完了
- [x] Phase 5: 検証合格

**ゲートステータス**:
- [x] 初期憲章チェック: 合格
- [x] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱なし

---
*憲章 v1.0.0 に基づく - `/memory/constitution.md` 参照*
