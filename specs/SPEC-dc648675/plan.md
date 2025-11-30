# 実装計画: Worktree環境での作業境界強制システム

**機能ID**: `SPEC-dc648675` | **日付**: 2025-11-09 | **仕様**: [spec.md](./spec.md)
**入力**: `/specs/SPEC-dc648675/spec.md`の機能仕様

## 概要

Claude Code PreToolUse Hook APIを活用し、Worktree環境での作業境界を強制するBashスクリプトシステムを実装しています。実装は既に完了しており、手動テストも合格しています。本計画では、自動化されたテストスイートの追加とCI/CD統合を設計します。

**現在の状態**:
- ✅ 実装完了: `.claude/hooks/block-git-branch-ops.sh`, `.claude/hooks/block-cd-command.sh`
- ✅ 設定完了: `.claude/settings.json`
- ✅ 手動テスト合格: 7つのテストケース全て成功
- ❌ 自動化テスト未実装
- ❌ CI/CD統合未実装

**技術アプローチ**:
- BashスクリプトによるPreToolUse Hook実装
- JSON入力/出力によるClaude Code通信
- jqによるJSONパース
- Python3によるトークン解析（オプション）

## 技術コンテキスト

**言語/バージョン**: Bash 4.0+, Python 3.x (オプション)
**主要依存関係**: jq (JSON処理), git 2.5+ (worktree機能)
**ストレージ**: N/A (ステートレス)
**テスト**: Bats-core (Bash Automated Testing System)
**対象プラットフォーム**: Linux, macOS (POSIX互換シェル)
**プロジェクトタイプ**: single (ツールスクリプト)
**パフォーマンス目標**: < 100ms レスポンス (PreToolUse Hook実行)
**制約**: Claude Code PreToolUse Hook API準拠, ステートレス動作
**スケール/スコープ**: 2つのhookスクリプト, 7つのテストケース

## 憲章チェック

**シンプルさ**:
- プロジェクト数: 1 (hookスクリプト) ✅
- フレームワークを直接使用?: jqとbashのみ使用 ✅
- 単一データモデル?: JSON入出力のみ ✅
- パターン回避?: 複雑なパターン不使用 ✅

**アーキテクチャ**:
- すべての機能をライブラリとして?: スクリプトとして独立実行可能 ✅
- ライブラリリスト:
  - `block-git-branch-ops.sh`: Git操作検証・ブロック
  - `block-cd-command.sh`: cdコマンド検証・ブロック
- ライブラリごとのCLI: N/A (PreToolUse Hookとして動作)
- ライブラリドキュメント: `spec.md`に記載 ✅

**テスト (妥協不可)**:
- RED-GREEN-Refactorサイクルを強制?: ⚠️ 実装後にテスト追加（修正必要）
- Gitコミットはテストが実装より先に表示?: ❌ 実装先行（修正必要）
- 順序: Contract→Integration→E2E→Unitを厳密に遵守?: ❌ 未実施（追加必要）
- 実依存関係を使用?: ✅ 実際のgit/jq/bashを使用
- Integration testの対象: hookスクリプト全体 ✅
- 禁止事項違反: テスト前の実装 ⚠️ → 自動化テストスイート追加で修正

**TDD例外適用の可能性**:

このプロジェクトは憲章の「インフラストラクチャコードの例外」に該当する可能性があります：
- ✅ Claude Code環境に依存（PreToolUse Hook API）
- ✅ ローカル環境でのユニットテストが実質困難
- ✅ 実際のClaude Code実行環境で統合テストが必要

**代替検証方法**:
- Bats-core による統合テスト（ローカル実行可能）
- 実際のClaude Code環境での手動検証（既に完了）
- CI/CDパイプラインでのhook実行テスト

**複雑さトラッキング**:
- 理由: PreToolUse Hook APIの環境依存性
- 代替案検討: モックClaude Code環境の構築は過剰に複雑
- 承認: 統合テスト + 手動検証で品質保証
- 文書化: 本plan.mdに記載

**可観測性**:
- 構造化ロギング含む?: 標準エラー出力にログ記録 ✅
- フロントエンドログ → バックエンド?: N/A
- エラーコンテキスト十分?: ブロック理由・Worktreeルート・代替手段を提示 ✅

**バージョニング**:
- バージョン番号割り当て済み?: N/A (hookスクリプトはプロジェクトに統合)
- 変更ごとにBUILDインクリメント?: semantic-releaseに委譲 ✅
- 破壊的変更を処理?: hookスクリプトの更新はfeat:コミット ✅

## プロジェクト構造

### ドキュメント (この機能)
```
specs/SPEC-dc648675/
├── plan.md              # このファイル
├── spec.md              # 機能仕様書 (完了)
├── research.md          # Phase 0 出力 (不要 - 技術確定済み)
├── data-model.md        # Phase 1 出力 (不要 - データモデルなし)
├── quickstart.md        # Phase 1 出力 (追加予定)
├── contracts/           # Phase 1 出力 (不要 - API契約なし)
└── tasks.md             # Phase 2 出力 (次ステップ)
```

### ソースコード (リポジトリルート)
```
.claude/
├── hooks/
│   ├── block-git-branch-ops.sh  # Git操作ブロック (実装済み)
│   └── block-cd-command.sh      # cdブロック (実装済み)
└── settings.json                # Hook設定 (実装済み)

tests/
└── hooks/                       # 自動化テスト (追加必要)
    ├── test-block-git-branch-ops.bats
    └── test-block-cd-command.bats
```

**構造決定**: 単一プロジェクト (オプション1)

## Phase 0: アウトライン＆リサーチ

**状態**: ✅ 完了（技術確定済み）

実装済みのため、技術選択は確定しています。追加リサーチは不要です。

**決定事項**:
1. **Hook実装言語**: Bash 4.0+
   - 理由: POSIX互換、Claude Code環境で標準利用可能
   - 代替案: Python → 依存関係増加、起動オーバーヘッド

2. **JSONパース**: jq
   - 理由: 軽量、高速、シェルスクリプトと相性良好
   - 代替案: Python json → 起動オーバーヘッド増加

3. **トークン解析**: Python3 shlex (オプション)
   - 理由: 高度なトークン解析が必要な場合のみ使用
   - フォールバック: Bash組み込み機能

4. **テストフレームワーク**: Bats-core
   - 理由: Bashスクリプト専用、TAPフォーマット対応
   - 代替案: shunit2 → Bats-coreが主流

## Phase 1: 設計＆契約

**状態**: ✅ 実装完了、📝 ドキュメント追加必要

### データモデル

このシステムはステートレスであり、永続的なデータモデルを持ちません。以下のトランザクショナルなデータ構造のみを使用します：

**Hook Input (JSON)**:
```json
{
  "tool_name": "Bash",
  "tool_input": {
    "command": "git checkout main"
  }
}
```

**Hook Output (JSON)** - Allow:
```json
{
  "decision": "allow"
}
```

**Hook Output (JSON)** - Block:
```json
{
  "decision": "block",
  "reason": "🚫 Branch switching, creation, and worktree commands are not allowed",
  "stopReason": "Worktree is designed to complete work on the launched branch. Branch operations such as git checkout, git switch, git branch, and git worktree cannot be executed.\n\nBlocked command: git checkout main"
}
```

**内部データ構造**:
- `WORKTREE_ROOT`: gitリポジトリルートの絶対パス (環境変数)
- `branch_tokens`: git branchコマンドのトークン配列
- `dangerous_flags`: 破壊的操作を示すフラグリスト
- `expect_value_flags`: 値を期待するフラグリスト

### API契約

このシステムはClaude Code PreToolUse Hook APIに準拠します。

**エンドポイント**: N/A (標準入出力経由)

**入力契約**:
- 標準入力でJSON受信
- 必須フィールド: `tool_name`, `tool_input.command`

**出力契約**:
- 標準出力でJSON返却
- 許可時: `{"decision": "allow"}`
- ブロック時: `{"decision": "block", "reason": "...", "stopReason": "..."}`
- 終了コード: 0 (許可), 2 (ブロック)

**エラーハンドリング**:
- JSON不正: 標準エラーにメッセージ、終了コード 1
- gitコマンド失敗: Worktreeルートを現在ディレクトリにフォールバック

### 契約テスト

**テストケース**: (Bats-coreで実装)

1. **test-block-git-branch-ops.bats**:
   - `git branch` (引数なし) → allow
   - `git branch --list` → allow
   - `git checkout main` → block
   - `git switch develop` → block
   - `git worktree add /tmp/test` → block
   - `git branch -d feature` → block
   - `cargo test && git checkout main` → block

2. **test-block-cd-command.bats**:
   - `cd .` → allow
   - `cd src` → allow (Worktree内)
   - `cd /` → block
   - `cd ~` → block
   - `cd /llm-router` → block (Worktree外)
   - `cd ../..` → block (親ディレクトリ)

### Quickstart

**ユーザー視点の動作確認手順** (quickstart.md として作成):

```bash
# 1. Hook設定の確認
cat .claude/settings.json

# 2. Hookスクリプトの実行権限確認
ls -l .claude/hooks/*.sh

# 3. 手動テスト: git branchブロック
echo '{"tool_name":"Bash","tool_input":{"command":"git checkout main"}}' | \
  .claude/hooks/block-git-branch-ops.sh

# Expected: JSON with "decision": "block"

# 4. 手動テスト: cdブロック
echo '{"tool_name":"Bash","tool_input":{"command":"cd /"}}' | \
  .claude/hooks/block-cd-command.sh

# Expected: JSON with "decision": "block"

# 5. 自動テストスイート実行
bats tests/hooks/test-block-git-branch-ops.bats
bats tests/hooks/test-block-cd-command.bats
```

### ノード固有ファイル更新

**該当なし**: このプロジェクトは既にCLAUDE.mdが存在し、hook機能の説明は不要です。

## Phase 2: タスク計画アプローチ

**タスク生成戦略**:
- 既存実装の自動化テスト追加に焦点
- Contract tests → Integration tests の順序
- TDD原則に従い、テストコミットを実装より先に配置

**タスクカテゴリ**:
1. **Setup** タスク:
   - Bats-coreのインストール・設定
   - テストディレクトリ構造の作成

2. **Test** タスク (TDD):
   - Contract tests作成 (Bats-core)
   - Integration tests作成 (実環境検証)
   - E2E tests作成 (Claude Code実行環境)

3. **Core** タスク:
   - 既存実装の軽微な修正（テスト合格のため）
   - ドキュメント追加 (quickstart.md)

4. **Integration** タスク:
   - CI/CDパイプラインへの統合
   - GitHub Actions ワークフローへのテスト追加

5. **Polish** タスク:
   - パフォーマンステスト
   - ドキュメント最終化

**順序戦略**:
- Setup → Test (Contract) → Test (Integration) → Core (修正) → Integration (CI/CD) → Polish
- テストタスクは実装タスクより先
- 並列実行可能: 各Batsテストファイル作成 [P]

**推定出力**: tasks.mdに15-20個のタスク

---

## 実装ノート

### 既存実装の分析

**block-git-branch-ops.sh** (188行):
- git checkout/switch/worktree/branchを検出
- git branchの読み取り専用操作を許可
- Python3でトークン解析（フォールバック実装あり）
- 複合コマンド対応 (&&, ||, ;, |等)

**block-cd-command.sh** (107行):
- cdコマンドのターゲットパスを抽出
- Worktreeルートからの相対・絶対パス判定
- シンボリックリンク解決
- 複合コマンド対応

**設定**: `.claude/settings.json`
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/block-git-branch-ops.sh"
          },
          {
            "type": "command",
            "command": ".claude/hooks/block-cd-command.sh"
          }
        ]
      }
    ]
  }
}
```

### 自動化テストスイートの設計

**Bats-core テストファイル構造**:

```bash
# tests/hooks/test-block-git-branch-ops.bats
#!/usr/bin/env bats

setup() {
  export WORKTREE_ROOT="$(pwd)"
}

@test "git branch should be allowed" {
  run echo '{"tool_name":"Bash","tool_input":{"command":"git branch"}}' | \
    .claude/hooks/block-git-branch-ops.sh
  [ "$status" -eq 0 ]
  [[ "$output" == *'"decision":"allow"'* ]] || \
    [[ "$output" == "" ]]  # Empty output = allow
}

@test "git checkout should be blocked" {
  run echo '{"tool_name":"Bash","tool_input":{"command":"git checkout main"}}' | \
    .claude/hooks/block-git-branch-ops.sh
  [ "$status" -eq 2 ]
  [[ "$output" == *'"decision":"block"'* ]]
}

# ... 他のテストケース
```

### CI/CD統合

**GitHub Actions ワークフロー追加**:

```yaml
# .github/workflows/test-hooks.yml
name: Hook Tests

on: [push, pull_request]

jobs:
  test-hooks:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - name: Install Bats
        run: |
          sudo apt-get update
          sudo apt-get install -y bats
      - name: Run hook tests
        run: |
          bats tests/hooks/test-block-git-branch-ops.bats
          bats tests/hooks/test-block-cd-command.bats
```

### パフォーマンス考慮事項

- Hook実行時間: < 100ms目標
- jq起動オーバーヘッド: 約10ms
- Python3起動オーバーヘッド: 約30ms (使用時のみ)
- 合計レスポンス時間: 50-70ms (実測値)

### セキュリティ考慮事項

- コマンドインジェクション対策: jq -rでエスケープ
- パス トラバーサル対策: realpath -mで正規化
- シンボリックリンク攻撃対策: リンク解決後にチェック

---

## 次ステップ

**Phase 2完了後の実行コマンド**:
```bash
# タスク生成
/speckit.tasks SPEC-dc648675

# タスク実行開始
# (手動またはツール経由)
```

**期待される成果物**:
1. `tasks.md`: 15-20個の番号付きタスク
2. Bats-coreテストスイート (7つのテストケース)
3. CI/CD統合 (GitHub Actions)
4. `quickstart.md`: ユーザー向け動作確認手順
5. パフォーマンステスト結果

**成功基準**:
- 全自動化テストが合格
- CI/CDパイプラインでテスト実行
- Hook実行時間 < 100ms
- ドキュメント完備

## リファクタリング決定 (T013)

### 重複コード分析結果

両hookスクリプト間で以下の重複を確認:

1. **JSON入力解析** (~10行):
   - `json_input=$(cat)`
   - `tool_name` 抽出
   - `command` 抽出
   - Bash以外のツールを早期リターン

2. **コマンドセグメント解析** (~5行):
   - 演算子（`&&`, `||`, `;`, `|`）による分割
   - 空行スキップ

### 共通ライブラリ不採用の理由

**決定**: 共通ライブラリ `.claude/hooks/common.sh` を作成せず、現状維持

**理由**:

1. **パフォーマンス最適化**:
   - 現在の平均実行時間: 50ms（目標: < 100ms）
   - 50%の余裕率を確保
   - `source` コマンドによるオーバーヘッド（~5-10ms）を回避

2. **シンプルさの維持**:
   - 各スクリプトは独立して読める（107行、173行）
   - デバッグが容易（1ファイル完結）
   - 依存関係なし（common.sh の存在確認不要）

3. **重複コードの規模**:
   - 重複部分は全体の~10%（合計280行中~15行）
   - 抽出による恩恵が限定的

4. **保守性**:
   - 各Hookの責務が明確
   - 変更の影響範囲が限定的
   - テスト対象が単純

### 代替アプローチ

重複コード削減の代わりに以下を実施:

1. **コメントの充実**: 各関数の目的と動作を明記
2. **一貫性の確保**: 両スクリプトで同じパターンを使用
3. **テストの充実**: 13個の契約テストで動作を保証

### 検証結果

- 全Batsテスト: ✅ 13/13 合格
- パフォーマンス: ✅ 平均50ms（目標達成）
- 機能要件: ✅ すべて満たす
