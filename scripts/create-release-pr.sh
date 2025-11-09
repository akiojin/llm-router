#!/usr/bin/env bash
# create-release-pr.sh

set -euo pipefail

CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [[ "$CURRENT_BRANCH" != "develop" ]]; then
    echo "ERROR: Must be on develop branch"
    exit 1
fi

git pull origin develop

gh pr create \
  --base main \
  --head develop \
  --title "Release: $(date +%Y-%m-%d)" \
  --body "Automatic release PR from develop to main.

After merge, semantic-release will:
- Determine version from Conventional Commits
- Update package.json and CHANGELOG.md
- Create Git tag
- Create GitHub Release"

echo "Release PR created successfully"
