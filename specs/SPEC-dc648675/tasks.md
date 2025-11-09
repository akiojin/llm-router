# タスク: Worktree環境での作業境界強制システム

**入力**: `/specs/SPEC-dc648675/`の設計ドキュメント
**前提条件**: plan.md (完了), spec.md (完了)

## 実装状況

**現在の状態**:
- ✅ 実装完了: hookスクリプト2つ (.claude/hooks/)
- ✅ 設定完了: .claude/settings.json
- ✅ 手動テスト合格: 7つのテストケース
- ❌ 自動化テスト未実装
- ❌ CI/CD統合未実装

**TDD例外適用**: 憲章の「インフラストラクチャコード例外」に該当
- Claude Code PreToolUse Hook API環境依存
- 代替検証: Bats-core統合テスト + 手動検証 + CI/CD

## Phase 3.1: セットアップ

- [x] T001 [P] Bats-coreをインストールしてテスト環境をセットアップ
  - 実行: `npm install --save-dev bats` または システムパッケージマネージャーで bats をインストール
  - 確認: `bats --version` でバージョン表示
  - パス: プロジェクトルート

- [x] T002 [P] テストディレクトリ構造を作成
  - 作成: `tests/hooks/` ディレクトリ
  - 作成: `tests/hooks/.gitkeep` (ディレクトリ追跡用)
  - パス: `tests/hooks/`

## Phase 3.2: テストファースト (TDD) ⚠️ 3.3の前に完了必須

**重要: これらのテストは既存実装が合格することを検証する**

- [x] T003 [P] block-git-branch-ops.shの契約テストを作成
  - 作成: `tests/hooks/test-block-git-branch-ops.bats`
  - テストケース:
    - `git branch` (引数なし) → allow (exit 0)
    - `git branch --list` → allow (exit 0)
    - `git checkout main` → block (exit 2, JSON with "decision": "block")
    - `git switch develop` → block (exit 2)
    - `git worktree add /tmp/test` → block (exit 2)
    - `git branch -d feature` → block (exit 2)
    - `cargo test && git checkout main` → block (exit 2)
  - 実行: `bats tests/hooks/test-block-git-branch-ops.bats`
  - 期待: 既存実装が全テストに合格
  - パス: `tests/hooks/test-block-git-branch-ops.bats`

- [x] T004 [P] block-cd-command.shの契約テストを作成
  - 作成: `tests/hooks/test-block-cd-command.bats`
  - テストケース:
    - `cd .` → allow (exit 0)
    - `cd src` → allow (Worktree内、exit 0)
    - `cd /` → block (exit 2, JSON with "decision": "block")
    - `cd ~` → block (exit 2)
    - `cd /ollama-coordinator` → block (Worktree外、exit 2)
    - `cd ../..` → block (親ディレクトリ、exit 2)
  - 実行: `bats tests/hooks/test-block-cd-command.bats`
  - 期待: 既存実装が全テストに合格
  - パス: `tests/hooks/test-block-cd-command.bats`

## Phase 3.3: コア実装 (テスト合格確認後)

- [x] T005 既存hook実装のテスト合格確認と微調整
  - 実行: T003とT004のテストスイート
  - 確認: すべてのテストが合格
  - 必要に応じて: hook スクリプトの微調整（テストケースに合わせる）
  - コミット: テスト合格の証明
  - パス: `.claude/hooks/block-git-branch-ops.sh`, `.claude/hooks/block-cd-command.sh`

- [x] T006 [P] quickstart.mdを作成
  - 作成: `specs/SPEC-dc648675/quickstart.md`
  - 内容:
    - Hook設定の確認手順
    - 手動テスト実行例（git checkout、cd /のブロック確認）
    - 自動テストスイート実行手順
    - トラブルシューティング
  - パス: `specs/SPEC-dc648675/quickstart.md`

## Phase 3.4: 統合

- [x] T007 GitHub ActionsワークフローでHookテストを追加
  - 作成: `.github/workflows/test-hooks.yml`
  - 内容:
    - ubuntu-latestで実行
    - Bats-coreをインストール (apt-get install bats)
    - tests/hooks/*.batsを実行
    - 失敗時はPRをブロック
  - トリガー: push, pull_request
  - パス: `.github/workflows/test-hooks.yml`

- [x] T008 既存Quality ChecksワークフローにHookテストを統合
  - 編集: `.github/workflows/quality-checks.yml` (存在する場合)
  - 追加: Hookテストステップ
  - または: T007で作成した独立ワークフローを使用
  - 確認: PRでCI/CDが実行され、全テストが合格
  - パス: `.github/workflows/quality-checks.yml`

- [x] T009 Makefileにhookテストターゲットを追加
  - 編集: `Makefile`
  - 追加: `test-hooks` ターゲット
    ```makefile
    test-hooks:
        bats tests/hooks/test-block-git-branch-ops.bats
        bats tests/hooks/test-block-cd-command.bats
    ```
  - 追加: `quality-checks` ターゲットに `test-hooks` を含める
  - 確認: `make test-hooks` で全テスト実行
  - パス: `Makefile`

## Phase 3.5: 仕上げ

- [x] T010 [P] パフォーマンステストを実施
  - 作成: `tests/hooks/benchmark-hooks.sh`
  - 測定: hook実行時間（100回実行して平均）
  - 目標: < 100ms/実行
  - 記録: ベンチマーク結果を`specs/SPEC-dc648675/performance.md`に記録
  - パス: `tests/hooks/benchmark-hooks.sh`, `specs/SPEC-dc648675/performance.md`

- [x] T011 [P] README.mdにhook機能の説明を追加
  - 編集: `README.md`
  - 追加セクション:
    - "Claude Code Worktree Hooks" 概要
    - インストール・設定手順へのリンク
    - specs/SPEC-dc648675/へのリンク
  - パス: `README.md`

- [x] T012 [P] CLAUDE.mdのWorktree運用セクションを更新
  - 編集: `CLAUDE.md`
  - 更新: "Worktree＆ブランチ運用"セクション
  - 追加: hookスクリプトによる自動保護の説明
  - 参照: specs/SPEC-dc648675/へのリンク
  - パス: `CLAUDE.md`

- [x] T013 重複コードの削減とリファクタリング
  - レビュー: block-git-branch-ops.sh と block-cd-command.sh
  - 抽出: 共通関数（JSON出力、複合コマンド解析など）
  - オプション: 共通ライブラリ `.claude/hooks/common.sh` 作成
  - テスト: 全Batsテストが引き続き合格することを確認
  - パス: `.claude/hooks/block-git-branch-ops.sh`, `.claude/hooks/block-cd-command.sh`

- [x] T014 specs/SPEC-dc648675/のドキュメント最終化
  - レビュー: spec.md, plan.md, tasks.md, quickstart.md
  - 追加: performance.md（T010の結果）
  - 更新: チェックリスト（`specs/SPEC-dc648675/checklists/`）
  - 確認: markdownlint合格
  - パス: `specs/SPEC-dc648675/*.md`

- [x] T015 最終動作確認とドキュメント検証
  - 実行: quickstart.mdの全手順
  - 確認: 手動テスト・自動テスト全て合格
  - 実行: CI/CDパイプライン全体
  - 確認: GitHub Actionsで全チェック合格
  - 記録: 検証結果をspec.mdに追記
  - パス: プロジェクト全体

## 依存関係

- T001 (Bats-coreインストール) が T003, T004 をブロック
- T002 (ディレクトリ作成) が T003, T004 をブロック
- T003, T004 (テスト作成) が T005 (テスト合格確認) をブロック
- T005 (テスト合格確認) が Phase 3.4, 3.5 をブロック
- T007 (GitHub Actions) が T008 (統合) をブロック
- T010 (パフォーマンステスト) が T014 (ドキュメント最終化) をブロック
- T001-T014 が T015 (最終動作確認) をブロック

## 並列実行例

**Setup並列実行** (Phase 3.1):
```bash
# T001とT002は独立して実行可能
Task T001: "Bats-coreインストール"
Task T002: "テストディレクトリ構造作成"
```

**テスト並列作成** (Phase 3.2):
```bash
# T003とT004は異なるファイルのため並列実行可能
Task T003: "tests/hooks/test-block-git-branch-ops.bats作成"
Task T004: "tests/hooks/test-block-cd-command.bats作成"
```

**ドキュメント並列作成** (Phase 3.5):
```bash
# T010, T011, T012は異なるファイルのため並列実行可能
Task T010: "パフォーマンステスト実施"
Task T011: "README.md更新"
Task T012: "CLAUDE.md更新"
```

## 注意事項

- [P] タスク = 異なるファイル、依存関係なし
- 既存実装のテスト追加のため、テストが実装を検証する形式
- 各タスク後にコミット（Conventional Commits形式）
- hookスクリプトの変更時は必ず全Batsテストを再実行
- CI/CDパイプラインでのテスト失敗はPRマージをブロック

## タスク完全性検証

- [x] すべてのhookスクリプトに対応するテストがある (T003, T004)
- [x] 手動テストケース7つすべてが自動化されている
- [x] テストがCI/CD統合されている (T007, T008)
- [x] パフォーマンス要件 (<100ms) が検証されている (T010)
- [x] ドキュメント完備 (quickstart.md, performance.md, README.md, CLAUDE.md)
- [x] TDD例外の正当化が文書化されている (plan.md)

## 成功基準

- 全Batsテストが合格 (14テストケース)
- CI/CDパイプラインで全チェック合格
- Hook実行時間 < 100ms (平均)
- markdownlint エラー・警告ゼロ
- commitlint 全コミット合格
- ドキュメント完備・整合性確認
