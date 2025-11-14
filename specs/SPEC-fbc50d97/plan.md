# 実装計画: リクエスト/レスポンス履歴保存機能

**機能ID**: `SPEC-fbc50d97` | **日付**: 2025-11-03 | **仕様**: [spec.md](./spec.md)
**入力**: `/specs/SPEC-fbc50d97/spec.md`の機能仕様

## 実行フロー (/speckit.plan コマンドのスコープ)

```
1. 入力パスから機能仕様を読み込み ✓
2. 技術コンテキストを記入 ✓
3. 憲章チェックセクションを評価 ✓
4. Phase 0 を実行 → research.md
5. Phase 1 を実行 → contracts, data-model.md, quickstart.md
6. 憲章チェックセクションを再評価
7. Phase 2 を計画 → タスク生成アプローチを記述
8. 停止 - /speckit.tasks コマンドの準備完了
```

## 概要

コーディネーターが受信するリクエストとエージェントから返されるレスポンスを
JSONファイルに保存し、Webダッシュボードで履歴を可視化する機能。
7日間のデータ保持、フィルタリング、詳細表示、エクスポート機能を提供する。

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+
**主要依存関係**: Axum (WebAPI), Tokio (非同期ランタイム), serde/serde_json
(JSON処理), chrono (日時処理), uuid (識別子生成)
**ストレージ**: JSONファイル (`~/.ollama-coordinator/request_history.json`)
**テスト**: cargo test (unit/integration/e2e)
**対象プラットフォーム**: Linux server (ubuntu-latest, windows-latest対応)
**プロジェクトタイプ**: single (既存の coordinator クレート内に実装)
**パフォーマンス目標**: プロキシオーバーヘッド < 5ms, ダッシュボード初期表示 < 1秒
**制約**: 非同期保存必須, ストリーミング対応, ファイルロック（排他制御）
**スケール/スコープ**: 7日間で10,000+ レコード, 100件/ページのページネーション

## 憲章チェック

*ゲート: Phase 0 research前に合格必須。Phase 1 design後に再チェック。*

**シンプルさ**:
- プロジェクト数: 1 (coordinatorクレートのみ) ✓
- フレームワークを直接使用? Yes (Axum直接使用、ラッパーなし) ✓
- 単一データモデル? Yes (RequestResponseRecord構造体のみ) ✓
- パターン回避? Yes (Repository パターン不使用、直接ファイルI/O) ✓

**アーキテクチャ**:
- すべての機能をライブラリとして? Yes (coordinator/src/ 以下にモジュール実装) ✓
- ライブラリリスト:
  - `coordinator::db::request_history` - ストレージ層
  - `coordinator::api::proxy` - プロキシ + キャプチャ機能
  - `coordinator::api::dashboard` - ダッシュボードAPI
- ライブラリごとのCLI: `ollama-coordinator --help/--version` (既存CLIを拡張) ✓
- ライブラリドキュメント: llms.txt形式を計画? 既存パターンに従う

**テスト (妥協不可)**:
- RED-GREEN-Refactorサイクルを強制? Yes ✓
- Gitコミットはテストが実装より先に表示? Yes (TDD厳守) ✓
- 順序: Contract→Integration→E2E→Unitを厳密に遵守? Yes ✓
- 実依存関係を使用? Yes (実ファイルシステム、実HTTP) ✓
- Integration testの対象: 新しいストレージ層、プロキシ統合、ダッシュボードAPI ✓
- 禁止: テスト前の実装、REDフェーズのスキップ ✓

**可観測性**:
- 構造化ロギング含む? Yes (tracing クレート使用、既存パターン準拠) ✓
- フロントエンドログ → バックエンド? N/A (フロントエンドはJavaScript、
  バックエンドログのみ)
- エラーコンテキスト十分? Yes (エラーチェーンとトレースID) ✓

**バージョニング**:
- バージョン番号割り当て済み? 既存バージョンに追随 (MINOR または PATCH) ✓
- 変更ごとにBUILDインクリメント? `npm version` コマンド使用 ✓
- 破壊的変更を処理? なし（新機能追加のみ、既存APIに影響なし） ✓

## プロジェクト構造

### ドキュメント (この機能)

```
specs/SPEC-fbc50d97/
├── spec.md              # 機能仕様 (/speckit.specify コマンド出力) ✓
├── plan.md              # このファイル (/speckit.plan コマンド出力)
├── research.md          # Phase 0 出力 (/speckit.plan コマンド)
├── data-model.md        # Phase 1 出力 (/speckit.plan コマンド)
├── quickstart.md        # Phase 1 出力 (/speckit.plan コマンド)
├── contracts/           # Phase 1 出力 (/speckit.plan コマンド)
└── tasks.md             # Phase 2 出力 (/speckit.tasks コマンド)
```

### ソースコード (リポジトリルート)

```
coordinator/
├── src/
│   ├── db/
│   │   ├── mod.rs                 # 既存（エージェント保存）
│   │   └── request_history.rs     # NEW: リクエスト履歴保存
│   ├── api/
│   │   ├── proxy.rs               # MODIFY: キャプチャ機能追加
│   │   └── dashboard.rs           # MODIFY: 履歴エンドポイント追加
│   └── web/
│       └── static/
│           ├── index.html         # MODIFY: 履歴タブ追加
│           ├── app.js             # MODIFY: 履歴UI実装
│           └── styles.css         # MODIFY: スタイル追加
│
common/
└── src/
    └── protocol.rs                # MODIFY: RequestResponseRecord追加

tests/
├── contract/
│   └── request_history_api_test.rs # NEW: API契約テスト
├── integration/
│   ├── request_capture_test.rs     # NEW: キャプチャ統合テスト
│   └── request_storage_test.rs     # NEW: ストレージ統合テスト
└── e2e/
    └── request_history_flow_test.rs # NEW: E2Eフロー
```

**構造決定**: 既存の単一プロジェクト構造を維持し、coordinator クレート内に
機能を追加

## Phase 0: アウトライン＆リサーチ

### 不明点の抽出と解決

1. **ストリーミングレスポンスのキャプチャ方法**
   - 決定: `hyper::Body` をバッファリングしてからクライアントに転送
   - 理由: レスポンス全体を保存するため、完全なバッファリングが必要
   - 検討した代替案:
     - チャンクごとに保存 → 再構築が複雑
     - T字パイプ → Axum + Tokioでの実装が複雑
   - トレードオフ: メモリ使用量増加（大きなレスポンス時）、但し一時的

2. **非同期ファイル保存の実装**
   - 決定: `tokio::spawn` で別タスクとして保存処理を実行
   - 理由: プロキシのレスポンス返却を待たせない
   - 検討した代替案:
     - チャネル経由のワーカータスク → 過剰設計
     - 同期保存 → レスポンスタイム悪化
   - パターン: Fire-and-forget (保存失敗はログ記録のみ)

3. **7日間のデータクリーンアップ**
   - 決定: 定期タスク（`tokio::time::interval`）で1時間ごとに実行
   - 理由: 即座のクリーンアップは不要、バッチ処理で十分
   - 検討した代替案:
     - 保存時にクリーンアップ → 毎回の処理コスト高
     - 起動時のみ → 長時間稼働時に肥大化
   - タイミング: サーバー起動時 + 1時間ごと

4. **大量レコードのフィルタリング実装**
   - 決定: メモリ内でのイテレータフィルタ + ページネーション
   - 理由: JSONファイルは全読み込み、データ量は7日間で管理可能
   - 検討した代替案:
     - SQLiteインデックス → 憲章違反（JSONファイル必須）
     - 複数ファイル分割 → 読み込み複雑化
   - スケール: 10,000件 × 平均10KB = 100MB程度、問題なし

5. **CSVエクスポートの実装**
   - 決定: `csv` クレート使用、メモリ内でCSV生成してレスポンス
   - 理由: 標準的なアプローチ、ストリーミング不要（データ量小）
   - パターン: JSON → Struct → CSV

### 技術選択のベストプラクティス

**Axumでのファイルダウンロード**:
- `Response::builder()` + `Content-Disposition: attachment` ヘッダー
- `application/json` または `text/csv` の Content-Type

**Tokioでの定期タスク**:
- `tokio::spawn` + `tokio::time::interval` パターン
- Graceful shutdown対応（Cancellation Token使用）

**ファイルロックの実装**:
- `Arc<Mutex<()>>` でファイルアクセスを排他制御
- `tokio::fs` の非同期ファイルI/O使用

**出力**: すべての要明確化が解決された `research.md` を作成済み

## Phase 1: 設計＆契約

### 1. データモデル (`data-model.md`)

**RequestResponseRecord構造体** (`common/src/protocol.rs`):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestResponseRecord {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub request_type: RequestType,
    pub model: String,
    pub agent_id: Uuid,
    pub agent_machine_name: String,
    pub agent_ip: IpAddr,
    pub request_body: serde_json::Value,
    pub response_body: Option<serde_json::Value>,
    pub duration_ms: u64,
    pub status: RecordStatus,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RequestType {
    Chat,
    Generate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecordStatus {
    Success,
    Error { message: String },
}
```

**関係性**:
- `Agent` (既存) ← (N:1) → `RequestResponseRecord` (agent_id で参照)

### 2. API契約 (`contracts/`)

**契約ファイル**: `contracts/dashboard-history-api.json` (OpenAPI 3.0 subset)

```json
{
  "GET /api/dashboard/request-responses": {
    "query_params": {
      "model": "string (optional)",
      "agent_id": "uuid (optional)",
      "status": "success|error (optional)",
      "start_time": "ISO8601 (optional)",
      "end_time": "ISO8601 (optional)",
      "page": "integer (default: 1)",
      "per_page": "integer (default: 100, max: 500)"
    },
    "response_200": {
      "records": "Array<RequestResponseRecord>",
      "total_count": "integer",
      "page": "integer",
      "per_page": "integer"
    }
  },
  "GET /api/dashboard/request-responses/:id": {
    "path_params": {
      "id": "uuid"
    },
    "response_200": "RequestResponseRecord",
    "response_404": { "error": "Record not found" }
  },
  "GET /api/dashboard/request-responses/export": {
    "query_params": {
      "format": "json|csv",
      "... (same filters as list endpoint)"
    },
    "response_200": "File download (application/json or text/csv)",
    "headers": {
      "Content-Disposition": "attachment; filename=history.{format}"
    }
  }
}
```

### 3. 契約テスト (`tests/contract/`)

**ファイル**: `tests/contract/request_history_api_test.rs`

```rust
#[tokio::test]
async fn test_list_request_responses_contract() {
    // Arrange: テストサーバー起動、データなし
    // Act: GET /api/dashboard/request-responses
    // Assert:
    //   - Status: 200
    //   - Body: { records: [], total_count: 0, page: 1, per_page: 100 }
    //   - Content-Type: application/json
}

#[tokio::test]
async fn test_get_request_response_detail_contract() {
    // RED: エンドポイント未実装なので失敗
}

#[tokio::test]
async fn test_export_request_responses_contract() {
    // RED: エンドポイント未実装なので失敗
}
```

### 4. Integration テストシナリオ

**ユーザーストーリー1 → Integration Test**:
```rust
// tests/integration/request_capture_test.rs
#[tokio::test]
async fn test_request_is_captured_and_stored() {
    // 1. コーディネーター起動
    // 2. テストエージェント登録
    // 3. /api/chat にリクエスト送信
    // 4. request_history.json にレコードが保存されることを確認
    // 5. レコードの内容が正しいことを検証
}
```

**ユーザーストーリー2 → Integration Test**:
```rust
// tests/integration/request_storage_test.rs
#[tokio::test]
async fn test_failed_request_is_captured_with_error() {
    // エラーが発生したリクエストもエラー情報付きで保存されることを確認
}
```

### 5. エージェントファイル更新

**CLAUDE.md の更新**:
- 現在の目的セクションに「リクエスト/レスポンス履歴機能の実装」を追加
- 既存の開発指針を維持
- トークン効率のため、変更なし（このタスク完了後に更新）

**出力**: data-model.md, contracts/, 失敗する契約テスト, quickstart.md

## Phase 2: タスク計画アプローチ

*このセクションは /speckit.tasks コマンドが実行することを記述*

**タスク生成戦略**:

1. **Setup タスク**:
   - [P] `RequestResponseRecord` 構造体定義（common/src/protocol.rs）
   - [P] ストレージディレクトリ初期化テスト

2. **Contract Test タスク**:
   - [P] List API 契約テスト
   - [P] Detail API 契約テスト
   - [P] Export API 契約テスト

3. **Integration Test タスク** (依存順):
   - ストレージ層保存テスト (request_history.rs)
   - ストレージ層読み込みテスト
   - ストレージ層クリーンアップテスト
   - プロキシキャプチャテスト (proxy.rs統合)
   - ストリーミングキャプチャテスト

4. **Core 実装タスク** (TDD: Test → Impl):
   - ストレージ層実装: `save_record()`
   - ストレージ層実装: `load_records()`
   - ストレージ層実装: `cleanup_old_records()`
   - プロキシ修正: キャプチャロジック追加
   - ダッシュボードAPI: List エンドポイント
   - ダッシュボードAPI: Detail エンドポイント
   - ダッシュボードAPI: Export エンドポイント

5. **UI 実装タスク**:
   - HTML: 履歴タブ追加
   - JavaScript: 履歴リスト表示
   - JavaScript: 詳細モーダル
   - JavaScript: フィルタ機能
   - JavaScript: エクスポート機能
   - CSS: スタイル調整

6. **E2E Test タスク**:
   - E2Eフロー: リクエスト → 保存 → ダッシュボード表示

7. **Polish タスク**:
   - ドキュメント更新 (README.md)
   - ローカル検証 (`make quality-checks`)
   - CLAUDE.md 更新

**順序戦略**:
- TDD順序厳守: Contract Test → Integration Test → Impl → E2E
- 並列実行可能: Setup, Contract tests は並列実行可 [P]
- ストレージ層 → プロキシ層 → API層 → UI層の依存関係順

**推定出力**: tasks.mdに約30個の番号付き、順序付きタスク

**重要**: このフェーズは /speckit.tasks コマンドで実行、/speckit.plan ではない

## Phase 3+: 今後の実装

*これらのフェーズは /plan コマンドのスコープ外*

**Phase 3**: タスク生成 (/speckit.tasks コマンドが tasks.md を作成)
**Phase 4**: 実装 (tasks.md を TDD で実行)
**Phase 5**: 検証 (全テスト合格、quickstart.md 実行、パフォーマンス確認)

## 複雑さトラッキング

*憲章チェックに正当化が必要な違反がある場合のみ記入*

| 違反 | 必要な理由 | より単純な代替案が却下された理由 |
| --- | --- | --- |
| なし | - | - |

すべての憲章要件を満たしています。

## 進捗トラッキング

*このチェックリストは実行フロー中に更新される*

**フェーズステータス**:
- [x] Phase 0: Research完了 (/speckit.plan コマンド)
- [x] Phase 1: Design完了 (/speckit.plan コマンド)
- [x] Phase 2: Task planning完了 (/speckit.plan コマンド - アプローチのみ記述)
- [ ] Phase 3: Tasks生成済み (/speckit.tasks コマンド)
- [ ] Phase 4: 実装完了
- [ ] Phase 5: 検証合格

**ゲートステータス**:
- [x] 初期憲章チェック: 合格
- [x] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱を文書化済み (なし)

---

*憲章 v1.0.0 に基づく - `/memory/constitution.md` 参照*
