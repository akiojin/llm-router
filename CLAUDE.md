# CLAUDE.md

このファイルは、このリポジトリでコードを扱う際のガイダンスを提供します。

## 開発指針

### 🛠️ 技術実装指針

- **設計・実装は複雑にせずに、シンプルさの極限を追求してください**
- **ただし、ユーザビリティと開発者体験の品質は決して妥協しない**
- 実装はシンプルに、開発者体験は最高品質に
- CLI操作の直感性と効率性を技術的複雑さより優先

### 📝 設計ガイドライン

- 設計に関するドキュメントには、ソースコードを書かないこと

## 開発品質

### 完了条件

- エラーが発生している状態で完了としないこと。必ずエラーが解消された時点で完了とする。

## 開発ワークフロー

### 基本ルール

- 作業（タスク）を完了したら、変更点を日本語でコミットログに追加して、コミット＆プッシュを必ず行う
- コミットメッセージは commitlint (Conventional Commits) に準拠した形式で記述する（例: `feat(core): add new api`）
- 作業（タスク）は、最大限の並列化をして進める
- 作業（タスク）は、最大限の細分化をしてToDoに登録する
- 作業（タスク）の開始前には、必ずToDoを登録した後に作業を開始する
- 作業（タスク）は、忖度なしで進める

### Spec駆動開発ライフサイクル

新機能の開発は、以下の3ステップで進めます：

1. **`/speckit.specify`**: 機能仕様書を作成 (`specs/SPEC-[UUID8桁]/spec.md`)
   - ビジネス要件とユーザーストーリーを定義
   - 「何を」「なぜ」に焦点を当てる（「どのように」は含めない）
   - SPECディレクトリ、featureブランチ、Worktreeを自動作成
   - 作業は`.worktrees/SPEC-xxx/`内で実施

2. **`/speckit.plan`**: 実装計画を作成 (`specs/SPEC-[UUID8桁]/plan.md`)
   - 技術スタック、アーキテクチャ、データモデルを設計
   - 憲章チェック（TDD/LLM最適化/シンプルさの原則）
   - Phase 0: 技術リサーチ (`research.md`)
   - Phase 1: 設計とコントラクト (`data-model.md`, `contracts/`, `quickstart.md`)
   - Phase 2: タスク計画 (`tasks.md`)

3. **`/speckit.tasks`**: 実行可能なタスクに分解 (`specs/SPEC-[UUID8桁]/tasks.md`)
   - Setup/Test/Core/Integration/Polishに分類
   - 並列実行可能なタスクに`[P]`マーク付与
   - 依存関係を明確化

#### Spec命名規則

- **形式**: `SPEC-[UUID8桁]`
- **UUID生成**: ランダムな英数字（小文字）8桁
  - ✅ 正しい例: `SPEC-a1b2c3d4`, `SPEC-3f8e9d2a`, `SPEC-7c4b1e5f`
  - ❌ 間違い例: `SPEC-001`, `SPEC-gameobj`, `SPEC-core-001`
- **禁止事項**:
  - 連番の使用（001, 002...）
  - 意味のある名前（gameobj, core, ui...）
  - 大文字の使用（UUID部分は小文字のみ）
- **生成方法**: `uuidgen | tr '[:upper:]' '[:lower:]' | cut -c1-8` またはオンラインUUID生成ツール

#### Worktree＆ブランチ運用（必須）

**すべてのSPEC開発はWorktreeで並行作業**

本プロジェクトは**Git Worktree**を使用した並行開発フローを採用しています。各SPECは独立したWorktreeで作業し、完了後にGitHub Pull Requestを作成して自動マージします（**GitHub Actions自動マージ**）。

**Worktree配置**:

- **場所**: `.worktrees/[ブランチ名]/`
- **例**: `.worktrees/feature-admin/`
- Git管理外（`.gitignore`登録済み）
- 各Worktreeは完全な作業ツリーを持つ

**新規SPEC作成フロー**:

1. `/speckit.specify` コマンド実行
   - SPECディレクトリ作成（`specs/SPEC-xxx/`）
   - Worktree作成（`.worktrees/[ブランチ名]/`）
   - 初期ファイル生成（`spec.md`）

2. Worktreeに移動して作業開始:
   ```bash
   cd .worktrees/[ブランチ名]/
   ```

3. 独立して開発作業を実行:
   - TDDサイクル厳守
   - 各変更をコミット
   - 他のSPECと完全に独立

**作業完了フロー**:

1. Worktree内で最終コミット完了を確認
2. finish-featureスクリプト実行:
   ```bash
   .specify/scripts/bash/finish-feature.sh
   # またはドラフトPRとして作成:
   .specify/scripts/bash/finish-feature.sh --draft
   ```

3. 自動実行される処理:
   - featureブランチをリモートにpush
   - GitHub PRを自動作成（spec.mdからタイトル取得）
   - GitHub Actions「**Quality Checks**」ワークフローで品質チェック実行（5つのジョブを並列実行）:
     1. **tasks-check**: tasks.mdの全タスク完了チェック（`.specify/scripts/checks/check-tasks.sh`）
     2. **rust-test**: Rustテスト実行（`cargo test`, ubuntu-latest + windows-latest）
     3. **rust-lint**: Rust lintチェック（`cargo fmt --check`, `cargo clippy`）
     4. **commitlint**: コミットメッセージ検証（Conventional Commits準拠、`.specify/scripts/checks/check-commits.sh`）
     5. **markdownlint**: マークダウンファイルlint（`npx markdownlint-cli`）
   - 全チェック成功時、GitHub Actions「**Auto Merge**」ワークフローが起動し、以下の条件を満たす場合に自動的にmainへマージ（MERGEメソッド、履歴保持）:
     - ✅ 全品質チェックが成功
     - ✅ PRがドラフトでない
     - ✅ マージ可能（コンフリクトなし）
     - ✅ マージ状態が正常（CLEAN または UNSTABLE）

**重要な注意事項**:

- **mainブランチで直接SPEC作業禁止**: 必ずWorktreeを使用
- **PR自動マージ**: GitHub Actionsで品質チェック後に自動マージ
- **ドラフトPR**: `--draft`オプションで作成したPRは自動マージ対象外。品質チェックは実行されるが、ドラフト解除するまで自動マージされない
- **並行開発推奨**: 複数のSPECを同時に異なるWorktreeで作業可能
- **Worktree間の独立性**: 各Worktreeは完全に独立（相互干渉なし）
- **コミット**: Worktree内でのコミットはfeatureブランチに記録される
- **GitHub CLI必須**: `gh auth login`で認証が必要

**スクリプト**:

- `.specify/scripts/bash/create-new-feature.sh`: 新規SPEC＆Worktree作成
- `.specify/scripts/bash/finish-feature.sh`: PR作成（自動マージトリガー）
- `.specify/scripts/checks/*.sh`: 品質チェックスクリプト（tasks/tests/compile/commits）

### TDD遵守（妥協不可）

**絶対遵守事項:**

- **Red-Green-Refactorサイクル必須**:
  1. **RED**: テストを書く → テスト失敗を確認
  2. **GREEN**: 最小限の実装でテスト合格
  3. **REFACTOR**: コードをクリーンアップ

- **禁止事項**:
  - テストなしでの実装
  - REDフェーズのスキップ（テストが失敗することを確認せずに実装）
  - 実装後のテスト作成（テストが実装より後のコミットになる）

- **Git commitの順序**:
  - テストコミットが実装コミットより先に記録される必要がある
  - 例: `feat(test): Fooのテスト追加` → `feat: Foo実装`

- **テストカテゴリと順序**:
  1. Contract tests (統合テスト) → API/インターフェース定義
  2. Integration tests → クリティカルパス100%
  3. E2E tests → 主要ユーザーワークフロー
  4. Unit tests → 個別機能、80%以上のカバレッジ

**詳細は [`memory/constitution.md`](memory/constitution.md) を参照**

### SDD (Spec-Driven Development) 規約

**すべての機能開発・要件追加は `/speckit.specify` から開始**

**新規機能開発フロー**:

1. `/speckit.specify` - ビジネス要件を定義（技術詳細なし）
   - 自動的にfeatureブランチ＆Worktree作成
   - `.worktrees/SPEC-xxx/`に移動して作業開始
2. `/speckit.plan` - 技術設計を作成（憲章チェック必須）
   - Worktree内で実行
3. `/speckit.tasks` - 実行可能タスクに分解
   - 実装実行は `/speckit.implement` で補助的に利用可能
   - Worktree内で実行
4. タスク実行（TDDサイクル厳守）
   - Worktree内で独立して作業
   - 各変更をfeatureブランチにコミット
5. 完了後、`finish-feature.sh`でmainにマージ＆Worktree削除

**既存機能のSpec化フロー**:

1. 対応するWorktreeに移動: `cd .worktrees/SPEC-xxx/`
2. `/speckit.specify` - 実装済み機能のビジネス要件を文書化
3. `/speckit.plan` - （必要に応じて）技術設計を追記
4. 既存実装とSpecの整合性確認
5. 完了後、`finish-feature.sh`でmainにマージ

**Spec作成原則**:

- ビジネス価値とユーザーストーリーに焦点
- 「何を」「なぜ」のみ記述（「どのように」は禁止）
- 非技術者が理解できる言葉で記述
- テスト可能で曖昧さのない要件

**憲章準拠**:

- すべての実装は [`memory/constitution.md`](memory/constitution.md) に準拠
- TDD、ハンドラーアーキテクチャ、LLM最適化は妥協不可

## コミュニケーションガイドライン

- 回答は必ず日本語

## ドキュメント管理

- ドキュメントはREADME.md/README.ja.mdに集約する

### Assets配下の扱い（サンプル/Unityプロジェクト側）

- 目的: サンプル・検証・デモ・手動動作確認用のシーン/プレハブ/補助スクリプトを配置。
- 禁止: unity-mcp-server 実装本体（ランタイム/エディタ拡張の主要コード）を置かない。UPM配布の対象にも含めない。
- 編集: 必要最小限に留める。C#編集は外部CLIで行い、Assets側は参照/設定のみで成立させる。

## バージョン管理

### npm versionコマンドの使用

バージョンアップは必ず`npm version`コマンドを使用する：

- **パッチバージョン**: `npm version patch` (例: 2.9.0 → 2.9.1)
- **マイナーバージョン**: `npm version minor` (例: 2.9.0 → 2.10.0)
- **メジャーバージョン**: `npm version major` (例: 2.9.0 → 3.0.0)

**重要**: package.jsonを直接編集してのバージョン変更は禁止

## コードクオリティガイドライン

- マークダウンファイルはmarkdownlintでエラー及び警告がない状態にする
- コミットログはcommitlintに対応する

## 開発ガイドライン

- 既存のファイルのメンテナンスを無視して、新規ファイルばかり作成するのは禁止。既存ファイルを改修することを優先する。

## ドキュメント作成ガイドライン

- README.mdには設計などは書いてはいけない。プロジェクトの説明やディレクトリ構成などの説明のみに徹底する。設計などは、適切なファイルへのリンクを書く。
