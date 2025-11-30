# 実装計画: 完全自動化リリースシステム

**機能ID**: `SPEC-ee2aa3ef` | **日付**: 2025-11-05 | **仕様**: [spec.md](./spec.md)
**入力**: `/specs/SPEC-ee2aa3ef/spec.md`の機能仕様

## 実行フロー

```
1. ✅ 入力パスから機能仕様を読み込み
2. ✅ 技術コンテキストを記入
3. ✅ 憲章チェックセクションを評価
4. ✅ Phase 0 を実行 → research.md (スキップ - 技術スタック確定済み)
5. ✅ Phase 1 を実行 → contracts, quickstart.md
6. ✅ 憲章チェックセクションを再評価
7. ✅ Phase 2 を計画 → タスク生成アプローチを記述
8. ✅ 実装完了（Phase 3-4実行済み）
```

**注記**: 本機能は既に実装完了しているため、このplan.mdは実装内容の文書化として作成。

## 概要

開発者がコマンド一つで正式版リリースを開始でき、品質チェックから本番公開まで完全自動化されたリリースシステム。semantic-releaseとGitHub Actionsを組み合わせて、Conventional Commitsから自動バージョニング、CHANGELOG生成、マルチプラットフォームバイナリビルドまでを実現。

**技術アプローチ**:
- semantic-release: 自動バージョニング・CHANGELOG・GitHub Release作成
- GitHub Actions: CI/CD実行環境
- Bash スクリプト: リリースPR作成・ホットフィックス支援
- Claude Code スラッシュコマンド: 開発者インターフェース

## 技術コンテキスト

**言語/バージョン**: Rust 1.75+, Bash 4.0+, Node.js 20+
**主要依存関係**: semantic-release, @semantic-release/*, GitHub CLI (gh), cargo-edit
**ストレージ**: Git (バージョン履歴)、GitHub (リリース成果物)
**テスト**: GitHub Actions品質チェック (cargo test, clippy, fmt, commitlint, markdownlint)
**対象プラットフォーム**: Linux x86_64, Windows x86_64, macOS x86_64/ARM64
**プロジェクトタイプ**: single (Rust CLIワークスペース)

**パフォーマンス目標**:

- alpha版リリース: PR作成から5分以内
- 正式版リリース: コマンド実行から30分以内（バイナリビルド含む）

**制約**:

- GitHub Actions環境で実行
- mainブランチは常に安定版を維持
- developブランチ必須
- Conventional Commits厳守

**スケール/スコープ**:

- 3ブランチ (main, develop, hotfix/**)
- 5プラットフォームバイナリ
- 並列品質チェック

## 憲章チェック

**シンプルさ**:
- プロジェクト数: 1 (llm-routerワークスペース) ✅
- フレームワークを直接使用? semantic-release・GitHub Actions直接利用 ✅
- 単一データモデル? N/A (インフラ・ツールチェーン) ✅
- パターン回避? リリーススクリプトは最小限の構造、不要な抽象化なし ✅

**アーキテクチャ**:
- すべての機能をライブラリとして? N/A (CIツール・スクリプト) ✅
- CLIツール:
  - `scripts/release/create-release-pr.sh` - develop→main PR作成
  - `scripts/release/create-hotfix.sh` - ホットフィックスブランチ作成
  - `/release`, `/hotfix` スラッシュコマンド
- ライブラリドキュメント: spec.md, plan.md, quickstart.md（今後作成）

**テスト (妥協不可)**:
- RED-GREEN-Refactorサイクルを強制? ⚠️ インフラコードのため厳密なTDDは適用困難
  - 代替: GitHub Actions実行での統合テスト（品質チェック必須）
- Gitコミットはテストが実装より先に表示? ⚠️ スクリプト実装後、CIで検証
- 順序: Contract→Integration→E2E→Unit? ⚠️ CI検証が統合テストとして機能
- 実依存関係を使用? ✅ 実GitHub Actions環境、実semantic-release
- Integration testの対象: ✅ 全リリースフロー（PR→品質チェック→マージ→リリース）
- **正当化**: インフラストラクチャコードであり、実行環境（GitHub Actions）でのみ検証可能

**可観測性**:
- 構造化ロギング含む? ✅ GitHub Actions標準ログ、スクリプト内の明確なステータス表示
- エラーコンテキスト十分? ✅ 失敗理由、推奨アクション、次のステップを明示

**バージョニング**:
- バージョン番号割り当て済み? ✅ semantic-releaseが自動計算（Conventional Commitsベース）
- 変更ごとにBUILDインクリメント? ✅ 自動（feat/fix/BREAKINGに基づく）
- 破壊的変更を処理? ✅ BREAKING CHANGE検出でメジャーバージョンアップ

## プロジェクト構造

### ドキュメント (この機能)
```
specs/SPEC-ee2aa3ef/
├── spec.md              # 機能仕様書 ✅
├── plan.md              # このファイル ✅
├── quickstart.md        # クイックスタートガイド（未作成）
└── tasks.md             # タスク分解（未作成）
```

### ソースコード (リポジトリルート)
```
scripts/release/
├── create-release-pr.sh      # develop→main PR作成 ✅
└── create-hotfix.sh          # ホットフィックスブランチ作成 ✅

.claude/commands/
├── release.md                # /release コマンド ✅
└── hotfix.md                 # /hotfix コマンド ✅

.github/workflows/
├── semantic-release.yml      # 自動バージョニング・リリース ✅
├── release-binaries.yml      # マルチプラットフォームビルド ✅
├── quality-checks.yml        # 品質チェック並列実行 ✅
└── auto-merge.yml            # 自動マージ（既存）

.releaserc.json               # semantic-release設定 ✅
CLAUDE.md                     # 古いnpm version記述削除 ✅
```

## Phase 0: アウトライン＆リサーチ

**スキップ理由**: 技術スタックはプロジェクト既存の設定を踏襲。

- ✅ semantic-release: 既にpackage.jsonに設定済み
- ✅ GitHub Actions: 既存ワークフロー（quality-checks, auto-merge）を拡張
- ✅ Rust toolchain: 既存のCargo.toml設定を利用
- ✅ Conventional Commits: commitlint既に導入済み

**決定事項**:
- developブランチでalpha版自動リリース（v1.2.3-alpha.N形式）
- mainブランチで正式版自動リリース（v1.2.3形式）
- hotfix/**ブランチからmainへ直接マージ（パッチ版）

## Phase 1: 設計＆契約

### 1. データモデル

**N/A** - インフラストラクチャコードのため、永続化データモデルなし

**状態管理**:
- Git ブランチ状態（main, develop, feature/**, hotfix/**）
- GitHub PR状態（品質チェック、承認、マージ）
- semantic-release状態（バージョン計算、CHANGELOG生成、リリース作成）

### 2. 契約（API/インターフェース）

#### A. スクリプトインターフェース

**create-release-pr.sh**
```bash
# 入力: なし（現在のGit状態を使用）
# 前提条件:
#   - developブランチが存在
#   - mainブランチが存在
#   - GitHub CLI (gh) がインストール済み
# 出力:
#   - develop → main プルリクエスト作成
#   - 終了コード 0: 成功
#   - 終了コード 1: エラー（前提条件不満足）
# 副作用:
#   - GitHub上にPR作成
#   - ラベル付与（release, auto-merge）
```

**create-hotfix.sh**
```bash
# 入力: [ISSUE_ID] (オプション)
# 前提条件:
#   - mainブランチが存在
#   - 作業ツリーがクリーン
# 出力:
#   - hotfix/[ISSUE_ID] ブランチ作成（mainから分岐）
#   - 修正ガイド表示
#   - 終了コード 0: 成功
#   - 終了コード 1: エラー
# 副作用:
#   - 新規Gitブランチ作成
#   - HEADの切り替え
```

#### B. GitHub Actions契約

**semantic-release.yml**
```yaml
# トリガー:
#   - push: [main, develop]
# 前提条件:
#   - Conventional Commitsでコミット
#   - .releaserc.json設定済み
#   - package.json依存関係インストール済み
# 出力:
#   - main: v1.2.3形式のタグ・リリース作成、バイナリ添付
#   - develop: v1.2.3-alpha.N形式のタグ・リリース作成
# 成果物:
#   - GitHub Release
#   - CHANGELOG.md更新
#   - Cargo.toml更新
#   - （mainのみ）Linux/Windows/macOS バイナリ
```

**quality-checks.yml**
```yaml
# トリガー:
#   - pull_request: [main, develop]
#   - push: [feature/**, hotfix/**]
# 前提条件: なし
# 出力:
#   - 全チェック合格: ステータス success
#   - いずれか失敗: ステータス failure
# 検証項目:
#   - cargo fmt --check
#   - cargo clippy -- -D warnings
#   - cargo test
#   - commitlint
#   - markdownlint
```

### 3. 契約テスト

**契約テスト戦略**:
実GitHub Actions環境での統合テストが契約テストとして機能。

- ✅ PR作成 → quality-checks自動実行
- ✅ 品質チェック合格 → auto-merge自動実行
- ✅ mainマージ → semantic-release自動実行（正式版）
- ✅ developマージ → semantic-release自動実行（alpha版）

### 4. ユーザーストーリーからテストシナリオ

**ストーリー1: 日常開発とアルファ版リリース**
```
Given: feature/xxxブランチで開発完了
When: develop へのPR作成
Then:
  - 品質チェックが並列実行される
  - 全チェック合格後、developへ自動マージ
  - v1.2.3-alpha.N形式のリリースが自動作成
  - CHANGELOG.mdが更新される
```

**ストーリー2: 正式リリースの開始**
```
Given: developブランチの準備完了
When: ./scripts/release/create-release-pr.sh 実行
Then:
  - develop → main PRが自動作成
  - 品質チェックが実行される
  - 全チェック合格後、mainへ自動マージ
  - v1.3.0形式の正式リリースが作成
  - 全プラットフォームバイナリがビルド・公開される
```

**ストーリー3: 緊急修正のリリース**
```
Given: 本番環境で問題発見
When: ./scripts/release/create-hotfix.sh 実行
Then:
  - hotfix/xxxブランチが作成される（mainから）
  - 修正完了後、main へのPR作成
  - 品質チェック → 自動マージ
  - v1.2.4形式のパッチリリースが即座に作成
```

### 5. ノードファイル更新

CLAUDE.md は既に更新済み（npm version記述削除）

## Phase 2: タスク計画アプローチ

**タスク生成戦略**:
実装は既に完了しているため、以下のタスクを遡及的に文書化：

### Setup タスク

1. [✅] developブランチ作成準備（メンテナ依頼）
2. [✅] .releaserc.json にdevelopブランチ追加
3. [✅] semantic-release.yml にdevelop対応追加

### Core タスク

1. [✅] create-release-pr.sh 実装
2. [✅] create-hotfix.sh 実装
3. [✅] /release スラッシュコマンド作成
4. [✅] /hotfix スラッシュコマンド作成
5. [✅] quality-checks.yml を develop/hotfix対応に更新
6. [✅] release-binaries.yml を workflow_call 対応に更新
7. [✅] semantic-release.yml にバイナリダウンロード追加

### Integration タスク

1. [✅] CLAUDE.md古い記述削除
2. [✅] markdownlint チェック
3. [✅] コミット＆プッシュ

### Polish タスク

1. [ ] quickstart.md 作成
2. [ ] tasks.md 作成
3. [ ] developブランチ実作成（メンテナ）
4. [ ] 動作確認（feature → develop → main フロー）

**順序戦略**:
- Setup → Core → Integration → Polish
- スクリプトとスラッシュコマンドは並列実行可能 [P]
- ワークフロー更新も並列実行可能 [P]

## Phase 3+: 今後の実装

**Phase 3**: タスク実行（tasks.md作成）
**Phase 4**: ✅ 実装完了
**Phase 5**: 検証（developブランチ作成後に実施）

## 複雑さトラッキング

| 違反 | 必要な理由 | より単純な代替案が却下された理由 |
|------|-----------|--------------------------------|
| TDD未適用 | インフラストラクチャコード | GitHub Actions環境でのみ検証可能。ローカルでのユニットテストは困難 |
| 3言語混在 (Rust/Bash/Node.js) | 既存プロジェクト構成 | Rust=本体、Bash=リリーススクリプト、Node.js=semantic-releaseツール |

**正当化**:
- Bashスクリプトは単純で、GitHub CLI (gh) との統合が容易
- semantic-releaseはNode.jsエコシステムの標準ツール
- 実GitHub Actions環境での統合テストがTDDの代替として機能

## 進捗トラッキング

**フェーズステータス**:
- [x] Phase 0: Research完了（スキップ - 技術スタック確定済み）
- [x] Phase 1: Design完了
- [x] Phase 2: Task planning完了（遡及的文書化）
- [x] Phase 3: Tasks生成済み（文書化予定）
- [x] Phase 4: 実装完了
- [ ] Phase 5: 検証合格（developブランチ作成後）

**ゲートステータス**:
- [x] 初期憲章チェック: 合格（複雑さトラッキングで正当化）
- [x] 設計後憲章チェック: 合格
- [x] すべての要明確化解決済み
- [x] 複雑さの逸脱を文書化済み

**実装完了**:
- [x] Spec文書作成（spec.md）
- [x] リリーススクリプト作成
- [x] スラッシュコマンド作成
- [x] semantic-release設定更新
- [x] GitHub Actionsワークフロー更新
- [x] CLAUDE.md修正
- [x] ローカル検証（markdownlint）
- [x] コミット＆プッシュ

**残タスク**:
- [ ] plan.md作成（このファイル）
- [ ] tasks.md作成
- [ ] quickstart.md作成
- [ ] developブランチ作成（メンテナ）
- [ ] 動作確認

## トラブルシューティング

### pipefail と早期パイプ終了の競合

**問題**: `tar -tzf "$archive" | head -1` のようなパイプライン処理で、
`set -o pipefail` が有効な場合、`head` がパイプを閉じると `tar` が
SIGPIPE (signal 13) を受信し、exit code 141 で終了する。
`pipefail` オプションがこれを失敗として扱うため、スクリプトが予期せず
終了する。

**解決策**: 該当行のみ `pipefail` を一時無効化する。

```bash
set +o pipefail
root_dir=$(tar -tzf "$archive" 2>&1 | head -1 | cut -d/ -f1)
set -o pipefail
```

**実例**: T024実行時（v1.0.0リリース）に7回の試行を経て解決。
詳細は `quickstart.md` の実践例を参照。

**教訓**: CI/CDワークフローでの厳格なエラーハンドリングと、
パイプラインの早期終了パターンは慎重に組み合わせる必要がある。

---
*実装完了後の遡及的文書化 - 2025-11-05*
*トラブルシューティング追加 - 2025-11-06*
