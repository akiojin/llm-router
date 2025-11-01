# 実装計画: GPU必須エージェント登録要件

**機能ID**: `SPEC-5cd7b614` ｜ **作成日**: 2025-11-01  
**参照仕様**: [spec.md](./spec.md)

## 実行フロー (/speckit.plan スコープ)

```text
1. 仕様の確認 ✅
2. 技術コンテキストの整理 ✅
3. 憲章チェック（Phase 0前後で再確認）
4. Phase 0: リサーチアウトライン策定
5. Phase 1: アーキテクチャ・データモデル・契約定義
6. Phase 2: タスク分解方針定義（/speckit.tasksで実体化）
7. ここで停止 → 実装は別フェーズ
```

> **重要**: 環境固定ルールに従い、作業ディレクトリやブランチを変更せずに計画のみをまとめる。実装時も同一ブランチで進める想定。  

## 概要

Ollamaコーディネーターで GPU を搭載したエージェントのみを受け入れ、GPUなしエージェントを事前登録から除外する。既存データベースのクリーンアップとダッシュボード上での GPU 情報可視化を同時に達成する。

### コア要素

1. **登録バリデーション**: エージェント登録時に GPU 情報 (モデル・個数) 必須化。欠損時は 403 応答。
2. **起動時クリーンアップ**: 既存ストレージから GPU 情報のないレコードを自動削除。
3. **ダッシュボード表示**: エージェント一覧・詳細モーダルに GPU 情報を表示（運用者視点）。

## 技術コンテキスト

- **対象領域**: Rust（coordinator, agent, common クレート）、前段 API/ダッシュボード (JS)、永続化層 (SQLite or JSON ストレージ)。
- **既存機能**: 直近の `SPEC-47c6f44c` で自動マージ周りが更新済み。GPU 関連の導線は部分的に `feature/gpu-performance` で導入済み。
- **利用可能な構成要素**:
  - Agentサイド: GPU情報を収集する `sysinfo` / `nvml-wrapper`。
  - Coordinator: 登録 API (`POST /api/agents`)、ヘルス/ダッシュボード API、`registry` 管理ロジック。
  - Web UI: `coordinator/src/web/static/app.js`（GPU指標表示を既に追加済みのため連携確認のみ）。
- **制約**: Spec Kit カスタム運用によりブランチ・Worktree操作不可。CI は Quality Checks + Auto Merge を利用し、ローカルで同等検証を必須。

## 憲章チェック（事前）

| 観点 | 項目 | 対応方針 |
|------|------|-----------|
| シンプル | 新規サービス追加なし、既存クレート拡張 | ✅ |
| TDD | 登録/削除/表示のテストをRED→GREEN→リファクタ順で追加 | ✅ |
| LLM最適化 | ドキュメント＆テストベースでLLM支援しやすくする | ✅ |
| ハンドラー構造 | 既存APIハンドラーへバリデーション追加、テスト整備 | ✅ |

## 既存調査（着手予定）

1. **GPU情報収集の現状**  
   - `agent/src/metrics.rs` に GPU 指標取得ロジックが追加済み。フォーマット・送信箇所を確認。
2. **登録 API の改修余地**  
   - `coordinator/src/api/agent.rs` (`register_agent`) のバリデーションフロー把握。
   - `common/src/types.rs` の `AgentRegistration` 構造体に GPU フィールドがあるか確認。
3. **ストレージクリーンアップ**  
   - `coordinator/src/db` / `registry` の起動シーケンスから削除処理を差し込む箇所を調査。
4. **ダッシュボード反映**  
   - `app.js` で GPU 情報の描画箇所を確認し、API レスポンス形式と整合させる。

調査結果は Phase 0 の `research.md` からリンクする。

## アーキテクチャ概要（Phase 1想定）

### 登録時バリデーション

- Agent から送信される `POST /api/agents` のペイロードに `gpu_info`（例: `[{model, count}]`）を要求。
- Coordinator で `gpu_info` を検証。空、null、0枚は即 403 (エラーメッセージ: "GPU hardware is required").
- Agent 側では起動時に GPU DETECTION を行い、取得失敗時は登録リクエスト自体を中止する fallback を実装予定。

### 既存データクレンジング

- Coordinator 起動時 (`coordinator/src/main.rs` or `registry::init`) でストレージをスキャン。
- `gpu_info` が空 or 欠損のエントリを削除し、削除件数をログに出力。

### ダッシュボード & API

- `GET /api/agents` レスポンスへ GPU 情報フィールドを含める。
- フロントエンドで GPU 名／枚数表示。既にGPU列が前提のUIが存在するため整合性チェックのみ。

## テスト戦略

1. **Contract Tests**  
   - 登録API: GPU情報あり→201 / なし→403  
   - APIレスポンス: GPU情報が JSON に含まれること。
2. **Integration Tests**  
   - 起動時クリーンアップ: 擬似ストレージに GPU無しエージェントを残した状態で起動→削除を検証。  
   - Dashboard API: GPU情報付きレスポンスが返ること。
3. **Agent 側テスト**  
   - GPU情報収集ユニットテスト（NVMLが利用できない環境ではmockに切替）。

### 手動／E2E チェック

- GPU搭載マシン（CI or ローカル）で Agent→Coordinator 登録フローを確認。
- ダッシュボードで GPU 情報表示を目視確認（Spec Quickstart で手順化予定）。

## Phase 0: リサーチアウトライン

| トピック | 調査内容 | 成果物 |
|----------|----------|--------|
| GPU情報取得実装 | Agent側での GPU detection の既存処理、返却フォーマット | `research.md` |
| 403 応答仕様 | API エラー応答フォーマット (既存の validation error 対応) | `research.md` |
| ストレージ整合性 | 既存 DB (SQLite/JSON) の構造と削除手順 | `research.md` |

## Phase 1: 設計アウトライン

- `contracts/`  
  - `agents-register-gpu.contract.yml`: 登録 API の成功/失敗シナリオ。
  - `agents-cleanup.contract.yml`: 起動時の削除処理を擬似的に検証。
- `data-model.md`  
  - Agent レコードに `gpu_info` フィールドを追加した ER 図 / JSON スキーマ。
  - API 応答サンプル。
- `quickstart.md`  
  - GPU搭載/非搭載エージェントの登録確認手順。
  - ダッシュボードでの確認手順。

## Phase 2: タスク分割方針（/speckit.tasksで具現化）

1. **Setup**  
   - GPU情報収集ロジック再確認、必要に応じてmock層整備。  
   - 既存テスト用ストレージをクリーンアップ用に準備。
2. **RED: Contractテスト追加**  
   - 登録APIの成功/失敗テスト。  
   - 起動時削除テストの準備。
3. **GREEN: 実装**  
   - Agent登録 API にバリデーション追加。  
   - 起動時削除ロジック実装。  
   - Agentサイドで GPU 情報送出を必須化。
4. **REFAC**  
   - ログ／エラーメッセージ整備。  
   - ダッシュボード描画調整とテスト。
5. **Polish**  
   - ドキュメント更新（README, クイックスタート）。  
   - ローカル検証手順の追記。

## リスクと対応策

| リスク | 対応 |
|--------|------|
| GPU検出が環境依存で失敗 | Agent側で fallback ログを出して登録自体を抑止。テストでは mock を利用。 |
| 既存データ削除の安全性 | 削除前にバックアップ用ログ出力。テスト用 DB を活用して検証。 |
| UI 互換性 | APIレスポンス変更に合わせて `app.js` の描画ロジックを再確認しテスト。 |

## 次アクション

- `/speckit.tasks` を実行し、上記方針をタスクリスト化する。  
- GPU検出ロジックの現行コード調査（Phase 0）から着手。  
- Plan更新後は CLAUDE.md のルールどおりローカル検証を行いながら進める。

