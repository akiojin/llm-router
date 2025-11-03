# タスク: リクエスト/レスポンス履歴保存機能

**入力**: `/specs/SPEC-fbc50d97/`の設計ドキュメント
**前提条件**: plan.md (✓), research.md (✓), data-model.md (✓), contracts/ (✓)

## 実行フロー

1. ✅ 機能ディレクトリから plan.md を読み込み → 技術スタック、ライブラリ、構造を抽出
2. ✅ オプション設計ドキュメントを読み込み:
   - data-model.md: RequestResponseRecord エンティティ → model タスク
   - contracts/: dashboard-history-api.json → contract test タスク
   - research.md: 技術決定を抽出 → setup タスク
3. ✅ カテゴリ別にタスクを生成
4. ✅ タスクルールを適用（並列実行、TDD順序）
5. ✅ タスクを順次番号付け
6. ✅ 依存関係グラフを生成
7. ✅ タスク完全性を検証

## フォーマット: `[ID] [P?] 説明`

- **[P]**: 並列実行可能（異なるファイル、依存関係なし）
- 説明には正確なファイルパスを含める

## パス規約

このプロジェクトは**単一プロジェクト構造**（coordinator クレート）を使用：
- `coordinator/src/` - ソースコード
- `common/src/` - 共有プロトコル定義
- `tests/` - テストコード

---

## Phase 3.1: セットアップ

- [x] T001 [P] `common/src/protocol.rs` に `RequestResponseRecord` 構造体を定義
  - `RequestType`, `RecordStatus` enum も含む
  - serde でシリアライズ/デシリアライズ対応
  - 依存: なし

- [x] T002 [P] `Cargo.toml` に `csv = "1.3"` 依存関係を追加
  - CSV エクスポート用
  - 依存: なし

- [x] T003 [P] `coordinator/src/db/mod.rs` に `request_history` モジュールを宣言
  - `pub mod request_history;` を追加
  - 依存: なし

---

## Phase 3.2: テストファースト（TDD）⚠️ 3.3の前に完了必須

**重要: これらのテストは記述され、実装前に失敗する必要がある（RED）**

### Contract Tests（並列実行可能）

- [x] T004 [P] `tests/contract/request_history_api_test.rs` に
  List API (`GET /api/dashboard/request-responses`) の contract test を作成
  - 空の履歴でも 200 OK を返すことをテスト
  - レスポンス構造（records, total_count, page, per_page）を検証
  - 依存: T001 (RequestResponseRecord 定義)

- [x] T005 [P] `tests/contract/request_history_api_test.rs` に
  Detail API (`GET /api/dashboard/request-responses/:id`) の contract test を作成
  - 存在しないIDで 404 を返すことをテスト
  - 存在するIDで RequestResponseRecord を返すことをテスト
  - 依存: T001

- [x] T006 [P] `tests/contract/request_history_api_test.rs` に
  Export API (`GET /api/dashboard/request-responses/export`) の contract test を作成
  - format=json で JSON 形式を返すことをテスト
  - format=csv で CSV 形式を返すことをテスト
  - Content-Disposition ヘッダーを検証
  - 依存: T001

### Integration Tests（ストレージ層）

- [x] T007 [P] `tests/integration/request_storage_test.rs` に
  ストレージ層の保存機能の integration test を作成
  - `save_record()` 関数をテスト
  - レコードがファイルに保存されることを確認
  - JSON 形式の検証
  - 依存: T001

- [x] T008 [P] `tests/integration/request_storage_test.rs` に
  ストレージ層の読み込み機能の integration test を作成
  - `load_records()` 関数をテスト
  - 保存されたレコードを正しく読み込めることを確認
  - 依存: T001

- [x] T009 [P] `tests/integration/request_storage_test.rs` に
  ストレージ層のクリーンアップ機能の integration test を作成
  - `cleanup_old_records()` 関数をテスト
  - 7日より古いレコードが削除されることを確認
  - 新しいレコードは残ることを確認
  - 依存: T001

- [x] T010 [P] `tests/integration/request_storage_test.rs` に
  ストレージ層のフィルタリング機能の integration test を作成
  - モデル名、エージェントID、ステータス、日時範囲でフィルタ
  - ページネーションの動作を確認
  - 依存: T001

### Integration Tests（プロキシ層）

- [x] T011 `tests/integration/request_capture_test.rs` に
  プロキシキャプチャ機能の integration test を作成
  - `/api/chat` へのリクエストがキャプチャされることをテスト
  - request_history.json にレコードが保存されることを確認
  - レスポンスが正しくクライアントに返されることを確認
  - 依存: T001, T007 (保存機能のテストが先)

- [x] T012 `tests/integration/request_capture_test.rs` に
  エラーリクエストのキャプチャ integration test を作成
  - エラーが発生したリクエストもエラー情報付きで保存されることを確認
  - 依存: T001, T011

- [x] T013 `tests/integration/request_capture_test.rs` に
  ストリーミングレスポンスのキャプチャ integration test を作成
  - ストリーミングモードでもレスポンス全体が保存されることを確認
  - 依存: T001, T011

---

## Phase 3.3: コア実装（テストが失敗した後のみ）

### ストレージ層実装

- [x] T014 `coordinator/src/db/request_history.rs` にストレージ構造体を作成
  - `RequestHistoryStorage` 構造体
  - `Arc<Mutex<()>>` でファイルロック
  - `new()` コンストラクタ
  - 依存: T001, T003

- [x] T015 `coordinator/src/db/request_history.rs` に `save_record()` 関数を実装
  - レコードを JSON 配列に追加して保存
  - ファイルロック使用
  - 一時ファイル + rename パターン（破損防止）
  - 依存: T014, T007 (テストが先)

- [x] T016 `coordinator/src/db/request_history.rs` に `load_records()` 関数を実装
  - JSON ファイルから全レコードを読み込み
  - ファイルが存在しない場合は空配列を返す
  - 依存: T014, T008 (テストが先)

- [x] T017 `coordinator/src/db/request_history.rs` に
  `cleanup_old_records()` 関数を実装
  - 7日より古いレコードを削除
  - 新しいレコードのみを残して保存
  - 依存: T014, T015, T016, T009 (テストが先)

- [x] T018 `coordinator/src/db/request_history.rs` に
  `filter_and_paginate()` 関数を実装
  - フィルタ条件（モデル名、エージェントID、ステータス、日時範囲）
  - ページネーション（page, per_page）
  - 依存: T014, T016, T010 (テストが先)

- [x] T019 `coordinator/src/db/request_history.rs` に
  定期クリーンアップタスクを実装
  - `tokio::spawn` + `tokio::time::interval` (1時間ごと)
  - サーバー起動時に1回実行
  - Graceful shutdown 対応（CancellationToken）
  - 依存: T017

### プロキシ層修正

- [x] T020 `coordinator/src/api/proxy.rs` の `proxy_chat()` 関数を修正
  - レスポンスをバッファリング（`hyper::body::to_bytes`）
  - RequestResponseRecord を作成
  - `tokio::spawn` で非同期保存（fire-and-forget）
  - クライアントにレスポンス返却
  - 依存: T001, T015, T011 (テストが先)

- [x] T021 `coordinator/src/api/proxy.rs` の `proxy_generate()` 関数を修正
  - T020 と同じキャプチャロジックを実装
  - 依存: T020, T011

- [x] T022 `coordinator/src/api/proxy.rs` に
  ストリーミングレスポンスのキャプチャ機能を実装
  - `forward_streaming_response()` 関数を修正
  - ストリーム完了後にレスポンス全体を保存
  - 依存: T020, T021, T013 (テストが先)

### ダッシュボードAPI実装

- [x] T023 `coordinator/src/api/dashboard.rs` に
  List エンドポイント (`GET /api/dashboard/request-responses`) を実装
  - クエリパラメータのパース（model, agent_id, status, start_time, end_time,
    page, per_page）
  - `filter_and_paginate()` 呼び出し
  - レスポンス構造（records, total_count, page, per_page）
  - 依存: T016, T018, T004 (テストが先)

- [x] T024 `coordinator/src/api/dashboard.rs` に
  Detail エンドポイント (`GET /api/dashboard/request-responses/:id`) を実装
  - パスパラメータから UUID を取得
  - レコードを検索して返却
  - 見つからない場合は 404
  - 依存: T016, T005 (テストが先)

- [x] T025 `coordinator/src/api/dashboard.rs` に
  Export エンドポイント (`GET /api/dashboard/request-responses/export`) を実装
  - format パラメータ（json または csv）
  - JSON 形式: フィルタ済みレコードを JSON 配列で返却
  - CSV 形式: `csv` クレート使用、レコードを CSV に変換
  - Content-Disposition ヘッダー設定（attachment）
  - 依存: T002, T016, T018, T006 (テストが先)

- [x] T026 `coordinator/src/api/mod.rs` または `coordinator/src/main.rs` に
  新しいエンドポイントをルーターに登録
  - `/api/dashboard/request-responses` → list handler
  - `/api/dashboard/request-responses/:id` → detail handler
  - `/api/dashboard/request-responses/export` → export handler
  - 依存: T023, T024, T025

---

## Phase 3.4: UI実装

- [x] T027 [P] `coordinator/src/web/static/index.html` に
  「リクエスト履歴」タブを追加
  - 新しいタブボタン（History）
  - 履歴コンテンツセクション（初期状態は非表示）
  - テーブル要素（ID: `history-table`）
  - フィルタフォーム（モデル名、エージェント、ステータス、日時範囲）
  - エクスポートボタン
  - 詳細モーダル要素
  - 依存: なし

- [x] T028 `coordinator/src/web/static/app.js` に
  履歴リスト表示機能を実装
  - `fetchRequestHistory()` 関数（API呼び出し）
  - テーブルにレコードを描画
  - 5秒ごとの自動更新
  - 依存: T023, T027

- [x] T029 `coordinator/src/web/static/app.js` に
  詳細モーダル表示機能を実装
  - レコードクリック時のイベントハンドラ
  - 詳細API呼び出し
  - モーダルにリクエスト/レスポンス本文を表示
  - JSON シンタックスハイライト（`<pre><code>` 使用）
  - モーダルを閉じる機能
  - 依存: T024, T027, T028

- [x] T030 `coordinator/src/web/static/app.js` に
  フィルタ機能を実装
  - フィルタフォームの submit イベントハンドラ
  - クエリパラメータ構築
  - フィルタ済み履歴を取得して表示
  - フィルタクリア機能
  - 依存: T023, T028

- [x] T031 `coordinator/src/web/static/app.js` に
  エクスポート機能を実装
  - JSON エクスポートボタンのクリックハンドラ
  - CSV エクスポートボタンのクリックハンドラ
  - ファイルダウンロード処理
  - 依存: T025, T027

- [x] T032 [P] `coordinator/src/web/static/styles.css` に
  履歴UI のスタイルを追加
  - 履歴テーブルのスタイル
  - 詳細モーダルのスタイル
  - フィルタフォームのスタイル
  - レスポンシブ対応
  - ステータス色分け（成功=緑、エラー=赤）
  - 依存: なし

---

## Phase 3.5: E2E テスト

- [ ] T033 `tests/e2e/request_history_flow_test.rs` に
  エンドツーエンドフローの E2E test を作成
  - コーディネーター起動
  - テストエージェント登録
  - リクエスト送信（Chat + Generate）
  - ダッシュボード API で履歴取得
  - レコード内容の検証
  - フィルタリングのテスト
  - エクスポートのテスト
  - 依存: T023, T024, T025

---

## Phase 3.6: 仕上げ

- [ ] T034 [P] `README.md` にリクエスト履歴機能の説明を追加
  - 機能概要の記述
  - ダッシュボードの使い方
  - API エンドポイントの説明
  - 依存: なし

- [ ] T035 [P] `tests/unit/` にユニットテストを追加（必要に応じて）
  - フィルタロジックのユニットテスト
  - バリデーションのユニットテスト
  - 依存: T018

- [x] T036 すべてのテストを実行して合格確認
  - `cargo test` 実行
  - 全テスト合格を確認
  - 依存: すべての実装タスク

- [x] T037 ローカル検証を実行
  - `cargo fmt --check` 実行
  - `cargo clippy -- -D warnings` 実行
  - `make quality-checks` 実行（または個別チェック）
  - すべて合格を確認
  - 依存: T036

- [ ] T038 `quickstart.md` の手順を実際に実行して検証
  - 5分で試す手順を実行
  - すべてのユーザーストーリー検証を実行
  - 問題があれば修正
  - 依存: T036, T037

- [ ] T039 コミット＆プッシュ
  - コミットメッセージ: `feat(coordinator): リクエスト/レスポンス履歴保存機能を実装`
  - commitlint 準拠を確認
  - 依存: T037, T038

---

## 依存関係グラフ

```
Setup (T001-T003) → すべて並列実行可能
  ↓
Contract Tests (T004-T006) → すべて並列実行可能
  ↓
Integration Tests (T007-T013) → T007-T010 は並列、T011-T013 は順次
  ↓
Core実装 (T014-T026)
  ├─ ストレージ層 (T014-T019) → 順次実装
  ├─ プロキシ層 (T020-T022) → 順次実装
  └─ ダッシュボードAPI (T023-T026) → T023-T025 は並列、T026 は最後
  ↓
UI実装 (T027-T032) → T027, T032 は並列、T028-T031 は順次
  ↓
E2E Test (T033)
  ↓
Polish (T034-T039) → T034, T035 は並列、T036-T039 は順次
```

---

## 並列実行例

### Setup フェーズ

```
# T001-T003 を並列実行
Task T001: "common/src/protocol.rs に RequestResponseRecord 構造体を定義"
Task T002: "Cargo.toml に csv 依存関係を追加"
Task T003: "coordinator/src/db/mod.rs に request_history モジュールを宣言"
```

### Contract Test フェーズ

```
# T004-T006 を並列実行（すべて同じファイルだが、異なるテスト関数）
Task T004: "List API の contract test を作成"
Task T005: "Detail API の contract test を作成"
Task T006: "Export API の contract test を作成"
```

### Integration Test（ストレージ）フェーズ

```
# T007-T010 を並列実行（すべて同じファイルだが、異なるテスト関数）
Task T007: "保存機能の integration test を作成"
Task T008: "読み込み機能の integration test を作成"
Task T009: "クリーンアップ機能の integration test を作成"
Task T010: "フィルタリング機能の integration test を作成"
```

### ダッシュボードAPI実装フェーズ

```
# T023-T025 を並列実行（異なるエンドポイント）
Task T023: "List エンドポイントを実装"
Task T024: "Detail エンドポイントを実装"
Task T025: "Export エンドポイントを実装"
```

### UI実装フェーズ

```
# T027, T032 を並列実行（異なるファイル）
Task T027: "index.html に履歴タブを追加"
Task T032: "styles.css に履歴UI のスタイルを追加"
```

---

## 注意事項

- **[P] タスク** = 異なるファイル、依存関係なし
- **実装前にテストが失敗することを確認**（TDD の RED フェーズ）
- **各タスク後にコミット**（細かいコミットで履歴を明確に）
- **回避**: 曖昧なタスク、同じファイルの競合

---

## タスク完全性検証チェックリスト

- [x] すべての contracts に対応するテストがある（T004-T006）
- [x] すべての entities に model タスクがある（T001: RequestResponseRecord）
- [x] すべてのテストが実装より先にある（Phase 3.2 → Phase 3.3）
- [x] 並列タスクは本当に独立している（ファイル単位で確認済み）
- [x] 各タスクは正確なファイルパスを指定している
- [x] 同じファイルを変更する [P] タスクがない

---

## 実装準備完了

✅ すべてのタスクが定義され、実装の準備が整いました。

**次のステップ**: T001 から順番にタスクを実行し、TDD サイクル（RED → GREEN → REFACTOR）
を厳守してください。
