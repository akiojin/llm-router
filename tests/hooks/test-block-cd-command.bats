#!/usr/bin/env bats

# block-cd-command.sh の契約テスト
# Claude Code PreToolUse Hook API 仕様に基づく動作検証

setup() {
    # hookスクリプトのパス
    HOOK_SCRIPT=".claude/hooks/block-cd-command.sh"

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

# テストケース1: cd . → allow (Worktree内、exit 0)
@test "cd . is allowed" {
    run run_hook "cd ."
    [ "$status" -eq 0 ]
}

# テストケース2: cd src → allow (Worktree内、exit 0)
@test "cd src is allowed (within worktree)" {
    run run_hook "cd src"
    [ "$status" -eq 0 ]
}

# テストケース3: cd / → block (exit 2)
@test "cd / is blocked" {
    run run_hook "cd /"
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}

# テストケース4: cd ~ → block (exit 2)
@test "cd ~ is blocked" {
    run run_hook "cd ~"
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}

# テストケース5: cd /runtime-coordinator → block (Worktree外、exit 2)
@test "cd /runtime-coordinator is blocked (outside worktree)" {
    run run_hook "cd /runtime-coordinator"
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}

# テストケース6: cd ../.. → block (親ディレクトリ、exit 2)
@test "cd ../.. is blocked (parent directory)" {
    run run_hook "cd ../.."
    [ "$status" -eq 2 ]
    decision=$(get_decision)
    [ "$decision" = "block" ]
}
