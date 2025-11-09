#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

echo "==========================================="
echo "Creating Release Branch"
echo "==========================================="
echo ""

if ! command -v gh >/dev/null 2>&1; then
  echo "Error: GitHub CLI (gh) is not installed" >&2
  echo "Please install gh and rerun this script." >&2
  exit 1
fi

if ! gh auth status >/dev/null 2>&1; then
  echo "Error: GitHub CLI is not authenticated" >&2
  echo "Please run: gh auth login" >&2
  exit 1
fi

echo "[1/2] Triggering GitHub Actions workflow..."
gh workflow run create-release.yml --ref develop
echo "✓ Workflow triggered successfully"
echo ""

echo "[2/2] Monitoring workflow..."
echo ""
sleep 5
echo "Latest workflow runs:"
gh run list --workflow=create-release.yml --limit 3

echo ""
echo "==========================================="
echo "✓ Release branch creation initiated"
echo "==========================================="
echo ""
echo "To monitor progress, run:"
echo "  gh run watch \$(gh run list --workflow=create-release.yml --limit 1 --json databaseId --jq '.[0].databaseId')"
echo ""
echo "Or visit:"
echo "  https://github.com/$(gh repo view --json nameWithOwner --jq .nameWithOwner)/actions/workflows/create-release.yml"
