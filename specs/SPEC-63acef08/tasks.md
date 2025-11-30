# タスク: 統一APIプロキシ

**ステータス**: ✅ **実装完了** (PR #1でマージ済み、2025-10-30)
**入力**: `/llm-router/specs/SPEC-63acef08/`の設計ドキュメント

## 実装済みタスク一覧

すべてのタスクは完了し、PR #1でmainブランチにマージ済みです。

---

## Phase 3.1: セットアップ

- [x] **T001** [P] 依存関係追加: `reqwest`, `AtomicUsize`使用のための`std::sync::atomic`
- [x] **T002** [P] モジュール宣言: `coordinator/src/api/proxy.rs` 作成
- [x] **T003** [P] HTTPクライアント初期化: AppStateに`reqwest::Client`追加

**実装時間**: 約20分

---

## Phase 3.2: テストファースト（TDD）

**実装完了**: すべてのテストはREDフェーズ確認後にGREENフェーズで実装

### Contract Tests

- [x] **T004** [P] `coordinator/tests/contract/proxy_chat_test.rs` に POST /api/chat のcontract test
  - リクエスト: ChatRequest（Ollama API互換）
  - 期待レスポンス: 200 OK, ChatResponse

- [x] **T005** [P] `coordinator/tests/contract/proxy_generate_test.rs` に POST /api/generate のcontract test
  - リクエスト: GenerateRequest（Ollama API互換）
  - 期待レスポンス: 200 OK, GenerateResponse

### Integration Tests

- [x] **T006** `coordinator/tests/integration/proxy_test.rs` にプロキシ基本動作テスト
  - 前提: 1台のノード登録済み
  - 実行: POST /api/chat でリクエスト送信
  - 検証: ノードにリクエストが転送され、レスポンスが返される

- [x] **T007** `coordinator/tests/integration/proxy_test.rs` にラウンドロビンテスト
  - 前提: 3台のノード登録済み
  - 実行: 9回連続でPOST /api/chat リクエスト送信
  - 検証: 各ノードが3リクエストずつ処理（均等分散）

- [x] **T008** `coordinator/tests/integration/proxy_test.rs` にノード不在エラーテスト
  - 前提: 登録されたノードなし
  - 実行: POST /api/chat リクエスト送信
  - 検証: 503 Service Unavailable、"No agents available"エラー

- [x] **T009** `coordinator/tests/integration/proxy_test.rs` にタイムアウトテスト
  - 前提: 応答しないモックノード登録
  - 実行: POST /api/chat リクエスト送信
  - 検証: 60秒後にタイムアウトエラー

**実装時間**: 約2時間

---

## Phase 3.3: コア実装

### ラウンドロビンロジック

- [x] **T010** `coordinator/src/registry/mod.rs` にround_robin_indexフィールド追加
  - フィールド: `round_robin_index: AtomicUsize`
  - 初期化: `AtomicUsize::new(0)`

- [x] **T011** `coordinator/src/registry/mod.rs` にselect_agent()メソッド実装
  - 機能: オンラインノード一覧取得 → ラウンドロビンで選択
  - アルゴリズム: `index % online_agents.len()`
  - ノード不在時: `None` 返却

### プロキシAPI層

- [x] **T012** [P] `common/src/protocol.rs` にChatRequest/ChatResponse実装
  - Ollama API互換の型定義
  - Derive: Debug, Clone, Serialize, Deserialize

- [x] **T013** [P] `common/src/protocol.rs` にGenerateRequest/GenerateResponse実装
  - Ollama API互換の型定義
  - Derive: Debug, Clone, Serialize, Deserialize

- [x] **T014** `coordinator/src/api/proxy.rs` にproxy_chat()ハンドラー実装
  - エンドポイント: POST /api/chat
  - 機能: select_agent() → HTTPリクエスト転送 → レスポンス返却
  - タイムアウト: 60秒

- [x] **T015** `coordinator/src/api/proxy.rs` にproxy_generate()ハンドラー実装
  - エンドポイント: POST /api/generate
  - 機能: select_agent() → HTTPリクエスト転送 → レスポンス返却
  - タイムアウト: 60秒

- [x] **T016** `coordinator/src/api/proxy.rs` にエラーハンドリング実装
  - AppError::NoAgents定義
  - AppError::RequestTimeout定義
  - AppError::AgentError定義

**実装時間**: 約2時間

---

## Phase 3.4: 統合

- [x] **T017** `coordinator/src/main.rs` にプロキシルート追加
  - /api/chat → proxy_chat
  - /api/generate → proxy_generate

- [x] **T018** `coordinator/src/main.rs` にHTTPクライアント初期化
  - `reqwest::Client::new()` でクライアント作成
  - AppStateに追加

- [x] **T019** 起動ログにプロキシエンドポイント情報追加
  - `tracing::info!("Proxy endpoints: /api/chat, /api/generate");`

**実装時間**: 約30分

---

## Phase 3.5: 仕上げ

### Unit Tests

- [x] **T020** [P] `coordinator/src/registry/mod.rs` にselect_agent()のunit test
  - ラウンドロビンインデックス更新テスト
  - オンラインノードのみ選択テスト
  - ノード不在時Noneテスト

- [x] **T021** [P] `common/src/protocol.rs` にプロトコル型のunit test
  - JSONシリアライゼーション/デシリアライゼーションテスト

### ドキュメント

- [x] **T022** [P] README.md に統一APIプロキシセクション追加
  - 使用例、エンドポイント説明
  - Ollama API互換性説明

- [x] **T023** [P] Rustdocコメント追加
  - すべてのpublic関数にドキュメントコメント
  - プロキシ動作の説明

**実装時間**: 約1時間

---

## タスク統計

| フェーズ | タスク数 | 完了 | 実装時間 |
|---------|---------|------|------------|
| Setup | 3 | 3 (100%) | 20分 |
| Tests | 6 | 6 (100%) | 2時間 |
| Core | 7 | 7 (100%) | 2時間 |
| Integration | 3 | 3 (100%) | 30分 |
| Polish | 4 | 4 (100%) | 1時間 |
| **合計** | **23** | **23 (100%)** | **約5.5時間** |

---

## 実装完了確認

**すべてのチェック項目クリア**:
- [x] すべてのcontractsに対応するテストがある
- [x] すべてのentitiesにmodelタスクがある
- [x] すべてのテストが実装より先にある（TDD遵守）
- [x] 並列タスクは本当に独立している
- [x] 各タスクは正確なファイルパスを指定
- [x] 同じファイルを変更する[P]タスクがない

**テスト結果**:
- ✅ すべてのテストが合格
- ✅ ラウンドロビン動作確認（9リクエスト → 3ノードに均等分散）
- ✅ エラーハンドリング正常動作
- ✅ cargo clippy: エラー/警告ゼロ
- ✅ cargo fmt --check: フォーマット準拠

**実装PR**: #1
**マージ日**: 2025-10-30
**レビュアー**: N/A（初期実装）

---

## 関連ドキュメント

- [機能仕様書](./spec.md)
- [実装計画](./plan.md)
- [データモデル](./data-model.md)（作成予定）
- [技術リサーチ](./research.md)（作成予定）
- [クイックスタート](./quickstart.md)（作成予定）
- [API契約](./contracts/proxy-api.yaml)（作成予定）
