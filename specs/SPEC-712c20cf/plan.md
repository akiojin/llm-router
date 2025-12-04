# 実装計画: 管理ダッシュボード

**機能ID**: `SPEC-712c20cf` | **日付**: 2025-10-31 | **仕様**: [spec.md](./spec.md)
**ステータス**: 📋 未実装
**元のSPEC**: SPEC-32e2b31aから分割
**依存SPEC**: SPEC-94621a1f, SPEC-63acef08, SPEC-443acc8c

## 実行フロー (/speckit.plan コマンドのスコープ)
```
1. 入力パスから機能仕様を読み込み ✅
2. 技術コンテキストを記入 ✅
3. 憲章チェックセクションを評価 ✅
4. Phase 0 を実行 → research.md (このplan.mdに統合)
5. Phase 1 を実行 → contracts, data-model.md, quickstart.md
6. 憲章チェックセクションを再評価
7. Phase 2 を計画 → タスク生成アプローチを記述
8. 停止 - /speckit.tasks コマンドの準備完了
```

## 概要

WebブラウザからアクセスできるリアルタイムダッシュボードUI。ノードの状態、リクエスト処理状況、パフォーマンスメトリクスを可視化する。

**主要要件**:
- ノード一覧表示（マシン名、IP、ステータス、稼働時間）
- リアルタイム更新（5秒ごとのポーリング）
- パフォーマンスメトリクス可視化（CPU、メモリ、リクエスト数）
- システム統計表示（総ノード数、オンライン数、総リクエスト数）

**技術アプローチ** (research.mdから):
- **フロントエンド**: Vanilla JS + Chart.js（シンプルさ優先、ビルドプロセス不要）
- **リアルタイム通信**: ポーリング（5秒間隔、実装簡単）
- **バックエンド**: Axum静的ファイル配信 + REST API

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+ (stable)
**主要依存関係**:
- `axum` - Web APIフレームワーク（既存）
- `tower-http` - 静的ファイル配信用の`ServeDir`ミドルウェア
- `tokio` - 非同期ランタイム（既存）
- `serde_json` - JSONシリアライゼーション（既存）
- `Chart.js` - グラフ可視化（CDN経由）

**ストレージ**: N/A（ノード情報は既存のAgentRegistryから取得）
**テスト**: `cargo test`（単体・統合テスト）、手動E2Eテスト（ブラウザ）
**対象プラットフォーム**: Linuxサーバー、Webブラウザ（クライアント）
**プロジェクトタイプ**: web（backend + frontend静的ファイル）
**パフォーマンス目標**: 初回ロード<2秒、ポーリング<100ms、1000ノードでも快適
**制約**:
- 認証機能なし（将来の拡張候補）
- リクエスト履歴はメモリのみ（再起動で消失）
**スケール/スコープ**: 1000ノード以下、4つの主要画面セクション

## 憲章チェック
*ゲート: Phase 0 research前に合格必須。Phase 1 design後に再チェック。*

**シンプルさ**:
- プロジェクト数: 1（coordinatorプロジェクトに統合）
- フレームワークを直接使用? ✅ Yes（AxumのServeDir、Chart.js CDN）
- 単一データモデル? ✅ Yes（既存のAgentとAgentMetricsを使用）
- パターン回避? ✅ Yes（Repository/UoW不使用、直接AgentRegistry経由）

**アーキテクチャ**:
- すべての機能をライブラリとして? N/A（coordinatorはバイナリクレート）
- ライブラリリスト: coordinator/src（既存のregistryとapi）
- ライブラリごとのCLI: coordinator binary（`--help`, `--version`対応予定）
- ライブラリドキュメント: llms.txt形式を計画? N/A

**テスト (妥協不可)**:
- RED-GREEN-Refactorサイクルを強制? ✅ Yes
- Gitコミットはテストが実装より先に表示? ✅ Yes
- 順序: Contract→Integration→E2E→Unitを厳密に遵守? ✅ Yes
- 実依存関係を使用? ✅ Yes（実AgentRegistry、モックなし）
- Integration testの対象: APIエンドポイント、静的ファイル配信
- 禁止: テスト前の実装、REDフェーズのスキップ ✅

**可観測性**:
- 構造化ロギング含む? ✅ Yes（既存のtracing使用）
- フロントエンドログ → バックエンド? N/A（ブラウザコンソールログ）
- エラーコンテキスト十分? ✅ Yes（AxumのIntoResponseでエラー詳細返却）

**バージョニング**:
- バージョン番号割り当て済み? ✅ Yes（Cargo.tomlで管理）
- 変更ごとにBUILDインクリメント? ✅ Yes
- 破壊的変更を処理? ✅ Yes（API v1としてバージョニング）

## プロジェクト構造

### ドキュメント (この機能)
```
specs/SPEC-712c20cf/
├── spec.md              # 機能仕様（既存）
├── plan.md              # このファイル (/speckit.plan コマンド出力)
├── research.md          # Phase 0 出力（このplan.mdに統合済み）
├── data-model.md        # Phase 1 出力
├── quickstart.md        # Phase 1 出力
├── contracts/           # Phase 1 出力
│   ├── dashboard-api.yaml
│   └── dashboard-ui.md
└── tasks.md             # Phase 2 出力 (/speckit.tasks コマンド)
```

### ソースコード (リポジトリルート)
```
coordinator/
├── src/
│   ├── api/
│   │   ├── mod.rs
│   │   ├── agent.rs         # 既存
│   │   ├── proxy.rs         # 既存
│   │   └── dashboard.rs     # 新規: ダッシュボードAPI
│   ├── registry/
│   │   └── mod.rs           # 既存: AgentRegistry
│   ├── web/
│   │   └── static/          # 新規: 静的ファイル
│   │       ├── index.html   # ダッシュボードHTML
│   │       ├── app.js       # ダッシュボードロジック
│   │       ├── styles.css   # スタイルシート
│   │       └── lib/
│   │           └── chart.min.js  # Chart.js（オプション、CDN推奨）
│   └── main.rs              # エントリーポイント
└── tests/
    ├── dashboard_api_test.rs    # 新規: APIテスト
    └── dashboard_static_test.rs # 新規: 静的ファイルテスト
```

**構造決定**: Webアプリケーション（backend + frontend静的ファイル）として実装、coordinatorプロジェクトに統合

## Phase 0: アウトライン＆リサーチ

### 技術選択の理由

#### フロントエンド技術スタック
**決定**: Vanilla JS + Chart.js

**理由**:
1. **シンプルさの極限を追求**（CLAUDE.md準拠）
2. ビルドプロセス不要 → 開発効率向上
3. 依存関係の最小化 → メンテナンス負担軽減
4. デバッグが容易 → ブラウザのDevToolsで直接確認可能
5. 学習コストが低い → 標準Web技術のみ

**検討した代替案**:
- **React + Recharts**: リッチなUI、保守性高いが、ビルドプロセス必要、複雑さ増加
- **Vue.js + ECharts**: 学習コスト低い、日本語ドキュメント豊富だが、依然としてビルド必要
- **却下理由**: ダッシュボードは比較的小規模（4セクション）なので、Vanilla JSで十分

#### リアルタイム通信
**決定**: ポーリング（5秒間隔）

**理由**:
1. 実装が最もシンプル（`setInterval` + `fetch`のみ）
2. 5秒間隔なら負荷も許容範囲（1000ノード × 5秒 = 200 req/min）
3. デバッグが容易（Network tabで確認可能）
4. ダッシュボードの用途では5秒遅延は許容範囲

**検討した代替案**:
- **WebSocket**: 双方向通信、低レイテンシだが、実装複雑、再接続処理必要
- **Server-Sent Events (SSE)**: 片方向通信、シンプルだが、ブラウザ互換性に懸念
- **却下理由**: リアルタイム性の要件が厳しくない（5秒更新で十分）ため、最もシンプルなポーリングを選択

#### グラフライブラリ
**決定**: Chart.js

**理由**:
1. 軽量（Vanilla JSとの相性良い）
2. ドキュメントが豊富
3. レスポンシブデザイン対応
4. CDN経由で簡単に導入可能

**検討した代替案**:
- **Recharts**: React専用（Vanilla JSでは使用不可）
- **ECharts**: 高機能だが、ファイルサイズが大きい
- **D3.js**: 柔軟性高いが、学習コストが高い
- **却下理由**: シンプルさとVanilla JSとの相性を優先

### 要明確化の解決

すべての技術選択が完了し、要明確化事項はありません。

## Phase 1: 設計＆契約

### API契約設計

#### 1. ダッシュボードページ
- **エンドポイント**: `GET /dashboard`
- **レスポンス**: HTML（index.html）
- **機能**: ダッシュボードのメインページを返す

#### 2. ノード状態API
- **エンドポイント**: `GET /api/dashboard/agents`
- **レスポンス**: JSON
```json
[
  {
    "id": "uuid",
    "machine_name": "server-01",
    "ip_address": "192.168.1.100",
    "status": "Online",
    "runtime_version": "0.1.0",
    "registered_at": "2025-10-30T10:00:00Z",
    "last_seen": "2025-10-30T12:30:00Z",
    "uptime_seconds": 9000
  }
]
```

#### 3. システム統計API
- **エンドポイント**: `GET /api/dashboard/stats`
- **レスポンス**: JSON
```json
{
  "total_agents": 10,
  "online_agents": 8,
  "offline_agents": 2,
  "total_requests": 1523,
  "avg_response_time_ms": 250,
  "errors_count": 5
}
```

#### 4. ノード設定API（FR-023）
- **エンドポイント**: `PUT /api/agents/:id/settings`
- **リクエスト**: JSON
```json
{
  "custom_name": "Production Server",
  "tags": ["production", "high-priority"],
  "notes": "Primary LLM server"
}
```
- **レスポンス**: 200 OK, 更新されたノード情報

#### 5. ノード削除API（FR-024）
- **エンドポイント**: `DELETE /api/agents/:id`
- **レスポンス**: 204 No Content
- **機能**: ノードを登録解除

#### 6. ノード強制切断API（FR-024）
- **エンドポイント**: `POST /api/agents/:id/disconnect`
- **レスポンス**: 200 OK
- **機能**: ノードを強制的にOffline状態に

#### 7. メトリクスAPI（SPEC-589f2df1実装後）
- **エンドポイント**: `GET /api/dashboard/metrics/:agent_id`
- **レスポンス**: JSON
```json
{
  "agent_id": "uuid",
  "cpu_usage": 45.2,
  "memory_usage": 60.5,
  "active_requests": 3,
  "request_history": [
    {"timestamp": "2025-10-30T12:25:00Z", "count": 10},
    {"timestamp": "2025-10-30T12:26:00Z", "count": 12}
  ]
}
```

### データモデル

詳細は `data-model.md` を参照。

**主要エンティティ**:
- `Agent` (既存): ノード情報
- `AgentMetrics` (将来拡張): パフォーマンスメトリクス
- `SystemStats` (新規): システム統計情報

### 契約テスト

#### contract_test_dashboard_api.rs (RED)
```rust
#[tokio::test]
async fn test_get_agents_returns_json_array() {
    // テストは失敗する必要がある（まだ実装なし）
}

#[tokio::test]
async fn test_get_stats_returns_system_stats() {
    // テストは失敗する必要がある（まだ実装なし）
}
```

#### contract_test_dashboard_static.rs (RED)
```rust
#[tokio::test]
async fn test_dashboard_page_returns_html() {
    // テストは失敗する必要がある（まだ実装なし）
}
```

### テストシナリオ（Integration）

ユーザーストーリーから抽出したテストシナリオ:

1. **シナリオ1**: ダッシュボードアクセス
   - 前提: ルーターが起動
   - 実行: `GET /dashboard`
   - 結果: HTMLページが返却され、ノード一覧が表示

2. **シナリオ2**: ノード情報表示
   - 前提: 複数のノードが登録
   - 実行: `GET /api/dashboard/agents`
   - 結果: JSON配列で全ノード情報が返却

3. **シナリオ3**: リアルタイム更新
   - 前提: ダッシュボード表示中
   - 実行: 5秒待機
   - 結果: JavaScriptが自動的にAPIを呼び出し、UIが更新

## Phase 2: タスク計画アプローチ
*このセクションは/speckit.tasksコマンドが実行することを記述*

**タスク生成戦略**:
- `/templates/tasks-template.md` をベースとして読み込み
- Phase 1設計ドキュメント (contracts, data model) からタスクを生成
- TDD順序を厳守: テスト → 実装 → テスト合格

**タスクカテゴリ**:

1. **Setup Tasks**:
   - S001: Cargo.tomlにtower-http依存関係を追加
   - S002: coordinator/src/web/static/ディレクトリ作成

2. **Contract Test Tasks** [P]:
   - C001: ダッシュボードAPI契約テスト作成（RED）
   - C002: 静的ファイル配信契約テスト作成（RED）

3. **Core Implementation Tasks**:
   - I001: ダッシュボードAPI実装（GREEN） - C001を合格させる
   - I002: 静的ファイル配信実装（GREEN） - C002を合格させる
   - I003: index.html作成（基本構造）
   - I004: app.js作成（ポーリングロジック）
   - I005: styles.css作成（レスポンシブデザイン）

4. **Integration Test Tasks**:
   - T001: エンドツーエンドテスト（ダッシュボードアクセス）
   - T002: リアルタイム更新テスト

5. **Polish Tasks**:
   - P001: エラーハンドリング改善
   - P002: パフォーマンス最適化
   - P003: ドキュメント更新（README.md）

**順序戦略**:
- TDD順序: C001→I001、C002→I002
- 依存関係順序: Setup → Contract Tests → Implementation → Integration Tests
- 並列実行のために[P]をマーク: C001とC002は並列実行可能

**推定出力**: tasks.mdに約30個の番号付き、順序付きタスク

## Phase 3+: 今後の実装
*これらのフェーズは/planコマンドのスコープ外*

**Phase 3**: タスク実行 (/speckit.tasksコマンドがtasks.mdを作成)
**Phase 4**: 実装 (tasks.mdを実行、TDDサイクル厳守)
**Phase 5**: 検証 (テスト実行、quickstart.md実行、パフォーマンス検証)

## 複雑さトラッキング
*憲章チェックに正当化が必要な違反がある場合のみ記入*

違反なし。すべての憲章要件を満たしています。

## 進捗トラッキング
*このチェックリストは実行フロー中に更新される*

**フェーズステータス**:
- [x] Phase 0: Research完了 (/speckit.plan コマンド)
- [x] Phase 1: Design完了 (/speckit.plan コマンド)
- [x] Phase 2: Task planning完了 (/speckit.plan コマンド - アプローチ記述済み)
- [ ] Phase 3: Tasks生成済み (/speckit.tasks コマンド)
- [ ] Phase 4: 実装完了
- [ ] Phase 5: 検証合格

**ゲートステータス**:
- [x] 初期憲章チェック: 合格
- [x] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱を文書化済み（違反なし）

---
*CLAUDE.md 開発指針に基づく*
