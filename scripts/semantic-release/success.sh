#!/usr/bin/env bash
# Record release information for downstream packaging jobs.

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <version> <git-tag>" >&2
  exit 1
fi

VERSION="$1"
TAG="$2"

mkdir -p release
cat <<EOF > release/semantic-release.json
{
  "version": "${VERSION}",
  "tag": "${TAG}"
}
EOF
