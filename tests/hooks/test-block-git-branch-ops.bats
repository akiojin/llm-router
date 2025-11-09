#!/usr/bin/env bats

# block-git-branch-ops.sh の契約テスト
# Claude Code PreToolUse Hook API 仕様に基づく動作検証

setup() {
    # hookスクリプトのパス
    HOOK_SCRIPT=".claude/hooks/block-git-branch-ops.sh"

    # hookスクリプトが存在し、実行可能であることを確認
    [ -x "$HOOK_SCRIPT" ]
}

# ヘルパー関数: JSON入力を生成してhookを実行
run_hook() {
    local command="$1"
    echo "{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"$command\"}}" | "$HOOK_SCRIPT" 2>&1
}

# ヘルパー関数: JSONレスポンスから"decision"フィールドを抽出
# 出力にはstderrメッセージとJSONが混在しているため、JSON部分のみを抽出
get_decision() {
    echo "$output" | sed -n '/{/,/^}/p' | jq -r '.decision // empty' 2>/dev/null || echo ""
}

# テストケース1: git branch (引数なし) → allow (exit 0)
@test "git branch without arguments is allowed" {
    run run_hook "git branch"
    [ "$status" -eq 0 ]
}

# テストケース2: git branch --list → allow (exit 0)
@test "git branch --list is allowed" {
    run run_hook "git branch --list"
    [ "$status" -eq 0 ]
}

# テストケース3: git checkout main → block (exit 2)
@test "git checkout is blocked" {
    run run_hook "git checkout main"
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}

# テストケース4: git switch develop → block (exit 2)
@test "git switch is blocked" {
    run run_hook "git switch develop"
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}

# テストケース5: git worktree add /tmp/test → block (exit 2)
@test "git worktree add is blocked" {
    run run_hook "git worktree add /tmp/test"
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}

# テストケース6: git branch -d feature → block (exit 2)
@test "git branch -d is blocked" {
    run run_hook "git branch -d feature"
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}

# テストケース7: cargo test && git checkout main → block (exit 2)
@test "compound command with git checkout is blocked" {
    run run_hook "cargo test && git checkout main"
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}
