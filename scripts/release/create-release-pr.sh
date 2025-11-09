#!/usr/bin/env bash

# create-release-pr.sh
# develop → main PR作成スクリプト

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# カラー定義
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# エラーハンドラ
error() {
    echo -e "${RED}[ERROR]${NC} $1" >&2
    exit 1
}

info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

# 前提条件チェック
check_prerequisites() {
    info "前提条件をチェック中..."

    # GitHubCLIの確認
    if ! command -v gh &> /dev/null; then
        error "GitHub CLI (gh) がインストールされていません"
    fi

    # Gitリポジトリの確認
    if ! git rev-parse --is-inside-work-tree &> /dev/null; then
        error "Gitリポジトリ内で実行してください"
    fi

    # developブランチの存在確認
    if ! git rev-parse --verify develop &> /dev/null; then
        error "developブランチが存在しません。先にdevelopブランチを作成してください"
    fi

    # mainブランチの存在確認
    if ! git rev-parse --verify main &> /dev/null; then
        error "mainブランチが存在しません"
    fi

    success "前提条件チェック完了"
}

# リモート同期
sync_branches() {
    info "ブランチをリモートと同期中..."

    git fetch origin develop:develop 2>/dev/null || warning "developブランチの同期に失敗（ローカルのみの可能性）"
    git fetch origin main:main || error "mainブランチの同期に失敗"

    success "ブランチ同期完了"
}

# 必要なラベルの確認・作成
ensure_labels() {
    info "必要なラベルを確認中..."

    declare -A LABEL_META=(
        ["release"]="1f883d|Release PR tracking"
        ["auto-merge"]="5319e7|Auto-merge eligible"
    )

    mapfile -t EXISTING_LABELS < <(gh label list --limit 200 --json name --jq '.[].name' 2>/dev/null || echo "")

    for label in "${!LABEL_META[@]}"; do
        if printf '%s\n' "${EXISTING_LABELS[@]}" | grep -Fxq "$label"; then
            info "ラベル '$label' は既に存在します"
            continue
        fi

        COLOR="${LABEL_META[$label]%|*}"
        DESC="${LABEL_META[$label]#*|}"
        warning "ラベル '$label' が存在しません。作成します..."

        if gh label create "$label" --color "$COLOR" --description "$DESC" >/dev/null 2>&1; then
            success "ラベル '$label' を作成しました"
        else
            error "ラベル '$label' の作成に失敗しました"
        fi
    done
}

# 既存PR確認
check_existing_pr() {
    info "既存のPRを確認中..."

    EXISTING_PR=$(gh pr list --base main --head develop --json number --jq '.[0].number' 2>/dev/null || echo "")

    if [ -n "$EXISTING_PR" ]; then
        warning "develop → main のPRが既に存在します: #$EXISTING_PR"
        echo ""
        read -p "既存のPRを使用しますか? (y/N): " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            error "処理を中止しました"
        fi
        info "既存のPR #$EXISTING_PR を使用します"
        return 0
    fi

    info "新規PR作成が可能です"
}

# PRテンプレート生成
generate_pr_body() {
    cat <<EOF
## リリース概要

このPRはdevelopブランチからmainブランチへの正式リリースを開始します。

## リリース内容

\`\`\`bash
# developとmainの差分を確認
git log main..develop --oneline
\`\`\`

## チェックリスト

- [ ] すべての品質チェックが合格している
- [ ] CHANGELOG.mdの内容を確認した
- [ ] 重大なバグが残っていないことを確認した
- [ ] ドキュメントが最新である
- [ ] リリースノートの内容を確認した

## リリース後の自動処理

マージ後、以下が自動実行されます：

1. semantic-releaseによるバージョン番号の自動計算
2. CHANGELOG.mdの自動更新
3. Cargo.tomlの自動更新
4. GitHubタグとリリースの自動作成
5. 全プラットフォームのバイナリ自動ビルド・公開

---

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
}

# PR作成
create_pr() {
    info "develop → main PR を作成中..."

    PR_BODY=$(generate_pr_body)

    # Conventional Commitsから次のバージョンを推測（簡易版）
    COMMITS=$(git log main..develop --pretty=format:"%s")
    VERSION_TYPE="パッチ"

    if echo "$COMMITS" | grep -q "^feat"; then
        VERSION_TYPE="マイナー"
    fi

    if echo "$COMMITS" | grep -q "BREAKING CHANGE"; then
        VERSION_TYPE="メジャー"
    fi

    PR_TITLE="chore(release): ${VERSION_TYPE}バージョンリリース準備"

    if [ -n "$EXISTING_PR" ]; then
        info "既存のPR #$EXISTING_PR を更新します"
        gh pr edit "$EXISTING_PR" --body "$PR_BODY" || warning "PR本文の更新に失敗"
        PR_URL=$(gh pr view "$EXISTING_PR" --json url --jq '.url')
    else
        PR_URL=$(gh pr create \
            --base main \
            --head develop \
            --title "$PR_TITLE" \
            --body "$PR_BODY" \
            --label "release,auto-merge" \
        ) || error "PR作成に失敗"
    fi

    success "PR作成完了: $PR_URL"
}

# 次のステップ表示
show_next_steps() {
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    info "次のステップ:"
    echo ""
    echo "  1. PRの品質チェックが完了するまで待機"
    echo "  2. 品質チェック合格後、自動的にmainにマージされます"
    echo "  3. マージ後、semantic-releaseが自動実行されます"
    echo "  4. 約30分以内にリリースとバイナリが公開されます"
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
}

# メイン処理
main() {
    cd "$PROJECT_ROOT"

    echo ""
    info "🚀 正式リリースPR作成スクリプト"
    echo ""

    check_prerequisites
    ensure_labels
    sync_branches
    check_existing_pr
    create_pr
    show_next_steps

    success "✅ リリースプロセスが開始されました"
}

main "$@"
