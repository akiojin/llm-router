# 実装計画: ノード自己登録システム

**機能ID**: `SPEC-94621a1f` | **日付**: 2025-10-31 | **仕様**: [spec.md](./spec.md)
**ステータス**: ✅ 実装完了 (PR #1)
**元のSPEC**: SPEC-32e2b31aから分割

## 概要

各マシンでノードアプリケーションを起動し、ルーターに自動的に登録される機能。ノードはルーターとの接続状態を管理し、ハートビートを送信する。

**主要機能**:
- ノード登録API (`POST /api/agents/register`)
- ノード一覧API (`GET /api/agents`)
- ハートビート送信 (`POST /api/agents/:id/heartbeat`)
- JSONファイルベースの永続化

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+ (stable)
**主要依存関係**:
- `axum` - Web APIフレームワーク
- `tokio` - 非同期ランタイム
- `serde` / `serde_json` - JSONシリアライゼーション
- `uuid` - ノードID生成
- `chrono` - タイムスタンプ管理

**ストレージ**: JSONファイル (`~/.llm-router/agents.json`)
**テスト**: `cargo test`（単体・統合テスト）
**対象プラットフォーム**: Linuxサーバー
**プロジェクトタイプ**: single（バイナリクレート）
**パフォーマンス目標**: ノード登録API < 100ms、最大1000ノード管理
**制約**: 認証機能なし（将来拡張候補）
**スケール/スコープ**: 1000ノード以下

## 憲章チェック
*ゲート: Phase 0 research前に合格必須。Phase 1 design後に再チェック。*

**シンプルさ**:
- プロジェクト数: 1（coordinatorプロジェクトに統合）✅
- フレームワークを直接使用? ✅ Yes（Axum直接使用）
- 単一データモデル? ✅ Yes（Agent構造体のみ）
- パターン回避? ✅ Yes（Repository/UoW不使用）

**アーキテクチャ**:
- すべての機能をライブラリとして? N/A（coordinatorはバイナリクレート）
- ライブラリリスト: coordinator/src（既存）
- ライブラリごとのCLI: coordinator binary
- ライブラリドキュメント: llms.txt形式を計画? N/A

**テスト (妥協不可)**:
- RED-GREEN-Refactorサイクルを強制? ✅ Yes
- Gitコミットはテストが実装より先に表示? ✅ Yes
- 順序: Contract→Integration→E2E→Unitを厳密に遵守? ✅ Yes
- 実依存関係を使用? ✅ Yes（実AgentRegistry、モックなし）
- Integration testの対象: APIエンドポイント、ファイルI/O
- 禁止: テスト前の実装、REDフェーズのスキップ ✅

**可観測性**:
- 構造化ロギング含む? ✅ Yes（tracingクレート使用）
- フロントエンドログ → バックエンド? N/A
- エラーコンテキスト十分? ✅ Yes

**バージョニング**:
- バージョン番号割り当て済み? ✅ Yes（Cargo.toml）
- 変更ごとにBUILDインクリメント? ✅ Yes
- 破壊的変更を処理? ✅ Yes（API v1バージョニング）

## プロジェクト構造

### ドキュメント (この機能)
```
specs/SPEC-94621a1f/
├── spec.md              # 機能仕様（既存）
├── plan.md              # このファイル（実装完了後のドキュメント）
└── tasks.md             # タスク一覧（実装完了後のドキュメント）
```

### ソースコード (リポジトリルート)
```
common/
├── src/
│   └── types.rs         # Agent, AgentStatus, SystemInfo構造体

coordinator/
├── src/
│   ├── api/
│   │   ├── mod.rs
│   │   └── agent.rs     # ノード登録・一覧・ハートビートAPI
│   ├── db/
│   │   └── mod.rs       # JSONファイルストレージ
│   ├── registry/
│   │   └── mod.rs       # AgentRegistry（メモリ上のノード管理）
│   └── main.rs          # エントリーポイント
└── tests/
    └── agent_test.rs    # 統合テスト

agent/
├── src/
│   └── main.rs          # ノードクライアント
└── Cargo.toml
```

**構造決定**: Rustワークスペース構成、common（共通型）、coordinator（サーバー）、agent（クライアント）

## Phase 0: アウトライン＆リサーチ

### 技術選択の理由

#### ストレージ: JSONファイル
**決定**: SQLiteではなくJSONファイル (`~/.llm-router/agents.json`)

**理由**:
1. **シンプルさの極限を追求**（CLAUDE.md準拠）
2. スキーマ不要（構造体をそのままシリアライズ）
3. デバッグが容易（ファイルを直接確認可能）
4. 依存関係の最小化（SQLiteクレート不要）
5. バックアップが簡単（ファイルコピーのみ）

**検討した代替案**:
- **SQLite**: リレーショナルDB、トランザクションサポート、複雑なクエリ可能
- **PostgreSQL**: 本格的なDB、高機能だが過剰
- **却下理由**: 1000ノード以下では、JSONファイルで十分。複雑さを避けるため。

#### 非同期ランタイム: Tokio
**決定**: Tokio

**理由**:
1. Axumの標準ランタイム
2. 成熟したエコシステム
3. 優れたドキュメント

**検討した代替案**:
- **async-std**: Tokioの代替、しかしエコシステムが小さい
- **却下理由**: Axumとの互換性を優先

#### Web フレームワーク: Axum
**決定**: Axum

**理由**:
1. Tokio公式フレームワーク
2. 型安全なルーティング
3. ミドルウェアサポート
4. 高パフォーマンス

**検討した代替案**:
- **Actix-web**: 高速だが複雑
- **Rocket**: シンプルだが非同期サポートが後発
- **却下理由**: AxumはTokioとの統合が最も良く、型安全性が高い

### 要明確化の解決

すべての技術選択が完了し、要明確化事項はありません。

## Phase 1: 設計＆契約（実装済み）

### API契約設計

#### 1. ノード登録API
- **エンドポイント**: `POST /api/agents/register`
- **リクエストボディ**: JSON
```json
{
  "machine_name": "server-01",
  "ip_address": "192.168.1.100",
  "runtime_version": "0.1.0",
  "system_info": {
    "os": "Linux",
    "arch": "x86_64",
    "cpu_cores": 8,
    "total_memory": 16777216
  }
}
```
- **レスポンス**: JSON
```json
{
  "id": "123e4567-e89b-12d3-a456-426614174000",
  "status": "registered"
}
```

#### 2. ノード一覧API
- **エンドポイント**: `GET /api/agents`
- **レスポンス**: JSON配列
```json
[
  {
    "id": "123e4567-e89b-12d3-a456-426614174000",
    "machine_name": "server-01",
    "ip_address": "192.168.1.100",
    "status": "Online",
    "runtime_version": "0.1.0",
    "registered_at": "2025-10-30T10:00:00Z",
    "last_seen": "2025-10-30T12:30:00Z"
  }
]
```

#### 3. ハートビートAPI
- **エンドポイント**: `POST /api/agents/:id/heartbeat`
- **レスポンス**: HTTP 200 OK

### データモデル（実装済み）

**Agent構造体** (`common/src/types.rs`):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub machine_name: String,
    pub ip_address: String,
    pub runtime_version: String,
    pub status: AgentStatus,
    pub registered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub system_info: SystemInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentStatus {
    Online,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
    pub cpu_cores: u32,
    pub total_memory: u64,
}
```

### アーキテクチャ決定（実装済み）

**レイヤー構成**:
1. **API層** (`coordinator/src/api/agent.rs`): HTTPリクエスト処理
2. **Registry層** (`coordinator/src/registry/mod.rs`): メモリ上のノード管理
3. **Storage層** (`coordinator/src/db/mod.rs`): ファイルI/O

**状態管理**:
- `Arc<RwLock<HashMap<Uuid, Agent>>>` でノード情報を管理
- 書き込み時: `RwLock::write()` → Registry更新 → ファイル保存
- 読み込み時: `RwLock::read()` → Registry参照

## Phase 2: タスク計画アプローチ（実装完了）

タスクは実装完了後にtasks.mdとしてドキュメント化しました。

**実装フロー**:
1. Setup: プロジェクト構造、依存関係
2. Tests: Contract tests, Integration tests
3. Core: Agent構造体、AgentRegistry、JSONストレージ
4. API: 登録・一覧・ハートビートエンドポイント
5. Integration: 全機能の統合テスト
6. Bug fixes: clippy警告修正、SQLite→JSON移行

**総開発時間**: 約8時間（30タスク）

## Phase 3+: 実装完了

**Phase 3**: タスク実行（完了）
**Phase 4**: 実装（完了、PR #1でマージ）
**Phase 5**: 検証（完了、全テスト合格）

## 実装上の決定事項

### SQLite から JSONファイルへの移行
**理由**: シンプルさの追求（CLAUDE.md原則）
**実装**: `coordinator/src/db/mod.rs`でJSONファイルI/O実装
**課題**: 初期実装でSQLiteを使用していたが、複雑すぎると判断
**解決策**: `serde_json::to_string_pretty()`で読みやすいJSON出力

### ハートビート自動復旧
**仕様**: ハートビート受信時、ステータスを自動的に`Online`に更新
**理由**: ノード再起動時の手動復旧を不要にするため
**実装**: `POST /api/agents/:id/heartbeat` で`status = AgentStatus::Online`を設定

### AtomicUsizeによるラウンドロビン
**仕様**: ノード選択時、AtomicUsizeでインデックス管理
**理由**: スレッドセーフで効率的
**実装**: `AgentRegistry::select_available_agent()`で`fetch_add(1, Ordering::Relaxed)`使用

## 複雑さトラッキング

違反なし。すべての憲章要件を満たしています。

## 進捗トラッキング

**フェーズステータス**:
- [x] Phase 0: Research完了
- [x] Phase 1: Design完了
- [x] Phase 2: Task planning完了
- [x] Phase 3: Tasks実行完了
- [x] Phase 4: 実装完了（PR #1マージ済み）
- [x] Phase 5: 検証合格（全テスト成功）

**ゲートステータス**:
- [x] 初期憲章チェック: 合格
- [x] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱を文書化済み（違反なし）

## 実装済み機能（PR #1）

✅ ノード登録API
✅ ノード一覧API
✅ ハートビート機能
✅ JSONファイルベースの永続化
✅ 統合テスト
✅ Clippy警告修正
✅ SQLite→JSON移行

## 今後の拡張候補

1. **認証機能**: APIキーまたはトークンベース認証
2. **ノード削除API**: `DELETE /api/agents/:id`
3. **ノード更新API**: `PUT /api/agents/:id`
4. **バックアップ機能**: JSONファイルの自動バックアップ
5. **PostgreSQL対応**: 大規模環境向けのDB切り替え

---
*このplan.mdは実装完了後のドキュメントとして作成されました*
