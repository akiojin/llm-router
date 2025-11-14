# 実装計画: 自動マージ機能の実装

**機能ID**: `SPEC-47c6f44c` | **日付**: 2025-10-30 | **仕様**: [spec.md](./spec.md)
**入力**: `/ollama-coordinator/specs/SPEC-47c6f44c/spec.md`の機能仕様

## 実行フロー (/speckit.plan コマンドのスコープ)

```text
1. 入力パスから機能仕様を読み込み ✅
2. 技術コンテキストを記入 ✅
3. 憲章チェックセクションを評価 (進行中)
4. Phase 0 を実行 → research.md
5. Phase 1 を実行 → contracts, data-model.md, quickstart.md
6. 憲章チェックセクションを再評価
7. Phase 2 を計画 → タスク生成アプローチを記述
8. 停止 - /speckit.tasks コマンドの準備完了
```

- **重要**: /speckit.planコマンドはステップ7で停止します。Phase 2-4は他のコマンドで実行:
  - Phase 2: /speckit.tasksコマンドがtasks.mdを作成
  - Phase 3-4: 実装実行 (手動またはツール経由)

## 概要

GitHub Actionsを使用して、PR作成後の品質チェック（tests、lint、tasks完了、commitlint、markdownlint）を自動実行し、全チェック合格後に自動的にmainブランチにマージする機能を実装します。

**主要コンポーネント**:

1. **quality-checks.yml**: 5つの並列ジョブで品質チェック実行
2. **auto-merge.yml**: 品質チェック完了後、条件判定してマージ実行
3. **.commitlintrc.json**: Conventional Commits準拠の設定

**技術アプローチ** (research.mdで詳細化):

- GitHub Actions `workflow_run` トリガーで自動マージ起動
- GraphQL API (`gh api graphql`)でPRマージ実行
- 既存の`.specify/scripts/checks/`スクリプトを活用

## 技術コンテキスト

**言語/バージョン**: Bash 5.x, GitHub Actions YAML, Rust 1.75
**主要依存関係**: GitHub CLI (gh), GitHub Actions, commitlint, markdownlint-cli2
**ストレージ**: N/A (ステートレス)
**テスト**: GitHub Actions自体の実行でテスト (ダミーPRでの動作確認)
**対象プラットフォーム**: GitHub-hosted runners (ubuntu-latest, windows-latest)
**プロジェクトタイプ**: single (既存のRustプロジェクトに設定ファイル追加)
**パフォーマンス目標**: PR作成から品質チェック完了まで5-10分以内
**制約**: GitHub Actionsの実行時間制限6時間、workflow_run トリガーは GitHub Enterpriseの一部バージョンで非対応
**スケール/スコープ**: 2つのワークフローファイル、5つの品質チェックジョブ、1つの設定ファイル

## 憲章チェック

*ゲート: Phase 0 research前に合格必須。Phase 1 design後に再チェック。*

**シンプルさ**:

- プロジェクト数: 1 (既存のollama-coordinatorプロジェクトに追加) ✅
- フレームワークを直接使用? Yes (GitHub Actions、既存スクリプト) ✅
- 単一データモデル? Yes (ワークフローYAML定義のみ) ✅
- パターン回避? Yes (既存スクリプトを直接呼び出し、ラッパーなし) ✅

**アーキテクチャ**:

- すべての機能をライブラリとして? Yes (`.specify/scripts/checks/`が既存ライブラリ) ✅
- ライブラリリスト:
  - `check-tasks.sh`: tasks.md完了チェック
  - `check-tests.sh`: Rustテスト実行
  - `check-commits.sh`: commitlint実行
- ライブラリごとのCLI: 各スクリプトは`--help`対応済み ✅
- ライブラリドキュメント: CLAUDE.mdに記載済み ✅

**テスト (妥協不可)**:

- RED-GREEN-Refactorサイクルを強制? Yes ✅
  - RED: 失敗するテストワークフローを先に作成
  - GREEN: 実際のワークフローを実装
  - Refactor: ワークフローの最適化
- Gitコミットはテストが実装より先に表示? Yes ✅
  - コミット順序: `test(workflow): quality-checksテスト追加` → `feat(workflow): quality-checks実装`
- 順序: Contract→Integration→E2E→Unitを厳密に遵守? Yes ✅
  - Contract: ワークフローYAML構造定義
  - Integration: 実際のPRでの動作テスト
- E2E: メンテナがCIワークフローを起動 → PR生成 → 自動マージ完了までのフルフロー
- 実依存関係を使用? Yes (実際のGitHub Actions、実際のPR) ✅
- Integration testの対象: 新しいワークフロー、既存スクリプトとの統合 ✅
- 禁止: テスト前の実装、REDフェーズのスキップ ✅

**可観測性**:

- 構造化ロギング含む? Yes (GitHub Actions標準ログ) ✅
- フロントエンドログ → バックエンド? N/A
- エラーコンテキスト十分? Yes (失敗ジョブ、失敗理由、ログリンク) ✅

**バージョニング**:

- バージョン番号割り当て済み? N/A (ワークフロー設定のため、バージョニング不要)
- 変更ごとにBUILDインクリメント? N/A
- 破壊的変更を処理? Yes (既存ci.ymlとの統合、段階的移行) ✅

## プロジェクト構造

### ドキュメント (この機能)

```text
specs/SPEC-47c6f44c/
├── spec.md              # 機能仕様書 (完了)
├── plan.md              # このファイル (進行中)
├── research.md          # Phase 0 出力
├── data-model.md        # Phase 1 出力
├── quickstart.md        # Phase 1 出力
├── contracts/           # Phase 1 出力
│   ├── quality-checks.contract.yml
│   └── auto-merge.contract.yml
└── tasks.md             # Phase 2 出力 (/speckit.tasks)
```

### ソースコード (リポジトリルート)

```text
.github/workflows/
├── quality-checks.yml   # 新規作成: 品質チェック統合
├── auto-merge.yml       # 新規作成: 自動マージ実行
└── ci.yml               # 既存: 削除または統合

.commitlintrc.json       # 新規作成: commitlint設定

.specify/scripts/checks/ # 既存: 変更なし
├── check-tasks.sh
├── check-tests.sh
├── check-commits.sh
└── check-compile.sh

.specify/scripts/bash/
└── finish-feature.sh    # 既存: PRボディ更新（メンテナ／CI専用）
```

**構造決定**: 既存のRustプロジェクトに設定ファイル追加（オプション1: 単一プロジェクト）

## Phase 0: アウトライン＆リサーチ

### リサーチタスク

1. **参考リポジトリ @akiojin/claude-worktree の解析**:
   - 目的: 自動マージの実装パターンを学ぶ
   - 調査内容:
     - `workflow_run` トリガーの使用方法
     - GraphQL API マージの実装
     - PR状態チェック（isDraft、mergeable、mergeStateStatus）
     - permissions設定（contents: write、pull-requests: write）
   - 出力: research.md の「参考実装」セクション

2. **GitHub Actions workflow_run トリガーの仕様**:
   - 目的: workflow_run の制約と動作を理解
   - 調査内容:
     - workflow_run.conclusion のフィルタリング（success、failure）
     - workflow_run.event のフィルタリング（pull_request）
     - トリガー元ワークフロー名の指定方法
     - デフォルトブランチ以外での動作
   - 出力: research.md の「workflow_runトリガー」セクション

3. **GitHub GraphQL API マージ実装**:
   - 目的: PRマージのGraphQL API使用方法を学ぶ
   - 調査内容:
     - `mergePullRequest` mutation の構文
     - マージ方法（MERGE、SQUASH、REBASE）の指定
     - PR IDの取得方法（`gh pr list --head ブランチ名`）
     - エラーハンドリング（コンフリクト、権限エラー）
   - 出力: research.md の「GraphQL API」セクション

4. **commitlintの設定方法**:
   - 目的: Conventional Commits準拠の設定を作成
   - 調査内容:
     - `.commitlintrc.json` の標準構造
     - `@commitlint/config-conventional` の使用方法
     - 日本語コミットメッセージのサポート
     - カスタムルールの追加方法（subject-case、body-max-line-lengthなど）
   - 出力: research.md の「commitlint設定」セクション

5. **既存ci.ymlとの統合方法**:
   - 目的: 既存ワークフローとの重複を避ける
   - 調査内容:
     - 現在のci.ymlの内容（rust test、rust lint、coverage）
     - quality-checks.ymlへの統合方法
     - 段階的移行の戦略（ci.ymlを削除するタイミング）
   - 出力: research.md の「既存ワークフロー統合」セクション

### リサーチエージェント生成

```bash
# タスク1: 参考リポジトリ解析
Research "auto-merge implementation in @akiojin/claude-worktree" for "GitHub Actions自動マージ機能"

# タスク2: workflow_run仕様
Research "GitHub Actions workflow_run trigger limitations and best practices" for "自動マージトリガー"

# タスク3: GraphQL API
Research "GitHub GraphQL API mergePullRequest mutation examples" for "PRマージ実装"

# タスク4: commitlint設定
Research "commitlint configuration for Conventional Commits with Japanese support" for "コミットメッセージ検証"

# タスク5: 既存ワークフロー統合
Analyze "existing .github/workflows/ci.yml" for "quality-checks.ymlへの統合方法"
```

**出力**: すべての要明確化が解決されたresearch.md

## Phase 1: 設計＆契約

*前提条件: research.md完了*

### 1. データモデル (`data-model.md`)

**エンティティ: QualityChecksWorkflow**

- フィールド:
  - `name`: "Quality Checks" (ワークフロー名)
  - `trigger`: pull_request, push (イベント)
  - `jobs`: [tasks-check, rust-test, rust-lint, commitlint, markdownlint] (並列ジョブ)
  - `conclusion`: success | failure (全体結果)

**エンティティ: AutoMergeWorkflow**

- フィールド:
  - `name`: "Auto Merge" (ワークフロー名)
  - `trigger`: workflow_run (QualityChecksWorkflow完了時)
  - `conditions`: [conclusion==success, !isDraft, mergeable==MERGEABLE] (マージ条件)
  - `merge_method`: MERGE (マージ方法)

**エンティティ: CommitlintConfig**

- フィールド:
  - `extends`: ["@commitlint/config-conventional"] (ベース設定)
  - `rules`: {type-enum, subject-case, body-max-line-length} (カスタムルール)

### 2. API契約 (`contracts/`)

契約ファイルは`.github/workflows/`に配置する実際のワークフローYAMLの構造を定義します。

**出力**: data-model.md, contracts/, quickstart.md

## Phase 2: タスク計画アプローチ

*このセクションは/speckit.tasksコマンドが実行することを記述 - /speckit.plan中は実行しない*

### タスク生成戦略

**ベーステンプレート**: `/ollama-coordinator/.specify/templates/tasks-template.md`

**タスク生成ロジック**:

1. **Setupタスク** (並列実行可能):
   - `.commitlintrc.json`作成 [P]
   - 既存`ci.yml`のバックアップ [P]
   - `package.json`にcommitlint依存関係追加（必要に応じて） [P]

2. **Contract testタスク** (TDD - RED):
   - `test-quality-checks.yml`作成（各ジョブの動作をテスト） [P]
   - `test-auto-merge.yml`作成（マージ条件のテスト） [P]
   - テスト実行 → 失敗確認

3. **Core実装タスク** (TDD - GREEN):
   - `quality-checks.yml`作成（tasks-check、rust-test、rust-lint、commitlint、markdownlint）
   - `auto-merge.yml`作成（workflow_run、GraphQL API）
   - テスト実行 → 合格確認

4. **Integrationタスク**:
   - メンテナが提供する検証用ワークフローを「未完了タスク」モードで実行 → tasks-check失敗をCIログで確認
   - 規約違反コミットを含む検証PRをメンテナが生成 → commitlint失敗をCIログで確認
   - 全チェック合格PRをCI上で再現 → auto-merge起動と成功ログを確認

5. **Polishタスク**:
   - `finish-feature.sh`のPRボディ更新（メンテナ／CI実行前提で文面調整）
   - `CLAUDE.md`の自動マージセクション更新
   - `ci.yml`削除または統合
   - ドキュメント最終確認

### 順序戦略

**TDD順序**: テストワークフロー → 実装ワークフロー → 統合テスト
**依存関係順序**:

- `.commitlintrc.json` → commitlintジョブ
- `quality-checks.yml` → `auto-merge.yml`
- 実装完了 → メンテナ用 `finish-feature.sh` メッセージ更新

**並列実行マーク [P]**:

- Setup: 全タスク並列実行可能
- Contract test: 各テストワークフロー独立
- Core実装: quality-checks.ymlとauto-merge.ymlは依存関係あり（順次）

### 推定出力

tasks.mdに約20-25個の番号付き、順序付きタスク:

- Setup: 3タスク
- Contract tests: 2-3タスク
- Core実装: 2タスク
- Integration: 3-4タスク
- Polish: 4-5タスク

**重要**: このフェーズは/speckit.tasksコマンドで実行、/speckit.planではない

## Phase 3+: 今後の実装

*これらのフェーズは/planコマンドのスコープ外*

**Phase 3**: タスク実行 (/speckit.tasksコマンドがtasks.mdを作成)
**Phase 4**: 実装 (憲章原則に従ってtasks.mdを実行)
**Phase 5**: 検証 (テスト実行、quickstart.md実行、パフォーマンス検証)

## 複雑さトラッキング

| 違反 | 必要な理由 | より単純な代替案が却下された理由 |
|------|-----------|--------------------------------|
| なし | - | - |

**注**: 本実装は憲章原則に完全準拠しています。既存スクリプトを活用し、新規作成するのは設定ファイルのみです。

## 進捗トラッキング

**フェーズステータス**:

- [x] Phase 0: Research完了 (/speckit.plan コマンド) ✅ 2025-10-30
- [x] Phase 1: Design完了 (/speckit.plan コマンド) ✅ 2025-10-30
- [x] Phase 2: Task planning完了 (/speckit.plan コマンド - アプローチのみ記述) ✅ 2025-10-30
- [x] Phase 3: Tasks生成済み (/speckit.tasks コマンド) ✅ 2025-10-30
  - 18タスク生成 (Setup: 2, Tests: 3, Implementation: 3, Integration: 4, Polish: 6)
  - TDD順序厳守 (RED → GREEN → REFACTOR)
  - 並列実行可能タスク: 10タスク
- [ ] Phase 4: 実装完了
- [ ] Phase 5: 検証合格

**ゲートステータス**:

- [x] 初期憲章チェック: 合格 ✅
- [x] 設計後憲章チェック: 合格 ✅（Phase 1完了、憲章違反なし）
- [x] すべての要明確化解決済み ✅（Phase 0完了、research.md参照）
- [x] 複雑さの逸脱を文書化済み ✅（違反なし）

---

*憲章 v1.0.0 に基づく - `.specify/memory/constitution.md` 参照*
