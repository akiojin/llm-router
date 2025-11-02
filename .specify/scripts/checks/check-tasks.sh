#!/usr/bin/env bash

# check-tasks.sh - tasks.mdの全タスク完了チェック
#
# 使用方法:
#   check-tasks.sh <tasks.md のパス>
#
# 戻り値:
#   0: すべてのタスクが完了
#   1: 未完了のタスクが存在
#   2: エラー（ファイルが存在しない等）

set -e

# 引数チェック - 引数がない場合はCI環境変数から推定
if [ $# -eq 0 ]; then
    # CI環境でブランチ名から SPEC-ID を抽出
    if [ -n "$GITHUB_HEAD_REF" ]; then
        # Pull request の場合
        BRANCH_NAME="$GITHUB_HEAD_REF"
    elif [ -n "$GITHUB_REF_NAME" ]; then
        # Push の場合
        BRANCH_NAME="$GITHUB_REF_NAME"
    else
        echo "Error: No tasks.md path provided and not in CI environment" >&2
        echo "Usage: $0 <tasks.md path>" >&2
        exit 2
    fi

    # ブランチ名が feature/SPEC-xxx 形式の場合、SPEC-ID を抽出
    if [[ "$BRANCH_NAME" =~ ^feature/(SPEC-[a-z0-9]{8})$ ]]; then
        SPEC_ID="${BASH_REMATCH[1]}"
        TASKS_FILE="specs/$SPEC_ID/tasks.md"
        echo "Auto-detected SPEC-ID: $SPEC_ID from branch: $BRANCH_NAME"
    else
        echo "⚠️  Branch name does not match feature/SPEC-xxx pattern: $BRANCH_NAME"
        echo "   Skipping tasks check"
        exit 0
    fi
elif [ $# -eq 1 ]; then
    TASKS_FILE="$1"
else
    echo "Usage: $0 [<tasks.md path>]" >&2
    echo "Example: $0 specs/SPEC-12345678/tasks.md" >&2
    exit 2
fi

# ファイル存在チェック
if [ ! -f "$TASKS_FILE" ]; then
    echo "Error: tasks.md not found: $TASKS_FILE" >&2
    exit 2
fi

echo "Checking tasks in: $TASKS_FILE"

# 未完了タスクのパターン: - [ ] または -[ ]（スペースの有無）
UNCOMPLETED=$(grep -E '^\s*-\s*\[\s*\]' "$TASKS_FILE" || true)

if [ -n "$UNCOMPLETED" ]; then
    echo ""
    echo "❌ Uncompleted tasks found:"
    echo "$UNCOMPLETED"
    echo ""

    # 完了済みと未完了の数をカウント
    COMPLETED_COUNT=$(grep -cE '^\s*-\s*\[x\]' "$TASKS_FILE" || echo "0")
    UNCOMPLETED_COUNT=$(grep -cE '^\s*-\s*\[\s*\]' "$TASKS_FILE" || echo "0")
    TOTAL=$((COMPLETED_COUNT + UNCOMPLETED_COUNT))

    echo "Progress: $COMPLETED_COUNT/$TOTAL tasks completed"
    exit 1
fi

# すべて完了している場合
COMPLETED_COUNT=$(grep -cE '^\s*-\s*\[x\]' "$TASKS_FILE" || echo "0")

if [ "$COMPLETED_COUNT" -eq 0 ]; then
    echo "⚠️  Warning: No tasks found in $TASKS_FILE"
    echo "   This might be okay if tasks are not yet defined."
    exit 0
fi

echo "✅ All $COMPLETED_COUNT tasks are completed!"
exit 0
