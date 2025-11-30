# 実装計画: ルーター主導のモデル自動配布機能

**機能ID**: `SPEC-8ae67d67` | **日付**: 2025-11-12 | **仕様**: [spec.md](./spec.md)
**入力**: `/specs/SPEC-8ae67d67/spec.md`の機能仕様

## 実行フロー

```
1. ✅ 入力パスから機能仕様を読み込み
2. ✅ 技術コンテキストを記入
3. → 憲章チェックセクションを評価
4. → Phase 0 を実行 → research.md
5. → Phase 1 を実行 → contracts, data-model.md, quickstart.md
6. → 憲章チェックセクションを再評価
7. → Phase 2 を計画 → タスク生成アプローチを記述
8. → 停止 - /speckit.tasks コマンドの準備完了
```

## 概要

ルーターから各ノードのOllamaモデルダウンロードを一元制御する機能を実装します。ノード登録時にGPU能力に応じた自動配布、ダッシュボードからの手動配布、モデル情報の可視化を提供し、運用効率を向上させます。

**主要要件**:
- ノード登録時のGPU能力ベース自動モデル配布
- ダッシュボードからの個別/一括モデル配布
- Ollama公式ライブラリAPIからのモデル一覧取得
- リアルタイム進捗表示
- 既存のノード自律ダウンロード機能との独立併存

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+, JavaScript ES6+
**主要依存関係**:
- Backend: axum (Webフレームワーク), tokio (非同期ランタイム), reqwest (HTTPクライアント), serde (シリアライゼーション)
- Frontend: Vanilla JavaScript (既存ダッシュボード)
**ストレージ**: JSONファイル（agents.json）、メモリ（HashMap）
**テスト**: cargo test, integration tests with real HTTP endpoints
**対象プラットフォーム**: Linux/macOS/Windows server
**プロジェクトタイプ**: single (coordinator + agent in same repo)
**パフォーマンス目標**:
- モデル一覧取得: <10秒
- 進捗更新: <5秒間隔
- 同時ダウンロードタスク: 10個
**制約**:
- Ollama公式ライブラリAPIへのインターネット接続必須
- ノード側ディスク容量依存
- ネットワーク安定性必須（リアルタイム進捗更新）
**スケール/スコープ**: 10-50ノード、数十～数百のモデル

## 憲章チェック

**シンプルさ**:
- プロジェクト数: 2 (coordinator, agent) ✅
- フレームワークを直接使用? Yes (axum直接使用、ラッパーなし) ✅
- 単一データモデル? Yes (Agent構造体、ModelInfo構造体) ✅
- パターン回避? Yes (Repository/UoWパターン不使用、直接HashMap操作) ✅

**アーキテクチャ**:
- すべての機能をライブラリとして? Partial (coordinator/agentはバイナリだが、共通ロジックはモジュール化)
- ライブラリリスト:
  - `coordinator::registry`: ノード管理
  - `coordinator::api`: REST APIハンドラー
  - `agent::ollama`: Ollama通信
- ライブラリごとのCLI:
  - `llm-router --help/--version`
  - `llm-node --help/--version`
- ライブラリドキュメント: 既存のREADME.mdに追記予定

**テスト (妥協不可)**:
- RED-GREEN-Refactorサイクルを強制? Yes ✅
- Gitコミットはテストが実装より先に表示? Yes ✅
- 順序: Contract→Integration→E2E→Unitを厳密に遵守? Yes ✅
- 実依存関係を使用? Yes (実HTTPエンドポイント、実ファイルI/O) ✅
- Integration testの対象: 新API、Ollama通信、ダッシュボード連携 ✅
- 禁止: テスト前の実装、REDフェーズのスキップ ✅

**可観測性**:
- 構造化ロギング含む? Yes (tracing crateで実装済み) ✅
- フロントエンドログ → バックエンド? N/A (ダッシュボードは最小限のUI)
- エラーコンテキスト十分? Yes (エラーメッセージにコンテキスト含む) ✅

**バージョニング**:
- バージョン番号割り当て済み? Yes (semantic-releaseで自動管理) ✅
- 変更ごとにBUILDインクリメント? Yes (CIで自動) ✅
- 破壊的変更を処理? Yes (API v1プレフィックス、将来のv2対応可能) ✅

## プロジェクトディレクトリ構造

### ドキュメント (この機能)

```
specs/SPEC-8ae67d67/
├── spec.md              # 機能仕様（完成）
├── plan.md              # このファイル（進行中）
├── research.md          # Phase 0 出力（次のステップ）
├── data-model.md        # Phase 1 出力
├── quickstart.md        # Phase 1 出力
├── contracts/           # Phase 1 出力
│   ├── models-api.yaml  # モデル関連APIのOpenAPI仕様
│   └── agent-api.yaml   # ノード関連APIのOpenAPI仕様拡張
└── tasks.md             # Phase 2 出力 (/speckit.tasks コマンド)
```

### ソースコード (リポジトリルート)

```
coordinator/
├── src/
│   ├── api/
│   │   ├── agent.rs      # 既存: ノード登録API
│   │   ├── health.rs     # 既存: ヘルスチェックAPI
│   │   ├── proxy.rs      # 既存: プロキシAPI
│   │   └── models.rs     # 新規: モデル管理API
│   ├── registry/
│   │   ├── mod.rs        # 既存: ノードレジストリ
│   │   └── models.rs     # 新規: モデル状態管理
│   ├── ollama/
│   │   ├── mod.rs        # 新規: Ollama公式ライブラリAPI通信
│   │   └── client.rs     # 新規: HTTPクライアント
│   └── main.rs           # 既存: メインエントリーポイント
├── tests/
│   ├── contract/
│   │   ├── models_api_test.rs   # 新規: モデルAPI契約テスト
│   │   └── agent_api_test.rs    # 既存拡張: ノードAPI契約テスト
│   ├── integration/
│   │   ├── auto_download_test.rs        # 新規: 自動配布統合テスト
│   │   ├── manual_distribution_test.rs  # 新規: 手動配布統合テスト
│   │   └── model_info_test.rs           # 新規: モデル情報統合テスト
│   └── unit/
│       ├── gpu_model_selector_test.rs   # 新規: GPU能力ベース選択ロジック
│       └── model_repository_test.rs     # 新規: モデル状態管理ロジック
└── static/
    ├── index.html        # 既存拡張: ダッシュボードHTML
    ├── app.js            # 既存拡張: ダッシュボードロジック
    └── models.js         # 新規: モデル管理UI

agent/
├── src/
│   ├── ollama.rs         # 既存拡張: モデルプル機能拡張
│   └── main.rs           # 既存: メインエントリーポイント
└── tests/
    └── integration/
        └── model_pull_test.rs   # 新規: モデルプル統合テスト
```

**構造決定**: 単一リポジトリ（coordinator + agent）、Rust Workspace構成

## Phase 0: アウトライン＆リサーチ

**リサーチタスク**:

1. **Ollama公式ライブラリAPI調査**:
   - タスク: "Research Ollama Library API for model listing"
   - 調査項目:
     - APIエンドポイント（<https://ollama.com/library> または <https://ollama.ai/library>）
     - レスポンス形式（JSON構造）
     - 認証の有無
     - レート制限
     - 代替手段（ollama list コマンド経由）
   - 決定事項:
     - 使用するAPIエンドポイント
     - フォールバック戦略

2. **リアルタイム進捗更新方式**:
   - タスク: "Research real-time progress update patterns"
   - 調査項目:
     - WebSocket vs Server-Sent Events vs Long Polling
     - axumでのWebSocket実装パターン
     - 既存ダッシュボードとの統合方法
   - 決定事項:
     - 進捗更新プロトコル
     - ポーリング間隔またはイベント駆動

3. **モデルダウンロードタスク管理**:
   - タスク: "Research task queue patterns in Rust"
   - 調査項目:
     - tokioのチャネルパターン
     - 非同期タスクスケジューリング
     - 進捗追跡の実装パターン
   - 決定事項:
     - タスクキュー実装方式
     - 状態管理方法（メモリ vs 永続化）

4. **GPU能力判定ロジック**:
   - タスク: "Review existing GPU memory detection code"
   - 調査項目:
     - 既存のagent/src/gpu.rs実装
     - GPUメモリサイズ取得方法
     - モデルサイズとの対応関係
   - 決定事項:
     - メモリサイズ → モデル選択マッピング

**出力**: `research.md` に上記すべての調査結果を統合

## Phase 1: 設計＆契約

*前提条件: research.md完了*

### 1. データモデル設計

`data-model.md` に記載:

**既存エンティティ拡張**:
- `Agent` 構造体:
  - 新規フィールド: `installed_models: Vec<InstalledModel>`
  - 新規フィールド: `downloading_models: Vec<DownloadTask>`

**新規エンティティ**:
- `ModelInfo`: Ollamaモデル情報
  - `name: String`
  - `size: u64` (bytes)
  - `description: String`
  - `required_memory: u64` (bytes)
  - `tags: Vec<String>`

- `InstalledModel`: ノードにインストール済みのモデル
  - `name: String`
  - `size: u64`
  - `installed_at: DateTime<Utc>`

- `DownloadTask`: ダウンロードタスク
  - `id: Uuid`
  - `agent_id: Uuid`
  - `model_name: String`
  - `status: DownloadStatus` (Pending/InProgress/Completed/Failed)
  - `progress: f32` (0.0-1.0)
  - `speed: Option<u64>` (bytes/sec)
  - `started_at: DateTime<Utc>`
  - `completed_at: Option<DateTime<Utc>>`
  - `error: Option<String>`

**状態遷移**:
- DownloadTask: Pending → InProgress → (Completed | Failed)

### 2. API契約設計

`contracts/models-api.yaml` (OpenAPI 3.0):

```yaml
/api/models/available:
  get:
    summary: ダウンロード可能なモデル一覧を取得
    responses:
      200:
        content:
          application/json:
            schema:
              type: object
              properties:
                models:
                  type: array
                  items: ModelInfo
                source:
                  type: string
                  enum: [ollama_library, agents]

/api/models/distribute:
  post:
    summary: モデルを配布
    requestBody:
      content:
        application/json:
          schema:
            type: object
            properties:
              model_name:
                type: string
              target:
                type: string
                enum: [all, specific, auto]
              agent_ids:
                type: array
                items:
                  type: string
                  format: uuid
    responses:
      202:
        content:
          application/json:
            schema:
              type: object
              properties:
                task_ids:
                  type: array
                  items:
                    type: string
                    format: uuid

/api/agents/{agent_id}/models:
  get:
    summary: ノードのインストール済みモデル一覧
    responses:
      200:
        content:
          application/json:
            schema:
              type: array
              items: InstalledModel

/api/agents/{agent_id}/models/pull:
  post:
    summary: 特定ノードにモデルダウンロード指示
    requestBody:
      content:
        application/json:
          schema:
            type: object
            properties:
              model_name:
                type: string
    responses:
      202:
        content:
          application/json:
            schema:
              type: object
              properties:
                task_id:
                  type: string
                  format: uuid

/api/tasks/{task_id}:
  get:
    summary: ダウンロードタスクの進捗取得
    responses:
      200:
        content:
          application/json:
            schema: DownloadTask
```

### 3. 契約テスト生成

`tests/contract/models_api_test.rs`:
- `test_get_available_models_contract()`
- `test_distribute_models_contract()`
- `test_get_agent_models_contract()`
- `test_pull_model_contract()`
- `test_get_task_progress_contract()`

各テストは:
1. APIエンドポイントを呼び出し
2. レスポンススキーマをアサート
3. 初期状態では失敗（未実装のため）

### 4. 統合テストシナリオ

**ユーザーストーリー1 → 統合テスト**:
`tests/integration/auto_download_test.rs`:
- `test_auto_download_on_registration_16gb_gpu()`
- `test_auto_download_on_registration_8gb_gpu()`
- `test_auto_download_on_registration_4_5gb_gpu()`
- `test_auto_download_on_registration_small_gpu()`
- `test_progress_display_during_download()`

**ユーザーストーリー2 → 統合テスト**:
`tests/integration/manual_distribution_test.rs`:
- `test_manual_distribution_to_specific_agent()`
- `test_bulk_distribution_to_all_agents()`
- `test_progress_tracking_multiple_agents()`
- `test_offline_agent_error_handling()`

**ユーザーストーリー3 → 統合テスト**:
`tests/integration/model_info_test.rs`:
- `test_list_available_models_from_ollama_library()`
- `test_list_installed_models_on_agent()`
- `test_model_matrix_view_multiple_agents()`

### 5. クイックスタートドキュメント

`quickstart.md`:

```markdown
# クイックスタート: モデル自動配布機能

## 前提条件
- llm-routerが起動している
- 少なくとも1つのノードが登録されている

## シナリオ1: ノード登録時の自動配布

1. 新しいノードを起動:
   \`\`\`bash
   llm-node --coordinator-url http://localhost:8080
   \`\`\`

2. ルーターログで自動配布を確認:
   \`\`\`
   [INFO] Agent registered: <agent_id>
   [INFO] Auto-downloading model: gpt-oss:20b (16GB+ GPU detected)
   [INFO] Download progress: 25%
   \`\`\`

3. ダッシュボードで進捗を確認:
   - http://localhost:8080/dashboard
   - ノード詳細 → ダウンロード進捗

## シナリオ2: ダッシュボードからの手動配布

1. ダッシュボードを開く: http://localhost:8080/dashboard
2. 「モデル管理」タブをクリック
3. ダウンロード可能なモデル一覧から選択
4. 配布先を選択（特定ノード or 全ノード）
5. 「ダウンロード」ボタンをクリック
6. 進捗をリアルタイムで確認

## シナリオ3: モデル情報の確認

1. ダッシュボードを開く
2. 「モデル管理」タブ → 「ダウンロード可能」サブタブ
3. 各ノードの詳細画面 → 「インストール済みモデル」セクション
```

### 6. ノードファイル更新

`CLAUDE.md` に追記:
```markdown
## モデル自動配布機能 (SPEC-8ae67d67)

### 概要
ルーターが各ノードのモデルダウンロードを制御

### 主要コンポーネント
- coordinator/src/api/models.rs: モデル管理API
- coordinator/src/registry/models.rs: モデル状態管理
- coordinator/src/ollama/mod.rs: Ollama公式API通信
- agent/src/ollama.rs: モデルプル機能拡張

### API エンドポイント
- GET /api/models/available
- POST /api/models/distribute
- GET /api/agents/{id}/models
- POST /api/agents/{id}/models/pull
- GET /api/tasks/{id}

### テストコマンド
- `cargo test --package coordinator --test models_api_test`
- `cargo test --package coordinator --test auto_download_test`
```

**出力**: data-model.md, contracts/*.yaml, 失敗するテスト, quickstart.md, CLAUDE.md更新

## Phase 2: タスク計画アプローチ

*このセクションは/speckit.tasksコマンドが実行することを記述 - /speckit.plan中は実行しない*

**タスク生成戦略**:

1. **契約からタスク生成**:
   - `contracts/models-api.yaml` の各エンドポイント → 契約テストタスク [P]
   - 各エンドポイント → 実装タスク

2. **データモデルからタスク生成**:
   - `ModelInfo` 構造体 → 定義タスク [P]
   - `InstalledModel` 構造体 → 定義タスク [P]
   - `DownloadTask` 構造体 → 定義タスク [P]

3. **ユーザーストーリーからタスク生成**:
   - ストーリー1 (自動配布) → 5つの統合テストタスク
   - ストーリー2 (手動配布) → 4つの統合テストタスク
   - ストーリー3 (可視化) → 3つの統合テストタスク
   - 各統合テスト → 実装タスク

4. **Phase 0リサーチからタスク生成**:
   - Ollama公式API調査 → 実装タスク
   - 進捗更新方式 → 実装タスク
   - タスクキュー → 実装タスク

**順序戦略**:

1. **Setup Phase** (並列可能):
   - データモデル定義 [P]
   - API契約定義 [P]
   - Ollama公式API通信モジュール [P]

2. **Test Phase** (TDD順序):
   - Contract tests (RED)
   - Integration tests (RED)
   - Unit tests (RED)

3. **Core Phase** (依存関係順):
   - モデル状態管理実装 (GREEN)
   - モデル管理API実装 (GREEN)
   - 自動配布ロジック実装 (GREEN)
   - 手動配布ロジック実装 (GREEN)

4. **Integration Phase**:
   - ダッシュボードUI拡張
   - ノード側モデルプル機能拡張
   - 進捗更新機能実装

5. **Polish Phase**:
   - エラーハンドリング強化
   - ログ追加
   - ドキュメント更新

**推定出力**: tasks.mdに35-40個の番号付き、順序付きタスク

**重要**: このフェーズは/speckit.tasksコマンドで実行、/speckit.planではない

## Phase 3+: 今後の実装

*これらのフェーズは/planコマンドのスコープ外*

**Phase 3**: タスク実行 (/speckit.tasksコマンドがtasks.mdを作成)
**Phase 4**: 実装 (憲章原則に従ってtasks.mdを実行)
**Phase 5**: 検証 (テスト実行、quickstart.md実行、パフォーマンス検証)

## 複雑さトラッキング

*現時点で憲章違反なし*

| 違反 | 必要な理由 | より単純な代替案が却下された理由 |
|------|-----------|--------------------------------|
| なし | - | - |

## 進捗トラッキング

**フェーズステータス**:
- [ ] Phase 0: Research完了 (/speckit.plan コマンド)
- [ ] Phase 1: Design完了 (/speckit.plan コマンド)
- [ ] Phase 2: Task planning完了 (/speckit.plan コマンド - アプローチのみ記述)
- [ ] Phase 3: Tasks生成済み (/speckit.tasks コマンド)
- [ ] Phase 4: 実装完了
- [ ] Phase 5: 検証合格

**ゲートステータス**:
- [x] 初期憲章チェック: 合格
- [ ] 設計後憲章チェック: 合格（Phase 1完了後）
- [ ] すべての要明確化解決済み（Phase 0完了後）
- [x] 複雑さの逸脱を文書化済み（なし）

---
*憲章に基づく - `/CLAUDE.md` 参照*
