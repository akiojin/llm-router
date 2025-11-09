#!/bin/bash

# Hook Performance Benchmark Script
# 目的: Claude Code PreToolUse Hookの実行時間を測定
# 目標: < 100ms/実行

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# 測定回数
ITERATIONS=100

echo "========================================="
echo "Hook Performance Benchmark"
echo "========================================="
echo "Iterations: $ITERATIONS"
echo "Target: < 100ms per execution"
echo ""

# ベンチマーク関数
benchmark_hook() {
    local hook_name="$1"
    local hook_path="$2"
    local test_command="$3"
    local json_input="{\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"$test_command\"}}"

    echo "Benchmarking: $hook_name"
    echo "Test command: $test_command"

    local total_time=0
    local min_time=999999
    local max_time=0

    for i in $(seq 1 $ITERATIONS); do
        local start=$(date +%s%N)
        echo "$json_input" | "$hook_path" > /dev/null 2>&1 || true
        local end=$(date +%s%N)

        local elapsed=$(( (end - start) / 1000000 ))  # ナノ秒をミリ秒に変換
        total_time=$((total_time + elapsed))

        if [ $elapsed -lt $min_time ]; then
            min_time=$elapsed
        fi

        if [ $elapsed -gt $max_time ]; then
            max_time=$elapsed
        fi
    done

    local avg_time=$((total_time / ITERATIONS))

    echo "  Average: ${avg_time}ms"
    echo "  Min: ${min_time}ms"
    echo "  Max: ${max_time}ms"

    if [ $avg_time -lt 100 ]; then
        echo "  Status: ✅ PASS (< 100ms)"
    else
        echo "  Status: ❌ FAIL (>= 100ms)"
    fi

    echo ""

    # 結果を配列に格納（後で使用）
    eval "${hook_name}_avg=$avg_time"
    eval "${hook_name}_min=$min_time"
    eval "${hook_name}_max=$max_time"
}

# block-git-branch-ops.sh のベンチマーク
benchmark_hook "git_allow" \
    "$PROJECT_ROOT/.claude/hooks/block-git-branch-ops.sh" \
    "git branch"

benchmark_hook "git_block" \
    "$PROJECT_ROOT/.claude/hooks/block-git-branch-ops.sh" \
    "git checkout main"

# block-cd-command.sh のベンチマーク
benchmark_hook "cd_allow" \
    "$PROJECT_ROOT/.claude/hooks/block-cd-command.sh" \
    "cd ."

benchmark_hook "cd_block" \
    "$PROJECT_ROOT/.claude/hooks/block-cd-command.sh" \
    "cd /"

# サマリー
echo "========================================="
echo "Summary"
echo "========================================="
echo ""
echo "block-git-branch-ops.sh:"
echo "  Allow (git branch): ${git_allow_avg}ms (min: ${git_allow_min}ms, max: ${git_allow_max}ms)"
echo "  Block (git checkout): ${git_block_avg}ms (min: ${git_block_min}ms, max: ${git_block_max}ms)"
echo ""
echo "block-cd-command.sh:"
echo "  Allow (cd .): ${cd_allow_avg}ms (min: ${cd_allow_min}ms, max: ${cd_allow_max}ms)"
echo "  Block (cd /): ${cd_block_avg}ms (min: ${cd_block_min}ms, max: ${cd_block_max}ms)"
echo ""

# 全体の平均
total_avg=$(( (git_allow_avg + git_block_avg + cd_allow_avg + cd_block_avg) / 4 ))
echo "Overall average: ${total_avg}ms"

if [ $total_avg -lt 100 ]; then
    echo "Overall status: ✅ PASS"
    exit 0
else
    echo "Overall status: ❌ FAIL"
    exit 1
fi
