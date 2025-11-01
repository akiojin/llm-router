#!/bin/bash
set -e

cat <<'EOM' >&2
Error: create-new-feature.sh は無効化されています。

このリポジトリではブランチ／Worktreeを開発者が勝手に作成することは禁止されています（CLAUDE.md「環境固定ルール」参照）。
必要な場合はリポジトリメンテナに依頼してください。
EOM
exit 2
