# マルチモデル自動起動 ExecPlan (2025-11-17)

このプランは、CLAUDE.md で要求されるフローと SPEC 群（特に SPEC-8ae67d67, SPEC-1f2a9c3d, SPEC-ee2aa3ef, SPEC-5cd7b614）に沿って、コーディネーター／エージェントが「対応モデルを全自動で起動・同期し、OpenAI互換APIは必ずエージェント経由」となる状態を完成させるための実行計画です。完了条件と検証方法を明示し、進捗を逐次更新します。

## 目的 (Purpose)
- 対応モデルセット（gpt-oss:20b / gpt-oss:120b / gpt-oss-safeguard:20b / qwen3-coder:30b）をコーディネーターが正として提示し、/v1/models でも同一内容を返す。
- エージェントは登録後ただちに全対応モデルを自動ダウンロードし、モデルごとに独立した Ollama プロセスを起動（ポート衝突なし、同一ホストで複数進行可）。
- コーディネーターはエージェント経由の OpenAI互換API だけを利用し、エージェントがすべての対応モデルを ready にするまでリクエストを待機（上限1024、超過は503）。
- 登録時ヘルスチェック: エージェントAPI /v1/models が応答し、少なくとも1モデルが起動済みでないと登録を拒否し「起動中」を示す。
- UI: 手動配布UIを残さず、ロード済みモデルは「全エージェント合算」表示のみ。エージェント表からモデル列を除去。

## 参照 (References)
- SPEC-8ae67d67: `specs/SPEC-8ae67d67/spec.md` (モデル自動配布・対応モデルセット、モデルリスト4件)
- SPEC-ee2aa3ef: `specs/SPEC-ee2aa3ef/tasks.md` (T025 ホットフィックスフロー/統合テスト)
- SPEC-5cd7b614: `specs/SPEC-5cd7b614/tasks.md` (PRマージ後の動作確認)
- SPEC-1f2a9c3d: `specs/SPEC-1f2a9c3d/` (ログAPI要件) — Agent `/api/logs` と Coordinator プロキシ `/api/agents/:id/logs`
- SPEC-712c20cf: `specs/SPEC-712c20cf/` (ダッシュボードUX/NFR) — モデル管理パネル削除後のUI整合

## 作業計画 (Plan of Work)
1. **対応モデルセット確定と仕様反映** ✅ 実装済み（4モデル固定で /api/models /v1/models / UI に反映）
   - common/coordinator で定数化済み。サイズ/推奨VRAMを更新。
2. **エージェント: マルチ Ollama オーケストレーター** ✅ 実装済み
   - モデルごとに `OllamaPool` で serve 起動・再利用。/v1/models はコーディネーター対応モデルを返す。
   - ready_models / initializing を報告。
3. **コーディネーター: 登録・待機制御の強化** ⏳ 部分完了
   - 登録時にエージェント `/v1/models` を取得し初期状態を同期済み。
   - 待機キュー(1024上限)は LoadManager で実装済み。503 文言/溢れケースのAPIレベル検証を追加する（SPEC-8ae67d67 FR-009 整合）。
   - リロード時に詳細モーダルが自動表示されないことをUI回帰テストで保証（SPEC-712c20cf）。
4. **テスト (TDD)** ⏳ 部分完了
   - unit: ready待機・ポート衝突なし → 済。  
   - integration: 全エージェント initializing→ready で復帰 → 済。  
   - integration: `/v1/completions` ハッピー経路 → 済。  
   - integration: 待機キュー溢れ→HTTP 503 を追加（本プランで実装）。  
   - UIスナップショット: タブ撤去＆モデル管理パネル削除を確認するメモ・テスト (`coordinator/tests/ui/model_panel_removed.rs`)。モーダル自動表示しない回帰を追加予定。
5. **ドキュメント/運用** ⏳ 進行中
   - README.ja.md に最新アーキ要点を追記済み。ポート表・起動手順は未更新。
   - SPEC-8ae67d67 はモデル一覧・手動配布廃止を更新済み。
6. **ログAPIの要件化とTDD** 🆕
   - 要件: エージェントの最新ログをHTTPで取得でき、コーディネーター経由でも同等に参照できること（tail件数指定可、デフォルト200行、JSONLテキスト返却）。
   - スコープ: Agent `/api/logs` (tail), Coordinator `/api/agents/:id/logs` プロキシ、DashboardのログパネルをこのAPIに接続。
   - TDD方針: 
     1) Agent単体: `/api/logs?tail=5` が末尾5行を返し、存在しないファイルでも空文字+200となることをテスト。
     2) Coordinator契約: エージェントが200を返すケース・タイムアウト/接続不可で502となるケースをカバー。
     3) UIはスモーク（fetchが成功し、テキストが表示される）で確認。

- [x] 2025-11-17T15:10Z  ready待機キューの挙動テストを追加し、clippy/test/markdownlint/タスクチェックを通過。
- [x] 2025-11-17 モデルセット統一（5モデル固定→後に4モデルへ移行）、UIプリセット更新。
- [x] 2025-11-17 エージェント側マルチモデル自動起動（OllamaPool）と /v1/models 同期を実装。
- [x] 2025-11-17 コーディネーター登録時にエージェント /v1/models を取り込み、初期状態を同期。
- [x] 2025-11-18 API統合テスト追加（待機キュー溢れ→HTTP503、env override付き）を完了。UI再チェックとREADME.ja.md ポート表整理は未。
- [x] 2025-11-18 対応モデルを実在リスト4件に修正（gpt-oss:20b/120b, gpt-oss-safeguard:20b, qwen3-coder:30b）し、UI・spec・テストを更新。
- [x] 2025-11-19 UI回帰: モデル管理パネル削除の自動テスト、リロード時にエージェント/リクエスト詳細モーダルが初期状態で閉じていることをテストで担保。
- [ ] README.ja.md ポート表追記と UI 文言最終確認。
- [ ] ログAPI実装: Agent `/api/logs`, Coordinator `/api/agents/:id/logs`, UI接続、テスト追加（上記TDD方針）。
- [ ] specs 未完了タスク (自動継承)
  - SPEC-ee2aa3ef: **T025 ホットフィックスフロー確認／統合テスト(T023–T025)実行**
  - SPEC-5cd7b614: **PRマージ後の動作確認（メンテナ作業）**
  - SPEC-8ae67d67: plan.md Phase0–5 のチェックボックス未完了（モデル自動配布フェーズの反映）
  - SPEC-712c20cf: quickstart/plan の UI/NFR チェック未完了（ダッシュボードUX）
  - SPEC-47c6f44c: plan Phase4–5 未完了（tasks-checkワークフロー完了条件）

## Surprises & Discoveries
- まだ無し。進行中に記載する。

## Decision Log
- 2025-11-17: キュー上限1024固定・タイムアウト無し・503で返す方針を維持。
- 2025-11-17: GPU自動選択ロジックは一旦無効化（feature フラグ付きテストに退避）し、対応モデルは固定リストで扱う。

## Outcomes & Retrospective
- 着手後に更新。
